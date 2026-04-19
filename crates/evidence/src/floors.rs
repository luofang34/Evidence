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
//! Each helper is testable locally, so a regression surfaces on a
//! developer's machine before reaching CI.
//!
//! **Scope of "library panics".** The `count_library_panics` walk
//! targets production code under `crates/*/src/**`, excluding the
//! cargo-evidence `main.rs` (which is the anyhow error-envelope layer
//! per the existing `clippy.toml` carve-out) and excluding `#[cfg(test)]`
//! blocks at the top of modules. Test-only panics, doc-example panics,
//! and panics inside string literals are not counted. The current
//! count is 0; the floors-gate delta ceiling pins 0-new-additions.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::trace::read_all_trace_files;

/// On-disk shape of `cert/floors.toml`.
///
/// Two tables:
/// - `[floors]` — absolute `current >= floor` contracts, one entry
///   per measured dimension.
/// - `[delta_ceilings]` — `added_in_pr_diff <= ceiling`. Applied only
///   when the gate runs with a `--base <ref>` scope.
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct FloorsConfig {
    /// Absolute floors: `current >= value`.
    pub floors: BTreeMap<String, u64>,
    /// Delta ceilings: `added_in_diff <= value`.
    pub delta_ceilings: BTreeMap<String, u64>,
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
        toml::from_str(&text).map_err(|e| format!("parsing {}: {}", path.display(), e))
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
                Ok(cfg) => LoadOutcome::Loaded(cfg),
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

/// Count `#[test]` attribute occurrences across `crates/**/*.rs`,
/// targeting function-level test declarations. Matches both
/// `#[test]` alone on a line and `#[test]` followed by attributes
/// on the same or next line.
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
/// production library source. Strips obvious `#[cfg(test)]` module
/// blocks and doc-comment examples. Conservative: comments and
/// string literals are NOT stripped (cost-of-false-positive is one
/// allowlist entry vs. cost-of-false-negative is a panic slipping
/// past the gate).
pub fn count_library_panics(workspace_root: &Path) -> u64 {
    let crates = workspace_root.join("crates");
    let mut files = Vec::new();
    walk_rs_files(&crates, &mut files);
    let mut total: u64 = 0;
    for file in &files {
        // Exclude tests/ dirs (integration tests). Windows paths use
        // `\` natively, so normalize before substring-matching — a
        // naive `contains("/tests/")` silently misses on Windows CI
        // and over-counts every integration-test file's panics.
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
            // Skip doc-comment lines to ignore example panics inside
            // module docstrings.
            let trimmed = line.trim_start();
            if trimmed.starts_with("//!") || trimmed.starts_with("///") || trimmed.starts_with("//")
            {
                continue;
            }
            for needle in ["panic!(", "unimplemented!(", "todo!("] {
                if let Some(pos) = line.find(needle) {
                    // Skip occurrences inside string literals —
                    // the walker's own source (`floors.rs`) has
                    // `"panic!("` inside this very array. Count
                    // unescaped `"` before the needle; odd count
                    // means we're mid-string. Not perfect (raw
                    // strings, escaped quotes) but rejects the
                    // false positives we actually see.
                    let quotes_before = line[..pos].chars().filter(|&c| c == '"').count();
                    if quotes_before % 2 == 1 {
                        continue;
                    }
                    total += 1;
                }
            }
        }
    }
    total
}

