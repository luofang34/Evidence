//! Integration tests for `verify_bundle`'s cross-check passes:
//! `engine_build_source` ↔ `engine_git_sha`, trace_outputs presence
//! in SHA256SUMS, env.json ↔ index.json field consistency.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

#[path = "helpers.rs"]
mod helpers;

use std::fs;

use evidence::verify::verify_bundle;

use helpers::{create_minimal_bundle, replace_in_index};

// ============================================================================
// engine_build_source cross-check vs engine_git_sha
// ============================================================================

#[test]
fn test_verify_rejects_git_source_with_nonhex_sha() {
    // source="git" but engine_git_sha is a release-style string —
    // exactly the drift we want the verifier to catch.
    let (_tmp, bundle_dir) = create_minimal_bundle(evidence::Profile::Dev);
    replace_in_index(
        &bundle_dir,
        "eeff001122334455667788990011223344556677",
        "release-v0.1.0",
    );
    let result = verify_bundle(&bundle_dir).unwrap();
    assert!(
        result.is_fail(),
        "should fail when source=git but sha is non-hex"
    );
    assert!(
        result.summary().contains("engine_git_sha"),
        "summary should name engine_git_sha, got: {}",
        result.summary()
    );
}

#[test]
fn test_verify_accepts_release_source_on_dev_profile() {
    // source="release" with a legitimate release-v... string on dev.
    let (_tmp, bundle_dir) = create_minimal_bundle(evidence::Profile::Dev);
    replace_in_index(
        &bundle_dir,
        "eeff001122334455667788990011223344556677",
        "release-v0.1.0",
    );
    replace_in_index(
        &bundle_dir,
        "\"engine_build_source\": \"git\"",
        "\"engine_build_source\": \"release\"",
    );
    let result = verify_bundle(&bundle_dir).unwrap();
    assert!(
        result.is_pass(),
        "dev profile should accept release source; got: {}",
        result.summary()
    );
}

#[test]
fn test_verify_rejects_release_source_on_cert_profile() {
    // Same release shape on cert profile should be rejected: cert
    // bundles must be pinned to a commit.
    let (_tmp, bundle_dir) = create_minimal_bundle(evidence::Profile::Cert);
    replace_in_index(
        &bundle_dir,
        "eeff001122334455667788990011223344556677",
        "release-v0.1.0",
    );
    replace_in_index(
        &bundle_dir,
        "\"engine_build_source\": \"git\"",
        "\"engine_build_source\": \"release\"",
    );
    let result = verify_bundle(&bundle_dir).unwrap();
    assert!(result.is_fail(), "cert profile must reject release source");
    assert!(
        result.summary().contains("engine_build_source"),
        "summary should name engine_build_source, got: {}",
        result.summary()
    );
}

#[test]
fn test_verify_rejects_unknown_source_on_cert_profile() {
    // Legacy-shaped bundle (source="unknown") on cert profile must
    // fail: cert cannot accept a bundle whose engine provenance is
    // unlabeled.
    let (_tmp, bundle_dir) = create_minimal_bundle(evidence::Profile::Cert);
    replace_in_index(
        &bundle_dir,
        "\"engine_build_source\": \"git\"",
        "\"engine_build_source\": \"unknown\"",
    );
    let result = verify_bundle(&bundle_dir).unwrap();
    assert!(result.is_fail(), "cert profile must reject unknown source");
    assert!(
        result.summary().contains("engine_build_source"),
        "summary should name engine_build_source, got: {}",
        result.summary()
    );
}

// ============================================================================
// trace_outputs ↔ SHA256SUMS
// ============================================================================

#[test]
fn test_verify_rejects_phantom_trace_output_not_in_sha256sums() {
    // Add an entry to index.json.trace_outputs pointing at a path
    // that isn't listed in SHA256SUMS. This is the tampering path
    // the cross-check is designed to catch (an attacker overclaiming
    // coverage without having to forge any hashed content).
    let (_tmp, bundle_dir) = create_minimal_bundle(evidence::Profile::Dev);
    replace_in_index(
        &bundle_dir,
        "\"trace_outputs\": []",
        "\"trace_outputs\": [\n    \"trace/phantom.md\"\n  ]",
    );
    let result = verify_bundle(&bundle_dir).unwrap();
    assert!(
        result.is_fail(),
        "phantom trace_outputs entry must be rejected"
    );
    assert!(
        result.summary().contains("trace_outputs") && result.summary().contains("phantom"),
        "summary should name the phantom path, got: {}",
        result.summary()
    );
}

// ============================================================================
// env.json vs index.json cross-file consistency
// ============================================================================

#[test]
fn test_verify_detects_env_index_profile_mismatch() {
    let (_tmp, bundle_dir) = create_minimal_bundle(evidence::Profile::Dev);

    let env_path = bundle_dir.join("env.json");
    let content = fs::read_to_string(&env_path).unwrap();
    let tampered = content.replace("\"dev\"", "\"cert\"");
    fs::write(&env_path, &tampered).unwrap();

    let result = verify_bundle(&bundle_dir).unwrap();
    assert!(result.is_fail(), "Should fail with profile mismatch");
    let summary = result.summary();
    assert!(
        summary.contains("mismatch") && summary.contains("profile"),
        "Should mention profile mismatch, got: {}",
        summary
    );
}

#[test]
fn test_verify_detects_env_index_git_sha_mismatch() {
    let (_tmp, bundle_dir) = create_minimal_bundle(evidence::Profile::Dev);

    let env_path = bundle_dir.join("env.json");
    let content = fs::read_to_string(&env_path).unwrap();
    let tampered = content.replace(
        "aabbccdd11223344aabbccdd11223344aabbccdd",
        "1111111111111111111111111111111111111111",
    );
    fs::write(&env_path, &tampered).unwrap();

    let result = verify_bundle(&bundle_dir).unwrap();
    assert!(result.is_fail(), "Should fail with git_sha mismatch");
    let summary = result.summary();
    assert!(
        summary.contains("mismatch") && summary.contains("git_sha"),
        "Should mention git_sha mismatch, got: {}",
        summary
    );
}
