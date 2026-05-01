//! Integration tests for the cargo-evidence CLI.
//!
//! These tests exercise the binary end-to-end using `assert_cmd`.
//! Each test runs in an isolated temp directory.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Helper: build a cargo-evidence command.
fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

// ============================================================================
// Init
// ============================================================================

#[test]
fn test_init_creates_cert_structure() {
    let tmp = TempDir::new().unwrap();
    cargo_evidence()
        .arg("evidence")
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("cert/boundary.toml"));

    assert!(tmp.path().join("cert/boundary.toml").exists());
    assert!(tmp.path().join("cert/trace").is_dir());
    assert!(tmp.path().join("cert/profiles").is_dir());
}

#[test]
fn test_init_refuses_without_force() {
    let tmp = TempDir::new().unwrap();
    // First init succeeds
    cargo_evidence()
        .arg("evidence")
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success();

    // Second init without --force fails
    cargo_evidence()
        .arg("evidence")
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("--force"));
}

#[test]
fn test_init_force_overwrites() {
    let tmp = TempDir::new().unwrap();
    cargo_evidence()
        .arg("evidence")
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success();

    cargo_evidence()
        .arg("evidence")
        .arg("init")
        .arg("--force")
        .current_dir(tmp.path())
        .assert()
        .success();
}

// ============================================================================
// Trace
// ============================================================================

#[test]
fn test_trace_no_action_errors() {
    let tmp = TempDir::new().unwrap();
    cargo_evidence()
        .arg("evidence")
        .arg("trace")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("specify an action"));
}

#[test]
fn test_trace_validate_missing_root_warns() {
    let tmp = TempDir::new().unwrap();
    cargo_evidence()
        .arg("evidence")
        .arg("trace")
        .arg("--validate")
        .arg("--trace-roots")
        .arg("nonexistent/trace")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("does not exist"));
}

#[test]
fn test_trace_validate_valid_traces() {
    use evidence_core::schema_versions::TRACE;

    let tmp = TempDir::new().unwrap();
    let trace_dir = tmp.path().join("trace");
    fs::create_dir_all(&trace_dir).unwrap();

    // Minimal valid HLR
    fs::write(
        trace_dir.join("hlr.toml"),
        format!(
            r#"
[meta]
document_id = "HLR-001"
revision = "1.0"

[schema]
version = "{ver}"

[[requirements]]
id = "HLR-1"
title = "Test requirement"
owner = "soi"
uid = "11111111-1111-1111-1111-111111111111"
verification_methods = ["review"]
"#,
            ver = TRACE
        ),
    )
    .unwrap();

    // Minimal valid LLR pointing to HLR
    fs::write(
        trace_dir.join("llr.toml"),
        format!(
            r#"
[meta]
document_id = "LLR-001"
revision = "1.0"

[schema]
version = "{ver}"

[[requirements]]
id = "LLR-1"
title = "Implementation detail"
owner = "soi"
uid = "22222222-2222-2222-2222-222222222222"
derived = false
traces_to = ["11111111-1111-1111-1111-111111111111"]
verification_methods = ["unit_test"]
"#,
            ver = TRACE
        ),
    )
    .unwrap();

    // Minimal valid Tests pointing to LLR
    fs::write(
        trace_dir.join("tests.toml"),
        format!(
            r#"
[meta]
document_id = "TESTS-001"
revision = "1.0"

[schema]
version = "{ver}"

[[tests]]
id = "TEST-1"
title = "Verify LLR-1"
owner = "soi"
uid = "33333333-3333-3333-3333-333333333333"
traces_to = ["22222222-2222-2222-2222-222222222222"]
verification_method = "unit_test"
"#,
            ver = TRACE
        ),
    )
    .unwrap();

    cargo_evidence()
        .arg("evidence")
        .arg("trace")
        .arg("--validate")
        .arg("--trace-roots")
        .arg(trace_dir.to_str().unwrap())
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("validation passed"));
}

// ============================================================================
// Verify
// ============================================================================

#[test]
fn test_verify_nonexistent_bundle_errors() {
    let tmp = TempDir::new().unwrap();
    cargo_evidence()
        .arg("evidence")
        .arg("verify")
        .arg("nonexistent-bundle")
        .current_dir(tmp.path())
        .assert()
        .failure();
}

// ============================================================================
// Schema
// ============================================================================

#[test]
fn test_schema_show_index() {
    cargo_evidence()
        .arg("evidence")
        .arg("schema")
        .arg("show")
        .arg("index")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"type\""));
}

#[test]
fn test_schema_show_env() {
    cargo_evidence()
        .arg("evidence")
        .arg("schema")
        .arg("show")
        .arg("env")
        .assert()
        .success()
        .stdout(predicate::str::contains("rustc"));
}

#[test]
fn test_schema_show_commands() {
    cargo_evidence()
        .arg("evidence")
        .arg("schema")
        .arg("show")
        .arg("commands")
        .assert()
        .success()
        .stdout(predicate::str::contains("argv"));
}

#[test]
fn test_schema_show_hashes() {
    cargo_evidence()
        .arg("evidence")
        .arg("schema")
        .arg("show")
        .arg("hashes")
        .assert()
        .success()
        .stdout(predicate::str::contains("additionalProperties"));
}

// ============================================================================
// Help and Version
// ============================================================================

#[test]
fn test_help_flag() {
    cargo_evidence()
        .arg("evidence")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("evidence"));
}

