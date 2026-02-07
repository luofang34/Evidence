//! Integration tests for the cargo-evidence CLI.
//!
//! These tests exercise the binary end-to-end using `assert_cmd`.
//! Each test runs in an isolated temp directory.

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
    let tmp = TempDir::new().unwrap();
    let trace_dir = tmp.path().join("trace");
    fs::create_dir_all(&trace_dir).unwrap();

    // Minimal valid HLR
    fs::write(
        trace_dir.join("hlr.toml"),
        r#"
[meta]
document_id = "HLR-001"
revision = "1.0"

[schema]
version = "0.0.1"

[[requirements]]
id = "HLR-1"
title = "Test requirement"
owner = "soi"
uid = "11111111-1111-1111-1111-111111111111"
verification_methods = ["review"]
"#,
    )
    .unwrap();

    // Minimal valid LLR pointing to HLR
    fs::write(
        trace_dir.join("llr.toml"),
        r#"
[meta]
document_id = "LLR-001"
revision = "1.0"

[schema]
version = "0.0.1"

[[requirements]]
id = "LLR-1"
title = "Implementation detail"
owner = "soi"
uid = "22222222-2222-2222-2222-222222222222"
derived = false
traces_to = ["11111111-1111-1111-1111-111111111111"]
verification_methods = ["unit_test"]
"#,
    )
    .unwrap();

    // Minimal valid Tests pointing to LLR
    fs::write(
        trace_dir.join("tests.toml"),
        r#"
[meta]
document_id = "TESTS-001"
revision = "1.0"

[schema]
version = "0.0.1"

[[tests]]
id = "TEST-1"
title = "Verify LLR-1"
owner = "soi"
uid = "33333333-3333-3333-3333-333333333333"
traces_to = ["22222222-2222-2222-2222-222222222222"]
verification_method = "unit_test"
"#,
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
