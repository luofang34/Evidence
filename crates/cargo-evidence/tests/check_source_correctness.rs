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

/// Seed a Rust library with a deliberate build error (undefined
/// macro) so `cargo test` exits non-zero AND produces no parseable
/// `test result:` line.
fn seed_cargo_workspace_with_build_error(dir: &Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        r#"[package]
name = "build-error-fixture"
version = "0.1.0"
edition = "2024"

[lib]
path = "src/lib.rs"
"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/lib.rs"),
        "pub fn x() { unknown_macro!(\"deliberate build error\"); }\n",
    )
    .unwrap();
}

/// Seed a Rust library with a passing test + a failing test. Build
/// succeeds, tests run, one fails → libtest writes a parseable
/// `test result:` line AND cargo exits non-zero (101 on test
/// failure). The disambiguation rule must keep this on the normal
/// REQ_GAP path, NOT route it to CHECK_TEST_RUNTIME_FAILURE.
fn seed_cargo_workspace_with_failing_test(dir: &Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        r#"[package]
name = "failing-test-fixture"
version = "0.1.0"
edition = "2024"

[lib]
path = "src/lib.rs"
"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/lib.rs"),
        r#"#[cfg(test)]
mod tests {
    #[test]
    fn passes() {
        assert_eq!(1 + 1, 2);
    }

    #[test]
    fn fails() {
        assert_eq!(1, 2, "deliberate failure");
    }
}
"#,
    )
    .unwrap();
}

/// **Compile-failure mislabel regression.** A genuine build
/// failure (undefined macro → cargo exits 101, no `test result:`
/// line) used to route to `CLI_INVALID_ARGUMENT` — wrong
/// category. The disambiguation rule is:
///
/// - `(Some(parsed), _)` → normal per-requirement path
/// - `(None, exit == 0)` → `CLI_INVALID_ARGUMENT` (weird shape)
/// - `(None, exit != 0)` → `CHECK_TEST_RUNTIME_FAILURE` (this test)
///
/// The message carries cargo's exit code + a 2KB tail of stderr
/// so an agent sees the underlying compiler diagnostic without
/// re-spawning cargo.
#[test]
fn build_failure_emits_check_test_runtime_failure_not_cli_invalid_argument() {
    let tmp = TempDir::new().expect("tempdir");
    seed_cargo_workspace_with_build_error(tmp.path());

    let out = cargo_evidence()
        .args(["evidence", "--format=jsonl", "check", "--mode=source"])
        .arg(tmp.path())
        .output()
        .expect("spawn");

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();

    assert!(
        stdout.contains("CHECK_TEST_RUNTIME_FAILURE"),
        "expected CHECK_TEST_RUNTIME_FAILURE in JSONL stream on build failure; \
         stdout={stdout}",
    );
    assert!(
        !stdout.contains("CLI_INVALID_ARGUMENT"),
        "build failure must NOT route to CLI_INVALID_ARGUMENT; stdout={stdout}",
    );

    // Terminal should be VERIFY_FAIL + exit 2.
    let last_line = stdout
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .expect("non-empty stdout");
    let terminal: Value = serde_json::from_str(last_line).expect("terminal is JSON");
    assert_eq!(
        terminal["code"].as_str(),
        Some("VERIFY_FAIL"),
        "terminal must be VERIFY_FAIL; got {terminal}"
    );
    assert_eq!(
        out.status.code(),
        Some(2),
        "build failure must exit 2 (verification failure); stdout={stdout}"
    );

    // Message must carry the cargo diagnostic — assert on a
    // substring of the tail so agents get actionable context.
    let runtime_line = stdout
        .lines()
        .find(|l| l.contains("CHECK_TEST_RUNTIME_FAILURE"))
        .expect("runtime-failure line present");
    let runtime_diag: Value = serde_json::from_str(runtime_line).expect("JSON");
    let msg = runtime_diag["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("cannot find macro") || msg.contains("unknown_macro"),
        "runtime-failure message must preserve cargo's stderr tail; got: {msg}",
    );
}