#[test]
fn test_generate_help() {
    cargo_evidence()
        .arg("evidence")
        .arg("generate")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("sign-key"));
}

// ============================================================================
// Verify: exit codes and JSON output
// ============================================================================

/// Missing-bundle is an I/O fault, not a verification finding.
/// Exit code is `EXIT_ERROR` (1), harmonized across
/// `--format={human,json,jsonl}`. The exit-1 convention is pinned
/// further by
/// `check_source_correctness::verify_missing_bundle_exit_code_consistent_across_formats`
/// which exercises the cross-format symmetry explicitly.
#[test]
fn test_verify_nonexistent_bundle_exits_error() {
    let tmp = TempDir::new().unwrap();
    cargo_evidence()
        .arg("evidence")
        .arg("verify")
        .arg("nonexistent-bundle")
        .current_dir(tmp.path())
        .assert()
        .code(1)
        .stderr(predicate::str::contains("bundle not found"));
}

#[test]
fn test_verify_json_nonexistent() {
    let tmp = TempDir::new().unwrap();
    cargo_evidence()
        .arg("evidence")
        .arg("verify")
        .arg("--json")
        .arg("nonexistent-bundle")
        .current_dir(tmp.path())
        .assert()
        .code(1)
        .stdout(predicate::str::contains("\"success\": false"));
}

// ============================================================================
// Generate: missing --out-dir
// ============================================================================

#[test]
fn test_generate_requires_out_dir() {
    let tmp = TempDir::new().unwrap();
    cargo_evidence()
        .arg("evidence")
        .arg("generate")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("--out-dir"));
}

// ============================================================================
// Generate: policy-not-implemented gate
// ============================================================================
//
// `assert_policy_implementable` was the safety rail that refused
// generate when boundary.toml enabled a rule whose enforcement
// hadn't shipped, so a bundle could not silently overclaim. As of
// this PR every `BoundaryPolicy` flag is enforced, so the
// "does not enforce" refusal path no longer exists at integration
// scope. The unit test
// `unimplemented_enabled_rules_is_empty_for_every_combination`
// in `evidence_core::policy::boundary` pins the empty-list invariant.
// If a future flag lands without enforcement, that unit test fires
// immediately, and a parallel integration test would land alongside
// the new flag.

/// Positive control on the implemented gates: a `boundary.toml`
/// with `forbid_build_rs = true` and `forbid_proc_macros = true`
/// but `in_scope = []` must not stall generate at the boundary
/// preflight — there is no in-scope crate, so neither check has
/// work to do. Pins the per-crate scoping invariant for the
/// implemented rules at integration scope: a project that wants
/// to assert the rules even before scoping crates should not see
/// false positives.
#[test]
fn test_generate_passes_when_implemented_policy_has_no_in_scope_violation() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("cert")).unwrap();
    fs::write(
        tmp.path().join("cert/boundary.toml"),
        format!(
            r#"
[schema]
version = "{ver}"

[scope]
in_scope = []

[policy]
no_out_of_scope_deps = false
forbid_build_rs = true
forbid_proc_macros = true
"#,
            ver = evidence_core::schema_versions::BOUNDARY
        ),
    )
    .unwrap();

    let out = TempDir::new().unwrap();
    let assert = cargo_evidence()
        .arg("evidence")
        .arg("generate")
        .arg("--out-dir")
        .arg(out.path())
        .arg("--profile")
        .arg("dev")
        .current_dir(tmp.path())
        .assert();
    // The run may still fail downstream (no Cargo.toml, no git, etc.),
    // but the failure must not name `forbid_build_rs` /
    // `forbid_proc_macros` as the cause — those checks have no
    // in-scope crate to inspect.
    let output = assert.get_output();
    let stderr = std::str::from_utf8(&output.stderr).unwrap_or("");
    assert!(
        !stderr.contains("forbid_build_rs is enabled and"),
        "boundary preflight must not fire on empty in_scope; stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("forbid_proc_macros is enabled and"),
        "boundary preflight must not fire on empty in_scope; stderr:\n{stderr}"
    );
}

/// Positive control: the `init`-scaffolded template must not trip
/// the policy-not-implemented gate. If it does, `cargo evidence init
/// && cargo evidence generate --out-dir …` would fail cold — a bad
/// first-run experience. The template ships with all three flags
/// `false` precisely to avoid that; this test fences that choice.
///
/// Scoped to a tempdir so we don't depend on repo git state. The
/// generate may still fail downstream (no git env, etc.), but
/// must not fail on the policy gate — we check stderr does **not**
/// mention "does not enforce".
#[test]
fn test_init_template_does_not_trip_policy_gate() {
    let tmp = TempDir::new().unwrap();
    cargo_evidence()
        .arg("evidence")
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success();

    // Don't assert success of generate — the tempdir isn't a git
    // repo and later phases will fail. We only assert that the
    // specific policy-gate error string doesn't appear.
    let out = TempDir::new().unwrap();
    let result = cargo_evidence()
        .arg("evidence")
        .arg("generate")
        .arg("--out-dir")
        .arg(out.path())
        .arg("--profile")
        .arg("dev")
        .current_dir(tmp.path())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        !stderr.contains("does not enforce"),
        "init template tripped the policy-not-implemented gate; stderr:\n{}",
        stderr
    );
}

// TEST-080's integration arm lives in
// `crates/cargo-evidence/tests/dal_qualification_gate.rs` (its own
// integration-test file) so this orchestrator stays under the
// workspace 500-line limit.
