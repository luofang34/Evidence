//! Ratcheting floors — per-dimension measurements enforcing "rigor only
//! goes up" (principle 2 of the development plan).
//!
//! This module exports the **measurement** half of the floors gate:
//! deterministic helpers that compute the current value along every
//! dimension the tool ratchets. The comparison half (reading
//! `cert/floors.toml`, diffing against these measurements, emitting
//! `FLOORS_BELOW_MIN`) lives in the CLI `cargo evidence floors`
//! subcommand.
//!
//! Source-walking primitives (`walk_rs_files`, `strip_cfg_test_modules`,
//! `needle_is_outside_string_literal`) live in the sibling
//! `walker` module so this file stays under the 500-line
//! workspace limit.
//!
//! **Scope of "library panics".** The `count_library_panics` walk
//! targets production code under `crates/*/src/**`, excluding the
//! cargo-evidence `main.rs` (the anyhow error-envelope layer, per
//! the existing `clippy.toml` carve-out) and excluding `#[cfg(test)]`
//! module blocks at indent 0. Test-only panics, doc-comment
//! examples, plain-string literals, and raw-string literals
//! (`r"…"` / `r#"…"#`) are not counted. The current count is 0; the
//! floors gate pins zero-new-additions.

mod walker;

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::trace::read_all_trace_files;

// Walker helpers are private to this module — no stable-API
// promise, no crate-wide visibility. Callers outside the crate go
// through `current_measurements` / `per_crate_measurements`; callers
// elsewhere in the crate have no need for these primitives.
use walker::{needle_is_outside_string_literal, strip_cfg_test_modules, walk_rs_files};

/// Current schema version for `cert/floors.toml`. Additive field
/// changes stay at `1`; a break in the shape (removed field,
/// renamed table, incompatible type change) bumps this. Parsers
/// refuse to proceed on an unknown future version.
pub const FLOORS_SCHEMA_VERSION: u32 = 1;

/// On-disk shape of `cert/floors.toml`.
///
/// Three tables:
/// - `[floors]` — workspace-wide absolute floors (one value applies
///   to the whole workspace). Used for dimensions with a single
///   global identity: `diagnostic_codes` (from `RULES` in one
///   crate), `terminal_codes`, per-layer trace entry counts.
/// - `[per_crate.<crate>]` — per-crate absolute floors. Used for
///   dimensions where a compensation mask would be invisible on a
///   workspace aggregate — e.g. evidence adds a `panic!`, cargo-
///   evidence removes one, total stays the same. Every in-scope
///   crate in `cert/boundary.toml` must appear here (enforced by
///   a bijection test).
/// - `[delta_ceilings]` — `added_in_pr_diff <= ceiling`. Applied
///   only when the gate runs with a `--base <ref>` scope. Parsed
///   today, reported as `deferred` pending the diff-enforcement
///   commit.
///
/// The top-level `schema_version` field pins the shape; older tools
/// refuse to parse a newer version rather than silently skip unknown
/// fields.
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct FloorsConfig {
    /// Shape version; must equal [`FLOORS_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Workspace-wide absolute floors: `current >= value`.
    pub floors: BTreeMap<String, u64>,
    /// Per-crate absolute floors. Key = crate name (must match
    /// `cert/boundary.toml` `scope.in_scope`); value = per-crate
    /// `{dimension → floor}` map.
    pub per_crate: BTreeMap<String, BTreeMap<String, u64>>,
    /// Per-crate absolute **ceilings**: `current <= value`. For
    /// dimensions where *fewer is better* — library panics,
    /// deprecated-API uses, etc. A floor and a ceiling share the
    /// same "absolute, checked every CI run" semantics; they differ
    /// only in comparison direction. Same key-set rules as
    /// [`FloorsConfig::per_crate`]: each crate name must match
    /// `cert/boundary.toml` `scope.in_scope`.
    pub per_crate_ceilings: BTreeMap<String, BTreeMap<String, u64>>,
    /// Delta ceilings: `added_in_diff <= value`.
    pub delta_ceilings: BTreeMap<String, u64>,
}

impl Default for FloorsConfig {
    fn default() -> Self {
        Self {
            schema_version: FLOORS_SCHEMA_VERSION,
            floors: BTreeMap::new(),
            per_crate: BTreeMap::new(),
            per_crate_ceilings: BTreeMap::new(),
            delta_ceilings: BTreeMap::new(),
        }
    }
}

/// Outcome of attempting to load `cert/floors.toml`. Distinguishes
/// a deliberately-absent file (downstream user hasn't opted into
/// the gate) from a malformed file (must fail hard — silent skip
/// would mask drift on a typo'd path).
pub enum LoadOutcome {
    /// Successfully parsed config.
    Loaded(FloorsConfig),
    /// File not found at the expected path. Downstream users who
    /// haven't adopted the floors gate hit this case; the CLI emits
    /// a friendly "no floors configured" message and exits 0.
    Missing,
    /// File exists but couldn't be read or parsed.
    Error(String),
}

