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

pub use walker::{needle_is_outside_string_literal, strip_cfg_test_modules, walk_rs_files};

/// Current schema version for `cert/floors.toml`. Additive field
/// changes stay at `1`; a break in the shape (removed field,
/// renamed table, incompatible type change) bumps this. Parsers
/// refuse to proceed on an unknown future version.
pub const FLOORS_SCHEMA_VERSION: u32 = 1;

/// On-disk shape of `cert/floors.toml`.
///
/// Two tables:
/// - `[floors]` — absolute `current >= floor` contracts, one entry
///   per measured dimension.
/// - `[delta_ceilings]` — `added_in_pr_diff <= ceiling`. Applied only
///   when the gate runs with a `--base <ref>` scope. Parsed today,
///   reported as `deferred` pending the diff-enforcement commit.
///
/// The top-level `schema_version` field pins the shape; older tools
/// refuse to parse a newer version rather than silently skip unknown
/// fields.
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct FloorsConfig {
    /// Shape version; must equal [`FLOORS_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Absolute floors: `current >= value`.
    pub floors: BTreeMap<String, u64>,
    /// Delta ceilings: `added_in_diff <= value`.
    pub delta_ceilings: BTreeMap<String, u64>,
}

impl Default for FloorsConfig {
    fn default() -> Self {
        Self {
            schema_version: FLOORS_SCHEMA_VERSION,
            floors: BTreeMap::new(),
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

/// Collect every absolute measurement, keyed by the same names the
/// floors TOML uses. Callers diff this map against
/// [`FloorsConfig::floors`] and emit `FLOORS_BELOW_MIN` for each
/// dimension whose measurement is below its committed floor.
pub fn current_measurements(workspace_root: &Path) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    out.insert("diagnostic_codes".into(), count_rules());
    out.insert("terminal_codes".into(), count_terminals());

    let (sys, hlr, llr, test) = count_trace_per_layer(workspace_root);
    out.insert("trace_sys".into(), sys);
    out.insert("trace_hlr".into(), hlr);
    out.insert("trace_llr".into(), llr);
    out.insert("trace_test".into(), test);

    out.insert("test_count".into(), count_tests(workspace_root));
    out.insert(
        "library_panics".into(),
        count_library_panics(workspace_root),
    );
    out
}

/// `evidence::RULES` length — locked by PR #47's bijection invariants,
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

/// Count `#[test]` attribute occurrences across `crates/**/*.rs`.
///
/// **Convention**: only the canonical `#[test]` form counts.
/// Custom test attributes (`#[tokio::test]`, `#[test_case(…)]`,
/// `#[rstest]`, etc.) are NOT counted — they would over-count or
/// require per-framework special cases, and the project's own code
/// uses only `#[test]`. A downstream project that uses custom
/// test attributes should either wrap them in a macro that emits
/// `#[test]`, or leave the `test_count` floor out of their
/// `cert/floors.toml` and rely on per-crate CI coverage counters
/// instead.
pub fn count_tests(workspace_root: &Path) -> u64 {
    let crates = workspace_root.join("crates");
    let mut files = Vec::new();
    walk_rs_files(&crates, &mut files);
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
/// 2. Skips `main.rs` files (the CLI's anyhow envelope layer).
/// 3. Strips top-level `#[cfg(test)]` module blocks via
///    [`strip_cfg_test_modules`].
/// 4. Skips `//`, `///`, and `//!` comment lines.
/// 5. Uses [`needle_is_outside_string_literal`] to reject
///    occurrences inside plain or raw string literals.
///
/// Remaining edge cases (tracked, not currently blocking):
///
/// - Escaped quotes inside non-raw strings (`"foo\"panic!("`).
/// - `char` literals containing `"` (`'"'`).
/// - Nested `#[cfg(test)]` sub-modules survive the strip.
///
/// A hand-curated allowlist at the file level is the escape hatch
/// if any of these produces a false positive in practice.
pub fn count_library_panics(workspace_root: &Path) -> u64 {
    let crates = workspace_root.join("crates");
    let mut files = Vec::new();
    walk_rs_files(&crates, &mut files);
    let mut total: u64 = 0;
    for file in &files {
        // Exclude tests/ dirs (integration tests). Windows paths use
        // `\` natively, so normalize before substring-matching.
        let normalized = file.to_string_lossy().replace('\\', "/");
        if normalized.contains("/tests/") {
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/")
            .parent()
            .expect("workspace root")
            .to_path_buf()
    }

    /// Single-source-of-truth regression: the committed
    /// `cert/floors.toml` must be satisfied by the current tree. No
    /// hardcoded per-dimension expected values — the test reads the
    /// TOML and asserts `current >= floor` for each entry. If a
    /// measurement helper drifts, this test fires with the dimension
    /// and both values in the panic message.
    #[test]
    fn current_measurements_satisfy_committed_floors() {
        let root = workspace_root();
        let floors_toml = root.join("cert").join("floors.toml");
        let cfg = FloorsConfig::load(&floors_toml)
            .unwrap_or_else(|e| panic!("load {}: {}", floors_toml.display(), e));
        let m = current_measurements(&root);

        let mut failures: Vec<String> = Vec::new();
        for (dim, &floor) in &cfg.floors {
            let current = m.get(dim).copied().unwrap_or(0);
            if current < floor {
                failures.push(format!(
                    "  {}: current = {}, floor = {}",
                    dim, current, floor
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "cert/floors.toml is not satisfied by the current tree:\n{}\n\
             either restore the measurement or edit cert/floors.toml with a \
             `Lower-Floor:` line in the PR body.",
            failures.join("\n")
        );
    }

    /// Regression: the walker must NOT count occurrences that sit
    /// inside a plain string literal. `floors.rs` itself has
    /// `["panic!(", "unimplemented!(", "todo!("]` as a literal
    /// Rust source; a naive substring scan would count all three
    /// and fire a false positive on this very module.
    #[test]
    fn count_library_panics_ignores_occurrences_inside_string_literals() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("crates").join("fake").join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("lib.rs"),
            concat!(
                "pub fn f() {\n",
                "    let patterns = [\"panic!(\", \"unimplemented!(\", \"todo!(\"];\n",
                "    let _ = patterns;\n",
                "}\n",
            ),
        )
        .unwrap();
        assert_eq!(count_library_panics(tmp.path()), 0);
    }

    /// Regression: raw-string literals (`r"…"` and `r#"…"#`) also
    /// count as strings. A `panic!(` inside a raw-string must not
    /// trip the gate.
    #[test]
    fn count_library_panics_ignores_occurrences_inside_raw_strings() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("crates").join("fake").join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("lib.rs"),
            "pub fn f() { let _s = r#\"panic!(\\\"inside raw\\\")\"#; }\n",
        )
        .unwrap();
        assert_eq!(count_library_panics(tmp.path()), 0);
    }

    /// Complementary: a REAL bare panic outside any string MUST be
    /// counted. Pins the floor-to-catch-real-regressions contract.
    #[test]
    fn count_library_panics_catches_bare_panic_outside_strings() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("crates").join("fake").join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("lib.rs"),
            "pub fn f() { panic!(\"real\"); }\npub fn g() { todo!(); }\n",
        )
        .unwrap();
        assert_eq!(count_library_panics(tmp.path()), 2);
    }

    /// Downstream users without a `crates/` directory (or without
    /// `tool/trace/`) shouldn't see the measurements blow up —
    /// helpers gracefully degrade to 0 so an external project can
    /// opt into specific floors without setting up the full
    /// workspace layout we use.
    #[test]
    fn measurements_on_empty_workspace_report_zero_gracefully() {
        let tmp = tempfile::TempDir::new().unwrap();
        let m = current_measurements(tmp.path());
        assert_eq!(m["trace_sys"], 0);
        assert_eq!(m["trace_hlr"], 0);
        assert_eq!(m["trace_llr"], 0);
        assert_eq!(m["trace_test"], 0);
        assert_eq!(m["test_count"], 0);
        assert_eq!(m["library_panics"], 0);
        // RULES / TERMINAL_CODES are compile-time constants.
        assert_eq!(m["diagnostic_codes"], count_rules());
        assert_eq!(m["terminal_codes"], count_terminals());
    }

    #[test]
    fn load_or_missing_distinguishes_not_found_from_parse_error() {
        let tmp = tempfile::TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist.toml");
        assert!(matches!(
            FloorsConfig::load_or_missing(&missing),
            LoadOutcome::Missing
        ));

        let bad = tmp.path().join("malformed.toml");
        std::fs::write(&bad, "this is = not valid {{{").unwrap();
        assert!(matches!(
            FloorsConfig::load_or_missing(&bad),
            LoadOutcome::Error(_)
        ));

        let ok = tmp.path().join("ok.toml");
        std::fs::write(&ok, "schema_version = 1\n[floors]\ndiagnostic_codes = 1\n").unwrap();
        assert!(matches!(
            FloorsConfig::load_or_missing(&ok),
            LoadOutcome::Loaded(_)
        ));
    }

    #[test]
    fn load_rejects_unknown_future_schema_version() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("future.toml");
        std::fs::write(&path, "schema_version = 99\n[floors]\n").unwrap();
        let err = FloorsConfig::load(&path).expect_err("future version must reject");
        assert!(err.contains("schema_version"));
    }

    #[test]
    fn count_tests_finds_at_least_one() {
        let root = workspace_root();
        let n = count_tests(&root);
        assert!(n > 0, "walker found no #[test] — parser regression?");
    }

    #[test]
    fn floors_config_deserializes_empty_with_defaults() {
        let cfg: FloorsConfig = toml::from_str("schema_version = 1\n").expect("parses");
        assert_eq!(cfg.schema_version, 1);
        assert!(cfg.floors.is_empty());
        assert!(cfg.delta_ceilings.is_empty());
    }

    #[test]
    fn floors_config_deserializes_full_shape() {
        let toml = r#"
schema_version = 1

[floors]
diagnostic_codes = 80

[delta_ceilings]
new_dead_code_allows = 0
"#;
        let cfg: FloorsConfig = toml::from_str(toml).expect("parses");
        assert_eq!(cfg.schema_version, 1);
        assert_eq!(cfg.floors.get("diagnostic_codes"), Some(&80u64));
        assert_eq!(cfg.delta_ceilings.get("new_dead_code_allows"), Some(&0u64));
    }
}
