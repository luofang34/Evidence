#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::BTreeMap;
use std::path::Path;

use tempfile::TempDir;

use super::{EvidenceBuildConfig, EvidenceBuilder};
use crate::bundle::BuilderError;
use crate::git::GitError;
use crate::policy::{BoundaryPolicy, Profile};
use crate::traits::GitProvider;

struct FixedGitProvider;

impl GitProvider for FixedGitProvider {
    fn sha(&self) -> Result<String, GitError> {
        Ok("aabbccdd11223344aabbccdd11223344aabbccdd".to_string())
    }

    fn branch(&self) -> Result<String, GitError> {
        Ok("main".to_string())
    }

    fn is_dirty(&self) -> Result<bool, GitError> {
        Ok(false)
    }

    fn dirty_files(&self) -> Result<Vec<String>, GitError> {
        Ok(Vec::new())
    }
}

fn config(output_root: &Path) -> EvidenceBuildConfig {
    EvidenceBuildConfig {
        output_root: output_root.to_path_buf(),
        profile: Profile::Dev,
        in_scope_crates: Vec::new(),
        trace_roots: Vec::new(),
        require_clean_git: false,
        fail_on_dirty: false,
        dal_map: BTreeMap::new(),
        boundary_policy: BoundaryPolicy::default(),
        resolution_policy: crate::policy::ResolutionPolicy::LOCKED_OFFLINE,
        skip_tests: false,
    }
}

#[test]
fn fixed_timestamp_rejects_existing_bundle_directory() {
    let output = TempDir::new().expect("create output directory");
    let timestamp = "20260719-000000Z";
    let first =
        EvidenceBuilder::new_with_provider_at(config(output.path()), FixedGitProvider, timestamp)
            .expect("first builder should create the bundle directory");

    let second =
        EvidenceBuilder::new_with_provider_at(config(output.path()), FixedGitProvider, timestamp);

    assert!(
        matches!(second, Err(BuilderError::BundleExists { ref path }) if path == first.bundle_dir()),
        "the fixed timestamp and git SHA must deterministically collide"
    );
}

/// TEST-162 (generate half): `finalize` writes the enriched recipe
/// manifest — target triple, features, locked-graph hash, command
/// recipe hash, inputs hash, resolution policy — and records its
/// SHA-256 as `index.json.recipe_hash` (never the legacy key).
#[test]
fn finalize_writes_recipe_manifest_and_recipe_hash() {
    let output = TempDir::new().expect("create output directory");
    let mut builder = EvidenceBuilder::new_with_provider_at(
        config(output.path()),
        FixedGitProvider,
        "20260722-000000Z",
    )
    .expect("builder");
    let bundle_dir = builder.bundle_dir().to_path_buf();

    // Production's capture phase writes env.json before finalize;
    // the test drives the same contract by hand.
    let env_fp = crate::env::EnvFingerprint {
        profile: Profile::Dev,
        rustc: "rustc 1.95.0".to_string(),
        cargo: "cargo 1.95.0".to_string(),
        git_sha: "aabbccdd11223344aabbccdd11223344aabbccdd".to_string(),
        git_branch: "main".to_string(),
        git_dirty: false,
        in_nix_shell: false,
        tools: BTreeMap::new(),
        nav_env: BTreeMap::new(),
        llvm_version: None,
        host: crate::env::Host::Linux {
            arch: "x86_64".to_string(),
            libc: None,
            kernel: None,
        },
        cargo_lock_hash: None,
        rust_toolchain_toml: None,
        rustflags: None,
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        tool_prerelease: false,
    };
    std::fs::write(
        bundle_dir.join("env.json"),
        serde_json::to_vec_pretty(&env_fp).expect("serialize env"),
    )
    .expect("write env.json");

    // One real source input through the real hashing path.
    let ws = TempDir::new().expect("workspace");
    std::fs::write(ws.path().join("main.rs"), b"fn main() {}\n").expect("write source");
    builder
        .hash_input_under(ws.path(), "main.rs")
        .expect("hash input");
    builder.write_inputs().expect("write inputs");
    builder.write_outputs().expect("write outputs");
    builder.write_commands().expect("write commands");

    builder.finalize(Vec::new()).expect("finalize");

    let manifest_bytes =
        std::fs::read(bundle_dir.join("deterministic-manifest.json")).expect("read manifest");
    let manifest: crate::env::RecipeManifest =
        serde_json::from_slice(&manifest_bytes).expect("parse manifest");
    assert_eq!(manifest.target_triple, "x86_64-unknown-linux-gnu");
    assert!(manifest.features.is_empty());
    // Dev profile without boundary flags: no cargo_metadata.json,
    // so the locked-graph hash records null.
    assert_eq!(manifest.locked_graph_hash, None);
    assert_eq!(
        manifest.resolution_policy,
        crate::policy::ResolutionPolicy::LockedOffline
    );
    let recorded_inputs: BTreeMap<String, String> = serde_json::from_slice(
        &std::fs::read(bundle_dir.join("inputs_hashes.json")).expect("read inputs"),
    )
    .expect("parse inputs");
    assert_eq!(
        manifest.inputs_hash,
        crate::env::inputs_digest(&recorded_inputs).expect("inputs digest")
    );

    let raw_index =
        std::fs::read_to_string(bundle_dir.join("index.json")).expect("read index.json");
    assert!(raw_index.contains("\"recipe_hash\""));
    assert!(
        !raw_index.contains("deterministic_hash"),
        "index.json must never emit the legacy key: {raw_index}"
    );
    let index: crate::bundle::EvidenceIndex =
        serde_json::from_str(&raw_index).expect("parse index.json");
    assert_eq!(index.recipe_hash, crate::hash::sha256(&manifest_bytes));
}