/// **Verify exit-code symmetry regression.** The non-JSONL
/// `cmd_verify` path used to return `EXIT_VERIFICATION_FAILURE`
/// (2) for a missing bundle while the JSONL path returned
/// `EXIT_ERROR` (1) for the same condition. Scripts switching
/// `--format` got different signals for identical state. The
/// harmonized rule: `EXIT_ERROR` universally for I/O / runtime
/// fault (bundle not found, file unreadable); `EXIT_VERIFICATION_
/// FAILURE` reserved for verify-ran-and-found-problems-in-an-
/// existing-bundle.
#[test]
fn verify_missing_bundle_exit_code_consistent_across_formats() {
    let tmp = TempDir::new().expect("tempdir");
    let missing = tmp.path().join("nonexistent-bundle");

    let human_out = cargo_evidence()
        .args(["evidence", "verify"])
        .arg(&missing)
        .output()
        .expect("spawn human");

    let jsonl_out = cargo_evidence()
        .args(["evidence", "--format=jsonl", "verify"])
        .arg(&missing)
        .output()
        .expect("spawn jsonl");

    assert_eq!(
        human_out.status.code(),
        jsonl_out.status.code(),
        "missing-bundle exit code diverges across formats: \
         human={:?} jsonl={:?}",
        human_out.status.code(),
        jsonl_out.status.code(),
    );

    // Both must be EXIT_ERROR (1), not EXIT_VERIFICATION_FAILURE
    // (2). The missing-bundle case is an I/O fault.
    assert_eq!(
        human_out.status.code(),
        Some(1),
        "missing-bundle must exit 1 (I/O fault), not 2 \
         (verification failure). Got: {:?}",
        human_out.status.code(),
    );
}

/// **Failing-test guardrail.** Tests that fail normally (build
/// succeeded, `test result: FAILED. N passed; M failed`) must
/// keep the existing REQ_PASS/REQ_GAP per-test path — NOT route
/// to `CHECK_TEST_RUNTIME_FAILURE`. Disambiguation happens on
/// parse-success, not exit code.
///
/// This is the regression pin for the "don't over-aggressively
/// map exit != 0 to runtime failure" nuance. Cargo exits 101 on
/// test failure exactly like it exits 101 on build failure; only
/// the parser's ability to extract `test result:` lines
/// distinguishes them.
#[test]
fn test_failure_keeps_normal_req_gap_path_not_runtime_failure() {
    let tmp = TempDir::new().expect("tempdir");
    seed_cargo_workspace_with_failing_test(tmp.path());
    // Failing-test fixture has no tool/trace, so the trace-phase
    // emits empty requirements. That's fine — this test pins the
    // DISAMBIGUATION behaviour, which is at phase 2 (parse).

    let out = cargo_evidence()
        .args(["evidence", "--format=jsonl", "check", "--mode=source"])
        .arg(tmp.path())
        .output()
        .expect("spawn");

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();

    assert!(
        !stdout.contains("CHECK_TEST_RUNTIME_FAILURE"),
        "test failure must NOT route to CHECK_TEST_RUNTIME_FAILURE — parsing \
         succeeded, so the normal per-test path must apply. stdout={stdout}",
    );

    // Sanity: all lines still parse as JSON.
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let _v: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("non-JSON line in --format=jsonl stream: {line:?}: {e}"));
    }
}

/// **Boundary-trace-roots rebase regression.** The convention
/// paths (`tool/trace`, `cert/trace`) rebase against the `<PATH>`
/// argument, but the boundary-fallback path in
/// `default_trace_roots` used to return entries verbatim. A
/// downstream project with `trace_roots = ["custom/trace"]` in its
/// `cert/boundary.toml` would silently resolve against the caller's
/// CWD and emit VERIFY_OK with 0 requirements.
///
/// Setup: downstream tempdir has NO `tool/trace/` (so the
/// convention auto-discovery misses), has `cert/boundary.toml`
/// configuring `trace_roots = ["custom/trace"]`, and has the real
/// trace files under `custom/trace/`. Invoke from a caller CWD
/// with a DIFFERENT trace tree (the Evidence repo). The JSONL
/// stream must mention the downstream's IDs.
#[test]
fn check_source_rebases_boundary_trace_roots() {
    let downstream = TempDir::new().expect("tempdir");
    seed_minimal_cargo_workspace(downstream.path());
    // Note: NO tool/trace here — forces the fallback through
    // `load_trace_roots(cert/boundary.toml)`.
    fs::create_dir_all(downstream.path().join("cert")).unwrap();
    fs::write(
        downstream.path().join("cert/boundary.toml"),
        r#"[scope]
in_scope = ["downstream-fixture"]
trace_roots = ["custom/trace"]
"#,
    )
    .unwrap();
    seed_minimal_trace(&downstream.path().join("custom/trace"));

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

    // Positive: the DOWNSTREAM-* IDs must appear (the boundary-
    // configured `custom/trace` got rebased against `<PATH>` and
    // the real trace files were loaded).
    assert!(
        stdout.contains("DOWNSTREAM-HLR-1")
            || stdout.contains("DOWNSTREAM-LLR-1")
            || stdout.contains("DOWNSTREAM-TEST-1"),
        "expected DOWNSTREAM-* IDs from `custom/trace` rebased against <PATH>; \
         got stdout={stdout}",
    );
    // Negative: the caller's TEST-001 canary must NOT appear.
    assert!(
        !stdout.contains("TEST-001"),
        "caller CWD's TEST-001 leaked into check output — boundary \
         trace_roots were not rebased. stdout={stdout}",
    );
}
