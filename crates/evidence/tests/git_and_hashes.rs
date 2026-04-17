//! Integration tests for `GitSnapshot::capture_with` and the raw
//! SHA-256 hashing primitives.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

#[path = "helpers.rs"]
mod helpers;

use std::fs;

use evidence::git::GitSnapshot;
use evidence::hash::{sha256, sha256_file};

use tempfile::TempDir;

use helpers::{FailingGitProvider, MockGitProvider};

#[test]
fn test_cert_mode_strict_errors_missing_git() {
    // With strict=true, a failing git provider must produce an error,
    // not fall back to "unknown".
    let provider = FailingGitProvider;
    let result = GitSnapshot::capture_with(&provider, true);
    assert!(
        result.is_err(),
        "strict mode should error on missing git repo, not return 'unknown'"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("cert") || err_msg.contains("valid git"),
        "Error should mention cert/record profile requirement, got: {}",
        err_msg
    );
}

#[test]
fn test_non_strict_mode_allows_unknown_git() {
    // Without strict mode, a failing provider should fall back to "unknown".
    let provider = FailingGitProvider;
    let result = GitSnapshot::capture_with(&provider, false);
    assert!(
        result.is_ok(),
        "non-strict mode should succeed with unknown git"
    );
    let snapshot = result.unwrap();
    assert_eq!(snapshot.sha, "unknown");
    assert_eq!(snapshot.branch, "unknown");
}

#[test]
fn test_git_snapshot_with_mock_provider() {
    let provider = MockGitProvider::clean();
    let snapshot = GitSnapshot::capture_with(&provider, false).unwrap();
    assert_eq!(snapshot.sha, "aabbccdd11223344aabbccdd11223344aabbccdd");
    assert_eq!(snapshot.branch, "main");
    assert!(!snapshot.dirty);
}

#[test]
fn test_git_snapshot_dirty_with_mock() {
    let provider = MockGitProvider::dirty();
    let snapshot = GitSnapshot::capture_with(&provider, false).unwrap();
    assert!(snapshot.dirty);
}

#[test]
fn test_sha256_known_value() {
    // Known test vector
    let hash = sha256(b"hello world");
    assert_eq!(
        hash,
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
}

#[test]
fn test_sha256_file_matches_sha256_bytes() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("test.txt");
    let content = b"deterministic content for hashing";
    fs::write(&file_path, content).unwrap();

    let file_hash = sha256_file(&file_path).unwrap();
    let bytes_hash = sha256(content);
    assert_eq!(
        file_hash, bytes_hash,
        "sha256_file and sha256 must agree on the same content"
    );
}
