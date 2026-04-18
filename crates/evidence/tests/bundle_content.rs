//! Integration tests for the content layer: content-hash integrity,
//! tampering detection, metadata/content separation, and path
//! normalization into SHA256SUMS.

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

use evidence::bundle::EvidenceIndex;
use evidence::hash::{sha256_file, write_sha256sums};
use evidence::verify::verify_bundle;

use tempfile::TempDir;

use helpers::create_minimal_bundle;

#[test]
fn test_content_hash_integrity() {
    let (_tmp, bundle_dir) = create_minimal_bundle(evidence::Profile::Dev);

    let index_content = fs::read_to_string(bundle_dir.join("index.json")).unwrap();
    let index: EvidenceIndex = serde_json::from_str(&index_content).unwrap();

    let sha256sums_hash = sha256_file(&bundle_dir.join("SHA256SUMS")).unwrap();

    assert_eq!(
        index.content_hash, sha256sums_hash,
        "content_hash in index.json must equal SHA256(SHA256SUMS)"
    );
}

#[test]
fn test_tampering_detection() {
    let (_tmp, bundle_dir) = create_minimal_bundle(evidence::Profile::Dev);

    let result = verify_bundle(&bundle_dir).unwrap();
    assert!(result.is_pass(), "Bundle should pass before tampering");

    // Tamper with a file inside the bundle
    let env_path = bundle_dir.join("env.json");
    let mut content = fs::read_to_string(&env_path).unwrap();
    content.push_str("\n/* tampered */");
    fs::write(&env_path, content).unwrap();

    let result = verify_bundle(&bundle_dir).unwrap();
    assert!(
        result.is_fail(),
        "Tampered bundle should cause verify_bundle to return Fail, got: {}",
        result.summary()
    );
    let summary = result.summary();
    assert!(
        summary.contains("hash mismatch") || summary.contains("Hash"),
        "Failure should mention hash mismatch, got: {}",
        summary
    );
}

#[test]
fn test_index_json_excluded_from_sha256sums() {
    let (_tmp, bundle_dir) = create_minimal_bundle(evidence::Profile::Dev);

    let sha256sums_content = fs::read_to_string(bundle_dir.join("SHA256SUMS")).unwrap();

    // index.json must NOT appear in SHA256SUMS (metadata layer separation).
    for line in sha256sums_content.lines() {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, "  ").collect();
        if parts.len() == 2 {
            assert_ne!(
                parts[1], "index.json",
                "index.json must NOT be listed in SHA256SUMS"
            );
        }
    }
}

#[test]
fn test_path_normalization_forward_slashes() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("sub").join("file.txt"), b"test content").unwrap();
    fs::write(root.join("top.txt"), b"top level").unwrap();

    let sha256sums_path = root.join("SHA256SUMS");
    write_sha256sums(root, &sha256sums_path).unwrap();

    let content = fs::read_to_string(&sha256sums_path).unwrap();
    for line in content.lines() {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, "  ").collect();
        assert_eq!(parts.len(), 2, "SHA256SUMS line should have hash and path");
        let path = parts[1];
        assert!(
            !path.contains('\\'),
            "Path '{}' in SHA256SUMS should not contain backslashes",
            path
        );
    }

    // Also verify hash_file_relative_into normalizes paths.
    let mut map = BTreeMap::new();
    evidence::hash::hash_file_relative_into(&mut map, &root.join("sub").join("file.txt"), root)
        .unwrap();

    for key in map.keys() {
        assert!(
            !key.contains('\\'),
            "Relative path key '{}' should not contain backslashes",
            key
        );
        assert!(
            key.contains("sub/file.txt"),
            "Key should contain 'sub/file.txt', got '{}'",
            key
        );
    }
}
