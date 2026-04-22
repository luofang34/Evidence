//! Correctness regression tests for `cargo evidence check --mode=source`
//! and sibling `verify` format-symmetry invariants.
//!
//! Three orthogonal bugs pinned here, one test each:
//!
//! 1. **Workspace-not-CWD.** `check --mode=source <PATH>` must
//!    validate `<PATH>`'s trace / boundary, not the caller CWD's.
//! 2. **Compile-failure mislabel.** A genuine build failure is a
//!    `CHECK_TEST_RUNTIME_FAILURE`, not a `CLI_INVALID_ARGUMENT`.
//! 3. **Verify exit-code symmetry.** Missing-bundle exit code is
//!    identical across `--format={human,jsonl}`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use evidence_core::schema_versions::TRACE;
use serde_json::Value;
use tempfile::TempDir;

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

/// Seed `<dir>` with a minimal 1-HLR / 1-LLR / 1-TEST chain that
/// links up cleanly. Mirrors `trace_discovery.rs`'s helper but
/// inlined here to keep the test file standalone.
fn seed_minimal_trace(trace_dir: &Path) {
    fs::create_dir_all(trace_dir).unwrap();
    let hlr_uid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let llr_uid = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    let test_uid = "cccccccc-dddd-eeee-ffff-000000000000";

    fs::write(
        trace_dir.join("hlr.toml"),
        format!(
            r#"[schema]
version = "{ver}"

[meta]
document_id = "HLR"
revision = "1.0"

[[requirements]]
id = "DOWNSTREAM-HLR-1"
title = "Downstream fixture HLR"
owner = "soi"
uid = "{hlr_uid}"
"#,
            ver = TRACE
        ),
    )
    .unwrap();

    fs::write(
        trace_dir.join("llr.toml"),
        format!(
            r#"[schema]
version = "{ver}"

[meta]
document_id = "LLR"
revision = "1.0"

[[requirements]]
id = "DOWNSTREAM-LLR-1"
title = "Downstream fixture LLR"
owner = "soi"
uid = "{llr_uid}"
traces_to = ["{hlr_uid}"]
verification_methods = ["test"]
"#,
            ver = TRACE
        ),
    )
    .unwrap();

    fs::write(
        trace_dir.join("tests.toml"),
        format!(
            r#"[schema]
version = "{ver}"

[meta]
document_id = "TESTS"
revision = "1.0"

[[tests]]
id = "DOWNSTREAM-TEST-1"
title = "Downstream fixture TEST"
owner = "soi"
uid = "{test_uid}"
traces_to = ["{llr_uid}"]
"#,
            ver = TRACE
        ),
    )
    .unwrap();
}

/// Seed a minimal Rust workspace (Cargo.toml + src/lib.rs) at
/// `dir`. Empty library with no tests satisfies libtest:
/// "test result: ok. 0 passed" — enough for `cmd_check_source`
/// to reach the trace-validation phase.
fn seed_minimal_cargo_workspace(dir: &Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        r#"[package]
name = "downstream-fixture"
version = "0.1.0"
edition = "2024"

[lib]
path = "src/lib.rs"
"#,
    )
    .unwrap();
    fs::write(dir.join("src/lib.rs"), "// empty library\n").unwrap();
}

/// **Workspace-not-CWD regression.** `cmd_check_source` must
/// resolve `default_trace_roots` and `BoundaryConfig::load_or_default`
/// against the `<PATH>` argument, not the process CWD. An
/// auditor running `cargo evidence check --mode=source
/// /downstream` from a parent directory would otherwise silently
/// get the caller's own `tool/trace/` and DAL policy — a
/// confidently-wrong cert verdict.
///
/// Setup: tempdir with its own `tool/trace/` containing a
/// `DOWNSTREAM-*`-prefixed chain. Invoke `check` from a CWD that
/// has a DIFFERENT trace (the Evidence repo — its trace uses
/// `HLR-001` / `TEST-001` IDs). The JSONL stream must mention
/// the tempdir's `DOWNSTREAM-*` IDs and MUST NOT mention the
/// caller's `TEST-001` canary.
#[test]
fn check_source_uses_argument_workspace_not_cwd() {
    // Build the downstream fixture: a minimal Rust workspace with
    // its own tool/trace/ tree.
    let downstream = TempDir::new().expect("tempdir");
    seed_minimal_cargo_workspace(downstream.path());
    seed_minimal_trace(&downstream.path().join("tool/trace"));

    // Caller CWD: the Evidence repo itself. Its tool/trace
    // contains TEST-001, HLR-001, etc. If the pre-#72 bug
    // returned, the JSONL would stream those.
    let caller_cwd = std::env::var("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .expect("CARGO_MANIFEST_DIR")
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf();

    let out = cargo_evidence()
        .args(["evidence", "--format=jsonl", "check", "--mode=source"])
        .arg(downstream.path())
        .current_dir(&caller_cwd)
        .output()
        .expect("spawn");

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();

    // Positive: the downstream fixture's IDs appear. The minimal
    // fixture has one TEST with no matching #[test] fn in the
    // library, so `check` will emit a REQ_SKIP / REQ_GAP for
    // DOWNSTREAM-TEST-1 and cascade — FINE for this test. We
    // assert on the ID presence, not pass/fail status.
    assert!(
        stdout.contains("DOWNSTREAM-HLR-1")
            || stdout.contains("DOWNSTREAM-LLR-1")
            || stdout.contains("DOWNSTREAM-TEST-1"),
        "expected stdout to reference DOWNSTREAM-* IDs from the argument \
         workspace's trace; got stdout={stdout}",
    );

    // Negative regression pin: the caller CWD's TEST-001 canary
    // (a stable ID in this repo's tool/trace) MUST NOT appear.
    // If trace loading silently fell back to CWD, that REQ_PASS
    // line would stream — its absence proves the workspace-not-
    // CWD bug is fixed.
    assert!(
        !stdout.contains("TEST-001"),
        "caller CWD's TEST-001 leaked into check output — the \
         workspace-not-CWD regression has returned. stdout={stdout}",
    );

    // Sanity: the JSONL stream is well-formed (every non-empty
    // line parses).
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let _v: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("non-JSON line in --format=jsonl stream: {line:?}: {e}"));
    }
}
