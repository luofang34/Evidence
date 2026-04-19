//! Unit tests for `evidence::floors`. Lives in a sibling file (pulled
//! in via `#[path]` from the parent) so the facade stays under the
//! 500-line workspace limit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

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
/// `cert/floors.toml` must be satisfied by the current tree across
/// BOTH workspace-wide and per-crate dimensions. No hardcoded
/// per-dimension expected values — the test reads the TOML and
/// asserts `current >= floor` for each entry. If a measurement
/// helper drifts, this test fires with the dimension (and crate,
/// for per-crate) named in the panic message.
#[test]
fn current_measurements_satisfy_committed_floors() {
    let root = workspace_root();
    let floors_toml = root.join("cert").join("floors.toml");
    let cfg = FloorsConfig::load(&floors_toml)
        .unwrap_or_else(|e| panic!("load {}: {}", floors_toml.display(), e));
    let workspace_m = current_measurements(&root);
    let per_crate_m = per_crate_measurements(&root);

    let mut failures: Vec<String> = Vec::new();

    // Workspace floors.
    for (dim, &floor) in &cfg.floors {
        let current = workspace_m.get(dim).copied().unwrap_or(0);
        if current < floor {
            failures.push(format!(
                "  [floors] {}: current = {}, floor = {}",
                dim, current, floor
            ));
        }
    }

    // Per-crate floors.
    for (crate_name, per) in &cfg.per_crate {
        let Some(current_per) = per_crate_m.get(crate_name) else {
            failures.push(format!(
                "  [per_crate.{}]: crate has no measurement (directory \
                 missing under crates/)",
                crate_name
            ));
            continue;
        };
        for (dim, &floor) in per {
            let current = current_per.get(dim).copied().unwrap_or(0);
            if current < floor {
                failures.push(format!(
                    "  [per_crate.{}] {}: current = {}, floor = {}",
                    crate_name, dim, current, floor
                ));
            }
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
/// `["panic!(", "unimplemented!(", "todo!("]` as a literal Rust
/// source; a naive substring scan would count all three and fire a
/// false positive on this very module.
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
/// `tool/trace/`) shouldn't see the measurements blow up — helpers
/// gracefully degrade to 0 (workspace-wide) or an empty map (per-
/// crate) so an external project can opt into specific floors
/// without setting up the full workspace layout we use.
#[test]
fn measurements_on_empty_workspace_report_zero_gracefully() {
    let tmp = tempfile::TempDir::new().unwrap();
    let m = current_measurements(tmp.path());
    assert_eq!(m["trace_sys"], 0);
    assert_eq!(m["trace_hlr"], 0);
    assert_eq!(m["trace_llr"], 0);
    assert_eq!(m["trace_test"], 0);
    // RULES / TERMINAL_CODES are compile-time constants.
    assert_eq!(m["diagnostic_codes"], count_rules());
    assert_eq!(m["terminal_codes"], count_terminals());
    // No crates/ dir → no per-crate rows at all.
    assert!(per_crate_measurements(tmp.path()).is_empty());
}

/// Per-crate aggregator produces one map per `crates/<name>/`
/// directory, each containing `test_count` + `library_panics`.
#[test]
fn per_crate_measurements_splits_by_crate() {
    let tmp = tempfile::TempDir::new().unwrap();
    let foo_src = tmp.path().join("crates").join("foo").join("src");
    let bar_src = tmp.path().join("crates").join("bar").join("src");
    std::fs::create_dir_all(&foo_src).unwrap();
    std::fs::create_dir_all(&bar_src).unwrap();
    std::fs::write(
        foo_src.join("lib.rs"),
        "#[test]\nfn t() {}\npub fn f() { panic!(); }\n",
    )
    .unwrap();
    std::fs::write(
        bar_src.join("lib.rs"),
        "#[test]\nfn a() {}\n#[test]\nfn b() {}\n",
    )
    .unwrap();

    let per = per_crate_measurements(tmp.path());
    assert_eq!(per.len(), 2);
    assert_eq!(per["foo"]["test_count"], 1);
    assert_eq!(per["foo"]["library_panics"], 1);
    assert_eq!(per["bar"]["test_count"], 2);
    assert_eq!(per["bar"]["library_panics"], 0);
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
    assert!(cfg.per_crate.is_empty());
    assert!(cfg.delta_ceilings.is_empty());
}

#[test]
fn floors_config_deserializes_full_shape() {
    let toml = r#"
schema_version = 1

[floors]
diagnostic_codes = 80

[per_crate.my-crate]
test_count = 10

[delta_ceilings]
new_dead_code_allows = 0
"#;
    let cfg: FloorsConfig = toml::from_str(toml).expect("parses");
    assert_eq!(cfg.schema_version, 1);
    assert_eq!(cfg.floors.get("diagnostic_codes"), Some(&80u64));
    assert_eq!(
        cfg.per_crate
            .get("my-crate")
            .and_then(|m| m.get("test_count")),
        Some(&10u64)
    );
    assert_eq!(cfg.delta_ceilings.get("new_dead_code_allows"), Some(&0u64));
}