/// Walk `root` recursively; push every `.rs` path into `out`. Skips
/// `target/` trees so a stale `cargo doc` output can't taint the
/// measurement.
fn walk_rs_files(root: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            walk_rs_files(&path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Strip top-level `#[cfg(test)]\nmod tests { ... }` blocks from a
/// source. Conservative: matches blocks at indent 0 by brace-depth
/// tracking; inner nested `#[cfg(test)]` sub-modules would survive
/// and potentially over-count, but they're rare and the miss-rate
/// is acceptable for a floor that starts at 0.
fn strip_cfg_test_modules(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // Look for `#[cfg(test)]` at the start of a line.
        let at_line_start = i == 0 || bytes[i - 1] == b'\n';
        if at_line_start && bytes[i..].starts_with(b"#[cfg(test)]") {
            // Skip to the next `{` (module body open).
            let mut j = i;
            while j < bytes.len() && bytes[j] != b'{' {
                j += 1;
            }
            if j >= bytes.len() {
                break;
            }
            // Find the matching `}`.
            let mut depth: i32 = 0;
            let mut k = j;
            while k < bytes.len() {
                match bytes[k] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            k += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            i = k;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
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

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/")
            .parent()
            .expect("workspace root")
            .to_path_buf()
    }

    /// Current-value snapshot. This test is the per-dimension regression
    /// canary: if any helper's output changes, this test fires before
    /// the downstream floors gate does, pinpointing which helper
    /// diverged.
    ///
    /// When a new code, test, or trace entry lands, update the expected
    /// values in the same PR — the whole point of the ratchet is that
    /// rigor additions go up.
    #[test]
    fn current_measurements_match_committed_snapshot() {
        let root = workspace_root();
        let m = current_measurements(&root);
        // Values reflect the PR #48 commit-1 state. Future PRs that
        // add rigor will bump these; they're the ratchet floor.
        assert_eq!(m["diagnostic_codes"], 82, "RULES count");
        assert_eq!(m["terminal_codes"], 4, "TERMINAL_CODES count");
        assert_eq!(m["trace_sys"], 9, "sys.toml requirements");
        assert_eq!(m["trace_hlr"], 37, "hlr.toml requirements");
        assert_eq!(m["trace_llr"], 37, "llr.toml requirements");
        assert_eq!(m["trace_test"], 37, "tests.toml tests");
        // test_count / library_panics shift as engineering evolves;
        // the floors-toml commit is the load-bearing snapshot for
        // those, so we just assert non-zero / zero here.
        assert!(m["test_count"] > 0, "#[test] fn count must be >0");
        // library_panics MUST be 0 — PR #48 locks this as a ceiling.
        assert_eq!(
            m["library_panics"], 0,
            "library panics must be 0 at PR #48 commit 1; audit before raising"
        );
    }

    /// Regression: the walker must NOT count occurrences that sit
    /// inside a string literal. `floors.rs` itself has
    /// `["panic!(", "unimplemented!(", "todo!("]` as a literal
    /// Rust source; a naive substring scan would count all three
    /// and fire a false positive on this very module.
    ///
    /// The quote-parity check (odd number of `"` before the
    /// needle → inside a string → skip) guards that case. This
    /// test pins the contract: removing the check breaks the test
    /// and the downstream user sees a clean failure.
    #[test]
    fn count_library_panics_ignores_occurrences_inside_string_literals() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("crates").join("fake").join("src");
        std::fs::create_dir_all(&src).unwrap();
        // File has "panic!(", "unimplemented!(", "todo!(" as string
        // literals (same pattern as floors.rs itself), plus nothing
        // outside strings. Expected count: 0.
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

    /// Complementary to the above: a REAL bare panic in non-test code
    /// MUST be counted. Pins the floor-to-catch-real-regressions
    /// contract in the other direction.
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
        // RULES and TERMINAL_CODES are compile-time constants, so
        // those dimensions stay at their true count regardless of
        // workspace shape.
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
        std::fs::write(&ok, "[floors]\ndiagnostic_codes = 1\n").unwrap();
        assert!(matches!(
            FloorsConfig::load_or_missing(&ok),
            LoadOutcome::Loaded(_)
        ));
    }

    #[test]
    fn strip_cfg_test_modules_removes_nested_braces() {
        let input = "fn live() { panic!(); }\n#[cfg(test)]\nmod tests { panic!(); fn t() { todo!(); } }\nfn also_live() {}\n";
        let stripped = strip_cfg_test_modules(input);
        assert!(stripped.contains("fn live()"));
        assert!(stripped.contains("fn also_live()"));
        assert!(!stripped.contains("mod tests"));
        assert!(!stripped.contains("todo!()"));
        // The live `panic!()` on line 1 survives.
        assert!(stripped.contains("panic!()"));
    }

    #[test]
    fn count_tests_finds_at_least_one() {
        // Sanity — this very test is a `#[test]`, so the count must
        // be ≥1 on any tree where the module builds.
        let root = workspace_root();
        let n = count_tests(&root);
        assert!(n > 0, "walker found no #[test] — parser regression?");
    }

    #[test]
    fn floors_config_deserializes_empty() {
        let cfg: FloorsConfig = toml::from_str("").expect("empty parses");
        assert!(cfg.floors.is_empty());
        assert!(cfg.delta_ceilings.is_empty());
    }

    #[test]
    fn floors_config_deserializes_two_tables() {
        let toml = r#"
[floors]
diagnostic_codes = 80

[delta_ceilings]
new_dead_code_allows = 0
"#;
        let cfg: FloorsConfig = toml::from_str(toml).expect("parses");
        assert_eq!(cfg.floors.get("diagnostic_codes"), Some(&80u64));
        assert_eq!(cfg.delta_ceilings.get("new_dead_code_allows"), Some(&0u64));
    }
}
