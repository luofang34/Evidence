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
