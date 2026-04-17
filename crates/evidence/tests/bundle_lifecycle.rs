//! Bundle-lifecycle integration tests: roundtrip, determinism of
//! `SHA256SUMS`, overwrite protection, directory naming, and the
//! TOCTOU re-check at finalize time.

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

use evidence::Profile;
use evidence::bundle::{EvidenceBuildConfig, EvidenceBuilder};
use evidence::hash::write_sha256sums;
use evidence::traits::GitProvider;
use evidence::verify::verify_bundle;

use tempfile::TempDir;

use helpers::{MockGitProvider, create_minimal_bundle};

#[test]
fn test_bundle_roundtrip() {
    let (_tmp, bundle_dir) = create_minimal_bundle("dev");
    let result = verify_bundle(&bundle_dir).expect("verify_bundle should not error");
    assert!(
        result.is_pass(),
        "Expected VerifyResult::Pass, got {:?}",
        result
    );
}

#[test]
fn test_determinism_sha256sums_identical() {
    // Create two bundles with the exact same content
    let tmp = TempDir::new().unwrap();

    let bundle_a = tmp.path().join("bundle-a");
    let bundle_b = tmp.path().join("bundle-b");

    for bundle_dir in [&bundle_a, &bundle_b] {
        fs::create_dir_all(bundle_dir.join("tests")).unwrap();
        fs::create_dir_all(bundle_dir.join("trace")).unwrap();

        let env_json = serde_json::json!({
            "profile": "dev",
            "rustc": "rustc 1.85.0",
            "cargo": "cargo 1.85.0",
            "git_sha": "aabbccdd11223344aabbccdd11223344aabbccdd",
            "git_branch": "main",
            "git_dirty": false,
            "in_nix_shell": false,
            "tools": {},
            "nav_env": {}
        });
        fs::write(
            bundle_dir.join("env.json"),
            serde_json::to_vec_pretty(&env_json).unwrap(),
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
        write_sha256sums(bundle_dir, &sha256sums_path).unwrap();
    }

    let sums_a = fs::read(bundle_a.join("SHA256SUMS")).unwrap();
    let sums_b = fs::read(bundle_b.join("SHA256SUMS")).unwrap();
    assert_eq!(
        sums_a, sums_b,
        "SHA256SUMS must be byte-identical for identical content"
    );
}

#[test]
fn test_overwrite_protection() {
    let tmp = TempDir::new().unwrap();

    let make_config = || EvidenceBuildConfig {
        output_root: tmp.path().to_path_buf(),
        profile: Profile::Dev,
        in_scope_crates: vec![],
        trace_roots: vec![],
        require_clean_git: false,
        fail_on_dirty: false,
        dal_map: BTreeMap::new(),
    };

    // First builder succeeds and creates the bundle directory.
    let _builder1 = EvidenceBuilder::new_with_provider(make_config(), MockGitProvider::clean())
        .expect("first builder should succeed");

    // Second builder called immediately (same second, same SHA) must fail
    // because the bundle directory already exists.
    let result = EvidenceBuilder::new_with_provider(make_config(), MockGitProvider::clean());
    match result {
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("already exists"),
                "Error should mention 'already exists', got: {}",
                msg
            );
        }
        Ok(_) => panic!("Second builder in same second should fail (overwrite protection)"),
    }
}

#[test]
fn test_profile_in_directory_name() {
    for profile in &["cert", "dev", "record"] {
        let (_tmp, bundle_dir) = create_minimal_bundle(profile);
        let dir_name = bundle_dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(
            dir_name.starts_with(&format!("{}-", profile)),
            "Bundle directory '{}' should start with '{}-'",
            dir_name,
            profile
        );
    }
}

// ============================================================================
// TOCTOU detection — git HEAD change between new() and finalize()
// ============================================================================

/// A mock GitProvider that changes its SHA after the first call,
/// simulating a commit happening during evidence generation.
struct MutatingGitProvider {
    call_count: std::cell::Cell<u32>,
}

impl MutatingGitProvider {
    fn new() -> Self {
        Self {
            call_count: std::cell::Cell::new(0),
        }
    }
}

impl GitProvider for MutatingGitProvider {
    fn sha(&self) -> anyhow::Result<String> {
        let n = self.call_count.get();
        self.call_count.set(n + 1);
        if n == 0 {
            Ok("aabbccdd11223344aabbccdd11223344aabbccdd".to_string())
        } else {
            Ok("1111111122222222333333334444444455555555".to_string())
        }
    }

    fn branch(&self) -> anyhow::Result<String> {
        Ok("main".to_string())
    }

    fn is_dirty(&self) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn dirty_files(&self) -> anyhow::Result<Vec<String>> {
        Ok(vec![])
    }
}

#[test]
fn test_toctou_detection() {
    let tmp = TempDir::new().unwrap();

    let config = EvidenceBuildConfig {
        output_root: tmp.path().to_path_buf(),
        profile: Profile::Dev,
        in_scope_crates: vec![],
        trace_roots: vec![],
        require_clean_git: false,
        fail_on_dirty: false,
        dal_map: BTreeMap::new(),
    };

    let builder = EvidenceBuilder::new_with_provider(config, MutatingGitProvider::new())
        .expect("builder should succeed");

    // Write minimal bundle files so finalize can proceed to the TOCTOU check
    let bundle_dir = builder.bundle_dir();
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
    fs::write(
        bundle_dir.join("env.json"),
        serde_json::json!({"profile":"dev"}).to_string(),
    )
    .unwrap();

    builder.write_inputs().unwrap();
    builder.write_outputs().unwrap();
    builder.write_commands().unwrap();

    let result = builder.finalize(vec![]);
    assert!(
        result.is_err(),
        "finalize should fail when git HEAD changed"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("TOCTOU"),
        "Error should mention TOCTOU, got: {}",
        err
    );
}