impl FloorsConfig {
    /// Parse `cert/floors.toml` at `path`. Malformed files return
    /// an Err; callers lift into their own envelope.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            fs::read_to_string(path).map_err(|e| format!("reading {}: {}", path.display(), e))?;
        let cfg: Self =
            toml::from_str(&text).map_err(|e| format!("parsing {}: {}", path.display(), e))?;
        if cfg.schema_version != FLOORS_SCHEMA_VERSION {
            return Err(format!(
                "{}: schema_version {} is not supported by this tool version (expected {}); \
                 either upgrade the tool or pin to a config version it understands",
                path.display(),
                cfg.schema_version,
                FLOORS_SCHEMA_VERSION
            ));
        }
        Ok(cfg)
    }

    /// Like [`load`], but distinguishes "file not found" from other
    /// errors. Used by `cargo evidence floors` so external projects
    /// without a `cert/floors.toml` don't see a scary error — they
    /// just haven't opted in yet.
    ///
    /// [`load`]: Self::load
    pub fn load_or_missing(path: &Path) -> LoadOutcome {
        match fs::read_to_string(path) {
            Ok(text) => match toml::from_str::<Self>(&text) {
                Ok(cfg) => {
                    if cfg.schema_version != FLOORS_SCHEMA_VERSION {
                        return LoadOutcome::Error(format!(
                            "{}: schema_version {} is not supported (expected {})",
                            path.display(),
                            cfg.schema_version,
                            FLOORS_SCHEMA_VERSION
                        ));
                    }
                    LoadOutcome::Loaded(cfg)
                }
                Err(e) => LoadOutcome::Error(format!("parsing {}: {}", path.display(), e)),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => LoadOutcome::Missing,
            Err(e) => LoadOutcome::Error(format!("reading {}: {}", path.display(), e)),
        }
    }
}

/// Collect every **workspace-wide** absolute measurement, keyed by
/// the same names `[floors]` uses. Per-crate dimensions
/// (`test_count`, `library_panics`) have a separate aggregator —
/// see [`per_crate_measurements`].
///
/// Callers diff this map against [`FloorsConfig::floors`] and emit
/// `FLOORS_BELOW_MIN` for any dimension where the measurement falls
/// below its committed floor.
pub fn current_measurements(workspace_root: &Path) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    out.insert("diagnostic_codes".into(), count_rules());
    out.insert("terminal_codes".into(), count_terminals());

    let (sys, hlr, llr, test) = count_trace_per_layer(workspace_root);
    out.insert("trace_sys".into(), sys);
    out.insert("trace_hlr".into(), hlr);
    out.insert("trace_llr".into(), llr);
    out.insert("trace_test".into(), test);

    out.insert("known_surfaces".into(), count_known_surfaces());
    out
}

/// Count entries in `evidence::trace::surfaces::KNOWN_SURFACES`.
///
/// The surface catalog is hand-curated — shrinking it without a
/// corresponding HLR update would relax the
/// `require_hlr_surface_bijection` check silently. The floor is the
/// guardrail: removing a surface requires raising or lowering the
/// floor in the same PR.
pub fn count_known_surfaces() -> u64 {
    crate::trace::KNOWN_SURFACES.len() as u64
}

/// Per-crate absolute measurements. Outer key = crate name (directory
/// under `crates/`); inner map = `{dimension → current value}` with
/// the same names `[per_crate.<crate>]` uses in `cert/floors.toml`.
///
/// Crates that don't exist as `crates/<name>/` directories are
/// omitted; the CLI cross-checks against `FloorsConfig::per_crate`
/// keys and emits a `FLOORS_BELOW_MIN` row with current=0 for any
/// declared crate that's gone missing.
///
/// Why per-crate for these two dimensions specifically: a workspace-
/// wide aggregate would mask compensation (evidence adds a panic,
/// cargo-evidence removes one, total stays the same). Every other
/// ratcheted dimension in this file has a single global identity;
/// these two do not.
pub fn per_crate_measurements(workspace_root: &Path) -> BTreeMap<String, BTreeMap<String, u64>> {
    let mut out: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let crates_root = workspace_root.join("crates");
    let entries = match fs::read_dir(&crates_root) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let mut per = BTreeMap::new();
        per.insert("test_count".into(), count_tests(&path));
        per.insert("library_panics".into(), count_library_panics(&path));
        out.insert(name.to_string(), per);
    }
    out
}

