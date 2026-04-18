//! Shared helpers for the crate's integration tests.
//!
//! This file is included into each test binary under `tests/` via
//! `#[path = "helpers.rs"] mod helpers;`. Cargo also compiles
//! `helpers.rs` itself as its own (empty) test target — that's the
//! cost of sharing test scaffolding across the topic-split integration
//! tests without using a `mod.rs` pattern the project-wide style
//! rules discourage. Every item is `pub` + `#[allow(dead_code)]` so
//! individual test files can import just the subset they need.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers may be partially used by any given test binary"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use evidence::bundle::EvidenceIndex;
use evidence::git::GitError;
use evidence::hash::{sha256_file, write_sha256sums};
use evidence::traits::GitProvider;

use tempfile::TempDir;

// ============================================================================
// Mock GitProvider
// ============================================================================

/// A mock GitProvider that returns deterministic, fixed values.
pub struct MockGitProvider {
    pub sha: String,
    pub branch: String,
    pub dirty: bool,
}

impl MockGitProvider {
    pub fn clean() -> Self {
        Self {
            sha: "aabbccdd11223344aabbccdd11223344aabbccdd".to_string(),
            branch: "main".to_string(),
            dirty: false,
        }
    }

    pub fn dirty() -> Self {
        Self {
            sha: "aabbccdd11223344aabbccdd11223344aabbccdd".to_string(),
            branch: "main".to_string(),
            dirty: true,
        }
    }
}

impl GitProvider for MockGitProvider {
    fn sha(&self) -> Result<String, GitError> {
        Ok(self.sha.clone())
    }

    fn branch(&self) -> Result<String, GitError> {
        Ok(self.branch.clone())
    }

    fn is_dirty(&self) -> Result<bool, GitError> {
        Ok(self.dirty)
    }

    fn dirty_files(&self) -> Result<Vec<String>, GitError> {
        Ok(vec![])
    }
}

/// A mock GitProvider that always fails (simulates missing git repo).
pub struct FailingGitProvider;

impl GitProvider for FailingGitProvider {
    fn sha(&self) -> Result<String, GitError> {
        Err(GitError::Other("not a git repository".to_string()))
    }

    fn branch(&self) -> Result<String, GitError> {
        Err(GitError::Other("not a git repository".to_string()))
    }

    fn is_dirty(&self) -> Result<bool, GitError> {
        Err(GitError::Other("not a git repository".to_string()))
    }

    fn dirty_files(&self) -> Result<Vec<String>, GitError> {
        Err(GitError::Other("not a git repository".to_string()))
    }
}

// ============================================================================
// Create a minimal valid evidence bundle in a temp directory
// ============================================================================

/// Creates a minimal evidence bundle manually (without EvidenceBuilder::new,
/// which calls real git). Returns (TempDir, bundle_dir_path).
pub fn create_minimal_bundle(profile: &str) -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().expect("create tempdir");
    let bundle_dir = tmp
        .path()
        .join(format!("{}-20260207-000000Z-aabbccdd", profile));
    fs::create_dir_all(bundle_dir.join("tests")).unwrap();
    fs::create_dir_all(bundle_dir.join("trace")).unwrap();

    // env.json must deserialize cleanly into `evidence::EnvFingerprint`
    // because `verify_bundle` re-projects it and byte-compares against
    // the committed deterministic-manifest.json.
    let env_fp = evidence::EnvFingerprint {
        profile: profile.to_string(),
        rustc: "rustc 1.85.0".to_string(),
        cargo: "cargo 1.85.0".to_string(),
        git_sha: "aabbccdd11223344aabbccdd11223344aabbccdd".to_string(),
        git_branch: "main".to_string(),
        git_dirty: false,
        in_nix_shell: false,
        tools: BTreeMap::new(),
        nav_env: BTreeMap::new(),
        llvm_version: None,
        host: evidence::Host::Linux {
            arch: "x86_64".to_string(),
            libc: None,
            kernel: None,
        },
        cargo_lock_hash: None,
        rust_toolchain_toml: None,
        rustflags: None,
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
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
        schema_version: evidence::schema_versions::INDEX.to_string(),
        boundary_schema_version: evidence::schema_versions::BOUNDARY.to_string(),
        trace_schema_version: evidence::schema_versions::TRACE.to_string(),
        profile: profile.to_string(),
        timestamp_rfc3339: "2026-02-07T00:00:00Z".to_string(),
        git_sha: "aabbccdd11223344aabbccdd11223344aabbccdd".to_string(),
        git_branch: "main".to_string(),
        git_dirty: false,
        engine_crate_version: env!("CARGO_PKG_VERSION").to_string(),
        // Distinct constant from `git_sha` so tests that sed on
        // `git_sha` don't accidentally also mutate `engine_git_sha`.
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
        dal_map: BTreeMap::new(),
    };
    fs::write(
        bundle_dir.join("index.json"),
        serde_json::to_vec_pretty(&index).unwrap(),
    )
    .unwrap();

    (tmp, bundle_dir)
}

/// Rewrite `index.json` in `bundle_dir` with the given substitution.
/// `index.json` is excluded from SHA256SUMS, so edits don't rotate
/// `content_hash` and the bundle still verifies as long as the
/// envelope stays schema-valid.
pub fn replace_in_index(bundle_dir: &Path, from: &str, to: &str) {
    let index_path = bundle_dir.join("index.json");
    let content = fs::read_to_string(&index_path).unwrap();
    assert!(
        content.contains(from),
        "replace_in_index: '{from}' not found in index.json",
    );
    fs::write(&index_path, content.replace(from, to)).unwrap();
}

// ============================================================================
// Trace helpers
// ============================================================================

pub fn make_trace_meta() -> evidence::trace::TraceMeta {
    evidence::trace::TraceMeta {
        document_id: "DOC-001".to_string(),
        revision: "1.0".to_string(),
    }
}

pub fn make_schema() -> evidence::trace::Schema {
    evidence::trace::Schema {
        version: evidence::schema_versions::TRACE.to_string(),
    }
}
