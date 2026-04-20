//! Integration tests for `verify_bundle`'s path-safety and field-
//! format checks on `index.json`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

#[path = "helpers.rs"]
mod helpers;

use std::fs;

use evidence_core::verify::verify_bundle;

use helpers::create_minimal_bundle;

#[test]
fn test_verify_rejects_path_traversal_in_sha256sums() {
    let (_tmp, bundle_dir) = create_minimal_bundle(evidence_core::Profile::Dev);

    // Tamper SHA256SUMS to include a path-traversal entry.
    let sha256sums_path = bundle_dir.join("SHA256SUMS");
    let mut content = fs::read_to_string(&sha256sums_path).unwrap();
    content.push_str(
        "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef  ../../../etc/passwd\n",
    );
    fs::write(&sha256sums_path, &content).unwrap();

    let result = verify_bundle(&bundle_dir).unwrap();
    assert!(result.is_fail(), "Should fail with path traversal");
    let summary = result.summary();
    assert!(
        summary.contains("unsafe path") || summary.contains("Unsafe"),
        "Should mention unsafe path, got: {}",
        summary
    );
}

#[test]
fn test_verify_rejects_absolute_path_in_sha256sums() {
    let (_tmp, bundle_dir) = create_minimal_bundle(evidence_core::Profile::Dev);

    let sha256sums_path = bundle_dir.join("SHA256SUMS");
    let mut content = fs::read_to_string(&sha256sums_path).unwrap();
    content.push_str(
        "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef  /etc/shadow\n",
    );
    fs::write(&sha256sums_path, &content).unwrap();

    let result = verify_bundle(&bundle_dir).unwrap();
    assert!(result.is_fail(), "Should fail with absolute path");
    assert!(
        result.summary().contains("unsafe path"),
        "Should mention unsafe path, got: {}",
        result.summary()
    );
}

#[test]
fn test_verify_rejects_invalid_profile() {
    // Regression for the deferred "stringly-typed profile" concern:
    // with `EvidenceIndex.profile: Profile`, a tampered `"yolo"`
    // string can't even round-trip through serde. `verify_bundle`
    // surfaces this as `VerifyRuntimeError::ParseIndex` (a run-time
    // fault) rather than the legacy `VerifyResult::Fail([
    // FormatError{field:"profile", ...}])` — the bug is caught one
    // layer earlier, at deserialization, which also means library
    // consumers who skip verify still can't construct an
    // `EvidenceIndex` with a bogus profile.
    let (_tmp, bundle_dir) = create_minimal_bundle(evidence_core::Profile::Dev);

    let index_path = bundle_dir.join("index.json");
    let content = fs::read_to_string(&index_path).unwrap();
    let tampered = content.replace("\"dev\"", "\"yolo\"");
    fs::write(&index_path, &tampered).unwrap();

    let env_path = bundle_dir.join("env.json");
    let env_content = fs::read_to_string(&env_path).unwrap();
    let env_tampered = env_content.replace("\"dev\"", "\"yolo\"");
    fs::write(&env_path, &env_tampered).unwrap();

    let err = verify_bundle(&bundle_dir).expect_err("should fail to parse the tampered index");
    let msg = err.to_string();
    assert!(
        msg.contains("index.json"),
        "error should name index.json, got: {msg}"
    );
}

#[test]
fn test_verify_rejects_bad_git_sha_in_cert_profile() {
    let (_tmp, bundle_dir) = create_minimal_bundle(evidence_core::Profile::Cert);

    let index_path = bundle_dir.join("index.json");
    let content = fs::read_to_string(&index_path).unwrap();
    let tampered = content.replace(
        "aabbccdd11223344aabbccdd11223344aabbccdd",
        "not-a-valid-sha",
    );
    fs::write(&index_path, &tampered).unwrap();

    // Also tamper env.json so cross-file doesn't trigger.
    let env_path = bundle_dir.join("env.json");
    let env_content = fs::read_to_string(&env_path).unwrap();
    let env_tampered = env_content.replace(
        "aabbccdd11223344aabbccdd11223344aabbccdd",
        "not-a-valid-sha",
    );
    fs::write(&env_path, &env_tampered).unwrap();

    let result = verify_bundle(&bundle_dir).unwrap();
    assert!(result.is_fail(), "Should fail with bad git_sha for cert");
    assert!(
        result.summary().contains("git_sha"),
        "Should mention git_sha, got: {}",
        result.summary()
    );
}

#[test]
fn test_verify_allows_unknown_git_sha_in_dev_profile() {
    // Dev profile allows "unknown" git_sha — should not trigger FormatError.
    let (_tmp, bundle_dir) = create_minimal_bundle(evidence_core::Profile::Dev);

    // Set git_sha to "unknown" in both files.
    let index_path = bundle_dir.join("index.json");
    let content = fs::read_to_string(&index_path).unwrap();
    let tampered = content.replace("aabbccdd11223344aabbccdd11223344aabbccdd", "unknown");
    fs::write(&index_path, &tampered).unwrap();

    let env_path = bundle_dir.join("env.json");
    let env_content = fs::read_to_string(&env_path).unwrap();
    let env_tampered = env_content.replace("aabbccdd11223344aabbccdd11223344aabbccdd", "unknown");
    fs::write(&env_path, &env_tampered).unwrap();

    let result = verify_bundle(&bundle_dir).unwrap();
    let summary = result.summary();
    assert!(
        !summary.contains("git_sha"),
        "Dev profile should not flag git_sha='unknown', got: {}",
        summary
    );
}