/// `evidence::RULES` length — locked by bijection invariants,
/// so any code addition is already reviewer-visible. The floor makes
/// deletion reviewer-visible too.
pub fn count_rules() -> u64 {
    crate::rules::RULES.len() as u64
}

/// `evidence::TERMINAL_CODES` length.
pub fn count_terminals() -> u64 {
    crate::TERMINAL_CODES.len() as u64
}

/// Per-layer trace entry counts by reading `tool/trace/` through the
/// production loader (same path `trace --validate` uses, so a future
/// loader change is reflected here). Returns `(sys, hlr, llr, test)`;
/// on load failure every layer is reported as 0 — the caller's floor
/// comparison will fire and name the affected dimension.
pub fn count_trace_per_layer(workspace_root: &Path) -> (u64, u64, u64, u64) {
    let trace_root = workspace_root.join("tool").join("trace");
    let Some(trace_root_str) = trace_root.to_str() else {
        return (0, 0, 0, 0);
    };
    match read_all_trace_files(trace_root_str) {
        Ok(tf) => (
            tf.sys.requirements.len() as u64,
            tf.hlr.requirements.len() as u64,
            tf.llr.requirements.len() as u64,
            tf.tests.tests.len() as u64,
        ),
        Err(_) => (0, 0, 0, 0),
    }
}

/// Count `#[test]` attribute occurrences inside `root` recursively.
/// Callers pass a whole-workspace root or a single-crate directory
/// depending on scope.
///
/// **Convention**: only the canonical `#[test]` form counts.
/// Custom test attributes (`#[tokio::test]`, `#[test_case(…)]`,
/// `#[rstest]`, etc.) are NOT counted — they would over-count or
/// require per-framework special cases, and the project's own code
/// uses only `#[test]`. A downstream project that uses custom test
/// attributes should either wrap them in a macro that emits
/// `#[test]`, or leave the `test_count` floor out of their
/// `cert/floors.toml` and rely on per-crate CI coverage counters
/// instead.
pub fn count_tests(root: &Path) -> u64 {
    let files = walk_rs_files(root);
    let mut total: u64 = 0;
    for file in &files {
        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == "#[test]" {
                total += 1;
            }
        }
    }
    total
}

/// Count non-test `panic!`, `unimplemented!`, `todo!` invocations in
/// production library source. Multi-layer filter:
///
/// 1. Skips `crates/*/tests/` (integration tests).
/// 2. Skips any file named `tests.rs` anywhere in the tree. These
///    are sibling unit-test modules pulled in via
///    `#[cfg(test)] #[path = "foo/tests.rs"] mod tests;` — test code
///    by convention, regardless of how the parent module attaches
///    them.
/// 3. Skips `main.rs` files (the CLI's anyhow envelope layer).
/// 4. Strips top-level `#[cfg(test)]` module blocks via the
///    internal `strip_cfg_test_modules` walker.
/// 5. Skips `//`, `///`, and `//!` comment lines.
/// 6. Uses the internal `needle_is_outside_string_literal` helper
///    to reject occurrences inside plain or raw string literals.
///
/// Remaining edge cases (tracked, not currently blocking):
///
/// - Escaped quotes inside non-raw strings (`"foo\"panic!("`).
/// - `char` literals containing `"` (`'"'`).
/// - Nested `#[cfg(test)]` sub-modules survive the strip.
///
/// A hand-curated allowlist at the file level is the escape hatch
/// if any of these produces a false positive in practice.
pub fn count_library_panics(root: &Path) -> u64 {
    let files = walk_rs_files(root);
    let mut total: u64 = 0;
    for file in &files {
        // Exclude tests/ dirs (integration tests) and any sibling
        // `tests.rs` facade-module. Windows paths use `\` natively,
        // so normalize before substring-matching.
        let normalized = file.to_string_lossy().replace('\\', "/");
        if normalized.contains("/tests/") || normalized.ends_with("/tests.rs") {
            continue;
        }
        // Exclude the CLI main.rs (anyhow envelope layer).
        if file.file_name().and_then(|n| n.to_str()) == Some("main.rs") {
            continue;
        }
        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let stripped = strip_cfg_test_modules(&content);
        for line in stripped.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//!") || trimmed.starts_with("///") || trimmed.starts_with("//")
            {
                continue;
            }
            for needle in ["panic!(", "unimplemented!(", "todo!("] {
                if needle_is_outside_string_literal(line, needle) {
                    total += 1;
                }
            }
        }
    }
    total
}

// Tests live in a sibling file pulled in via `#[path]` so the facade
// stays under the 500-line workspace limit. The naming collision
// with the parent `mod floors;` is avoided by pointing directly at
// the file.
#[cfg(test)]
#[path = "floors/tests.rs"]
mod tests;
