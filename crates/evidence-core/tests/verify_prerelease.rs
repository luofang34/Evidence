//! Pre-release tool detection end-to-end (TEST-049).
//!
//! Exercises `verify_bundle` against bundles whose `env.json` sets
//! `tool_prerelease = true`. The library pushes
//! `VerifyError::PrereleaseToolDetected` on every profile; the
//! CLI-side severity downgrade for Dev is exercised in
//! `cargo-evidence/tests/verify_prerelease.rs`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

#[path = "helpers.rs"]
mod helpers;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use evidence_core::bundle::EvidenceIndex;
use evidence_core::hash::{sha256_file, write_sha256sums};
use evidence_core::verify::{VerifyError, VerifyResult, verify_bundle};

use tempfile::TempDir;

/// Build a bundle with `env.json.tool_prerelease = true`. Re-hashes
/// SHA256SUMS and content_hash so the bundle is internally
/// consistent — only the prerelease flag is abnormal.
fn build_prerelease_bundle(profile: evidence_core::Profile) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let bundle_dir = tmp
        .path()
        .join(format!("{}-20260207-000000Z-aabbccdd", profile));
    fs::create_dir_all(&bundle_dir).unwrap();

    let env_fp = evidence_core::EnvFingerprint {
        profile,
        rustc: "rustc 1.85.0".to_string(),
        cargo: "cargo 1.85.0".to_string(),
        git_sha: "aabbccdd11223344aabbccdd11223344aabbccdd".to_string(),
        git_branch: "main".to_string(),
        git_dirty: false,
        in_nix_shell: false,
        tools: BTreeMap::new(),
        nav_env: BTreeMap::new(),
        llvm_version: None,
        host: evidence_core::Host::Linux {
            arch: "x86_64".to_string(),
            libc: None,
            kernel: None,
        },
        cargo_lock_hash: None,
        rust_toolchain_toml: None,
        rustflags: None,
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        tool_prerelease: true,
    };
    fs::write(
        bundle_dir.join("env.json"),
        serde_json::to_vec_pretty(&env_fp).unwrap(),
    )
    .unwrap();

    let manifest = env_fp.deterministic_manifest();
    fs::write(
        bundle_dir.join("deterministic-manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let empty_map: BTreeMap<String, String> = BTreeMap::new();
    fs::write(
        bundle_dir.join("inputs_hashes.json"),
        serde_json::to_vec_pretty(&empty_map).unwrap(),
    )
    .unwrap();
    fs::write(
        bundle_dir.join("outputs_hashes.json"),
        serde_json::to_vec_pretty(&empty_map).unwrap(),
    )
    .unwrap();
    let empty_cmds: Vec<serde_json::Value> = vec![];
    fs::write(
        bundle_dir.join("commands.json"),
        serde_json::to_vec_pretty(&empty_cmds).unwrap(),
    )
    .unwrap();

    let sha256sums_path = bundle_dir.join("SHA256SUMS");
    write_sha256sums(&bundle_dir, &sha256sums_path).unwrap();
    let content_hash = sha256_file(&sha256sums_path).unwrap();
    let deterministic_hash = sha256_file(&bundle_dir.join("deterministic-manifest.json")).unwrap();

    let index = EvidenceIndex {
        schema_version: evidence_core::schema_versions::INDEX.to_string(),
        boundary_schema_version: evidence_core::schema_versions::BOUNDARY.to_string(),
        trace_schema_version: evidence_core::schema_versions::TRACE.to_string(),
        profile,
        timestamp_rfc3339: "2026-02-07T00:00:00Z".to_string(),
        git_sha: "aabbccdd11223344aabbccdd11223344aabbccdd".to_string(),
        git_branch: "main".to_string(),
        git_dirty: false,
        engine_crate_version: "0.1.0-pre.1".to_string(),
        engine_git_sha: "eeff001122334455667788990011223344556677".to_string(),
        engine_build_source: "git".to_string(),
        inputs_hashes_file: "inputs_hashes.json".to_string(),
        outputs_hashes_file: "outputs_hashes.json".to_string(),
        commands_file: "commands.json".to_string(),
        env_fingerprint_file: "env.json".to_string(),
        trace_roots: vec![],
        trace_outputs: vec![],
        bundle_complete: true,
        content_hash,
        deterministic_hash,
        test_summary: None,
        tool_command_failures: Vec::new(),
        dal_map: BTreeMap::new(),
    };
    fs::write(
        bundle_dir.join("index.json"),
        serde_json::to_vec_pretty(&index).unwrap(),
    )
    .unwrap();

    (tmp, bundle_dir)
}

#[test]
fn verify_library_pushes_prerelease_finding_on_cert_bundle() {
    let (_tmp, bundle) = build_prerelease_bundle(evidence_core::Profile::Cert);
    let result = verify_bundle(&bundle).expect("runtime ok");
    let errors = match result {
        VerifyResult::Fail(errs) => errs,
        other => panic!("expected Fail; got {:?}", other.summary()),
    };
    let found = errors.iter().any(|e| {
        matches!(
            e,
            VerifyError::PrereleaseToolDetected {
                profile,
                engine_crate_version
            } if profile == "cert" && engine_crate_version == "0.1.0-pre.1"
        )
    });
    assert!(
        found,
        "cert bundle with tool_prerelease=true must fire \
         VerifyError::PrereleaseToolDetected; got {:?}",
        errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
    );
}

#[test]
fn verify_library_pushes_prerelease_finding_on_dev_bundle_too() {
    // Library is policy-free — pushes the finding regardless of
    // profile. The CLI downgrades severity for dev; that split is
    // covered in `cargo-evidence/tests/verify_prerelease.rs`.
    let (_tmp, bundle) = build_prerelease_bundle(evidence_core::Profile::Dev);
    let result = verify_bundle(&bundle).expect("runtime ok");
    let errors = match result {
        VerifyResult::Fail(errs) => errs,
        other => panic!("expected Fail; got {:?}", other.summary()),
    };
    let found = errors.iter().any(|e| {
        matches!(
            e,
            VerifyError::PrereleaseToolDetected { profile, .. } if profile == "dev"
        )
    });
    assert!(
        found,
        "dev bundle also pushes the finding at the library layer"
    );
}
