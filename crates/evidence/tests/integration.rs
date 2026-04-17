//! Comprehensive integration tests for the evidence library.
//!
//! These tests exercise the PUBLIC API end-to-end, using mock GitProvider
//! implementations and tempfile::TempDir for filesystem isolation.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::BTreeMap;
use std::fs;

use evidence::Profile;
use evidence::bundle::{EvidenceBuildConfig, EvidenceBuilder, EvidenceIndex};
use evidence::git::GitSnapshot;
use evidence::hash::{sha256, sha256_file, write_sha256sums};
use evidence::trace::{
    HlrEntry, HlrFile, LlrEntry, LlrFile, Schema, TestEntry, TestsFile, TraceMeta,
    generate_traceability_matrix, validate_trace_links,
};
use evidence::traits::GitProvider;
use evidence::verify::verify_bundle;

use tempfile::TempDir;

// ============================================================================
// Mock GitProvider
// ============================================================================

/// A mock GitProvider that returns deterministic, fixed values.
struct MockGitProvider {
    sha: String,
    branch: String,
    dirty: bool,
}

impl MockGitProvider {
    fn clean() -> Self {
        Self {
            sha: "aabbccdd11223344aabbccdd11223344aabbccdd".to_string(),
            branch: "main".to_string(),
            dirty: false,
        }
    }

    fn dirty() -> Self {
        Self {
            sha: "aabbccdd11223344aabbccdd11223344aabbccdd".to_string(),
            branch: "main".to_string(),
            dirty: true,
        }
    }
}

impl GitProvider for MockGitProvider {
    fn sha(&self) -> anyhow::Result<String> {
        Ok(self.sha.clone())
    }

    fn branch(&self) -> anyhow::Result<String> {
        Ok(self.branch.clone())
    }

    fn is_dirty(&self) -> anyhow::Result<bool> {
        Ok(self.dirty)
    }

    fn dirty_files(&self) -> anyhow::Result<Vec<String>> {
        Ok(vec![])
    }
}

/// A mock GitProvider that always fails (simulates missing git repo).
struct FailingGitProvider;

impl GitProvider for FailingGitProvider {
    fn sha(&self) -> anyhow::Result<String> {
        anyhow::bail!("not a git repository")
    }

    fn branch(&self) -> anyhow::Result<String> {
        anyhow::bail!("not a git repository")
    }

    fn is_dirty(&self) -> anyhow::Result<bool> {
        anyhow::bail!("not a git repository")
    }

    fn dirty_files(&self) -> anyhow::Result<Vec<String>> {
        anyhow::bail!("not a git repository")
    }
}

// ============================================================================
// Helper: Create a minimal valid evidence bundle in a temp directory
// ============================================================================

/// Creates a minimal evidence bundle manually (without EvidenceBuilder::new,
/// which calls real git). Returns (TempDir, bundle_dir_path).
fn create_minimal_bundle(profile: &str) -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().expect("create tempdir");
    let bundle_dir = tmp
        .path()
        .join(format!("{}-20260207-000000Z-aabbccdd", profile));
    fs::create_dir_all(bundle_dir.join("tests")).unwrap();
    fs::create_dir_all(bundle_dir.join("trace")).unwrap();

    // Write env.json. Must deserialize cleanly into
    // `evidence::EnvFingerprint` because `verify_bundle` re-projects
    // it and compares byte-for-byte against the committed
    // deterministic-manifest.json.
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

    // Write deterministic-manifest.json using the library's
    // projection so the re-projection check inside verify_bundle
    // matches byte-for-byte.
    let manifest = env_fp.deterministic_manifest();
    fs::write(
        bundle_dir.join("deterministic-manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // Write inputs_hashes.json (empty map)
    let empty_map: BTreeMap<String, String> = BTreeMap::new();
    fs::write(
        bundle_dir.join("inputs_hashes.json"),
        serde_json::to_vec_pretty(&empty_map).unwrap(),
    )
    .unwrap();

    // Write outputs_hashes.json (empty map)
    fs::write(
        bundle_dir.join("outputs_hashes.json"),
        serde_json::to_vec_pretty(&empty_map).unwrap(),
    )
    .unwrap();

    // Write commands.json (empty array)
    let empty_cmds: Vec<serde_json::Value> = vec![];
    fs::write(
        bundle_dir.join("commands.json"),
        serde_json::to_vec_pretty(&empty_cmds).unwrap(),
    )
    .unwrap();

    // Write SHA256SUMS (content layer, excludes index.json)
    let sha256sums_path = bundle_dir.join("SHA256SUMS");
    write_sha256sums(&bundle_dir, &sha256sums_path).unwrap();

    // Compute hashes: content_hash (full SHA256SUMS) +
    // deterministic_hash (the manifest projection).
    let content_hash = sha256_file(&sha256sums_path).unwrap();
    let deterministic_hash = sha256_file(&bundle_dir.join("deterministic-manifest.json")).unwrap();

    // Write index.json (metadata layer)
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
        dal_map: std::collections::BTreeMap::new(),
    };
    fs::write(
        bundle_dir.join("index.json"),
        serde_json::to_vec_pretty(&index).unwrap(),
    )
    .unwrap();

    (tmp, bundle_dir)
}

// ============================================================================
// Helper: Generate trace test data
// ============================================================================

fn make_trace_meta() -> TraceMeta {
    TraceMeta {
        document_id: "DOC-001".to_string(),
        revision: "1.0".to_string(),
    }
}

fn make_schema() -> Schema {
    Schema {
        version: evidence::schema_versions::TRACE.to_string(),
    }
}

// ============================================================================
// TEST 1: Bundle Roundtrip
// ============================================================================

#[test]
fn test_bundle_roundtrip() {
    let (_tmp, bundle_dir) = create_minimal_bundle("dev");

    // Verify the bundle we just created
    let result = verify_bundle(&bundle_dir).expect("verify_bundle should not error");
    assert!(
        result.is_pass(),
        "Expected VerifyResult::Pass, got {:?}",
        result
    );
}

// ============================================================================
// TEST 2: Determinism
// ============================================================================

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

    // Compare SHA256SUMS content byte-for-byte
    let sums_a = fs::read(bundle_a.join("SHA256SUMS")).unwrap();
    let sums_b = fs::read(bundle_b.join("SHA256SUMS")).unwrap();
    assert_eq!(
        sums_a, sums_b,
        "SHA256SUMS must be byte-identical for identical content"
    );
}

// ============================================================================
// TEST 3: Tampering Detection
// ============================================================================

#[test]
fn test_tampering_detection() {
    let (_tmp, bundle_dir) = create_minimal_bundle("dev");

    // Verify passes first
    let result = verify_bundle(&bundle_dir).unwrap();
    assert!(result.is_pass(), "Bundle should pass before tampering");

    // Tamper with a file inside the bundle
    let env_path = bundle_dir.join("env.json");
    let mut content = fs::read_to_string(&env_path).unwrap();
    content.push_str("\n/* tampered */");
    fs::write(&env_path, content).unwrap();

    // Verification must now return Fail (not Pass)
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

// ============================================================================
// TEST 4: Content Hash Integrity
// ============================================================================

#[test]
fn test_content_hash_integrity() {
    let (_tmp, bundle_dir) = create_minimal_bundle("dev");

    // Read index.json and extract content_hash
    let index_content = fs::read_to_string(bundle_dir.join("index.json")).unwrap();
    let index: EvidenceIndex = serde_json::from_str(&index_content).unwrap();

    // Compute SHA256(SHA256SUMS) directly
    let sha256sums_hash = sha256_file(&bundle_dir.join("SHA256SUMS")).unwrap();

    assert_eq!(
        index.content_hash, sha256sums_hash,
        "content_hash in index.json must equal SHA256(SHA256SUMS)"
    );
}

// ============================================================================
// TEST 5: Cert Mode Strict Errors
// ============================================================================

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
    // Without strict mode, a failing provider should fall back to "unknown"
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

// ============================================================================
// TEST 6: Overwrite Protection
// ============================================================================

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
        dal_map: std::collections::BTreeMap::new(),
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

// ============================================================================
// TEST 7: Profile in Directory Name
// ============================================================================

#[test]
fn test_profile_in_directory_name() {
    // Create bundles with different profiles and verify the directory name
    // starts with the profile name.
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
// TEST 8: Path Normalization
// ============================================================================

#[test]
fn test_path_normalization_forward_slashes() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create nested files to test path handling
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("sub").join("file.txt"), b"test content").unwrap();
    fs::write(root.join("top.txt"), b"top level").unwrap();

    // Write SHA256SUMS
    let sha256sums_path = root.join("SHA256SUMS");
    write_sha256sums(root, &sha256sums_path).unwrap();

    // Read SHA256SUMS and verify all paths use forward slashes
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

    // Also verify hash_file_relative_into normalizes paths
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

// ============================================================================
// TEST 9: Traceability Bidirectional
// ============================================================================

#[test]
fn test_traceability_bidirectional_matrix() {
    let hlr_uid = "550e8400-e29b-41d4-a716-446655440001";
    let llr_uid = "550e8400-e29b-41d4-a716-446655440002";
    let test_uid = "550e8400-e29b-41d4-a716-446655440003";

    let hlr_file = HlrFile {
        schema: make_schema(),
        meta: make_trace_meta(),
        requirements: vec![HlrEntry {
            uid: Some(hlr_uid.to_string()),
            ns: None,
            id: "HLR-001".to_string(),
            title: "System shall do X".to_string(),
            owner: Some("nav-kernel".to_string()),
            scope: None,
            sort_key: Some(1),
            category: None,
            source: None,
            description: None,
            rationale: None,
            verification_methods: vec!["test".to_string()],
        }],
    };

    let llr_file = LlrFile {
        schema: make_schema(),
        meta: make_trace_meta(),
        requirements: vec![LlrEntry {
            uid: Some(llr_uid.to_string()),
            ns: None,
            id: "LLR-001".to_string(),
            title: "Module shall implement X".to_string(),
            owner: Some("nav-kernel".to_string()),
            sort_key: Some(1),
            traces_to: vec![hlr_uid.to_string()],
            source: None,
            modules: vec![],
            derived: false,
            description: None,
            rationale: None,
            verification_methods: vec!["test".to_string()],
        }],
    };

    let tests_file = TestsFile {
        schema: make_schema(),
        meta: make_trace_meta(),
        tests: vec![TestEntry {
            uid: Some(test_uid.to_string()),
            ns: None,
            id: "TEST-001".to_string(),
            title: "Test that X works".to_string(),
            owner: Some("nav-kernel".to_string()),
            sort_key: Some(1),
            traces_to: vec![llr_uid.to_string()],
            description: None,
            category: None,
            test_selector: None,
            source: None,
        }],
    };

    // Validate trace links first
    validate_trace_links(
        &hlr_file.requirements,
        &llr_file.requirements,
        &tests_file.tests,
    )
    .expect("trace links should validate successfully");

    // Generate traceability matrix
    let matrix =
        generate_traceability_matrix(&hlr_file, &llr_file, &tests_file, "DOC-001").unwrap();

    // Verify forward trace: HLR -> LLR
    assert!(
        matrix.contains("HLR to LLR Traceability"),
        "Matrix should contain HLR to LLR table"
    );
    assert!(
        matrix.contains("LLR-001"),
        "HLR->LLR table should show LLR-001"
    );

    // Verify forward trace: LLR -> Test
    assert!(
        matrix.contains("LLR to Test Traceability"),
        "Matrix should contain LLR to Test table"
    );
    assert!(
        matrix.contains("TEST-001"),
        "LLR->Test table should show TEST-001"
    );

    // Verify REVERSE trace table exists: Test -> LLR -> HLR
    assert!(
        matrix.contains("Reverse Trace: Test to LLR to HLR"),
        "Matrix must contain reverse trace table"
    );

    // Verify HLR -> Test roll-up exists
    assert!(
        matrix.contains("End-to-End: HLR to Test Roll-Up"),
        "Matrix must contain HLR to Test roll-up table"
    );
}

// ============================================================================
// TEST 10: Orphan Test Detection
// ============================================================================

#[test]
fn test_orphan_test_detection() {
    let hlr_uid = "550e8400-e29b-41d4-a716-446655440001";
    let llr_uid = "550e8400-e29b-41d4-a716-446655440002";
    let test_uid_linked = "550e8400-e29b-41d4-a716-446655440003";
    let test_uid_orphan = "550e8400-e29b-41d4-a716-446655440004";

    let hlrs = vec![HlrEntry {
        uid: Some(hlr_uid.to_string()),
        ns: None,
        id: "HLR-001".to_string(),
        title: "System requirement".to_string(),
        owner: Some("nav-kernel".to_string()),
        scope: None,
        sort_key: Some(1),
        category: None,
        source: None,
        description: None,
        rationale: None,
        verification_methods: vec!["test".to_string()],
    }];

    let llrs = vec![LlrEntry {
        uid: Some(llr_uid.to_string()),
        ns: None,
        id: "LLR-001".to_string(),
        title: "Implementation requirement".to_string(),
        owner: Some("nav-kernel".to_string()),
        sort_key: Some(1),
        traces_to: vec![hlr_uid.to_string()],
        source: None,
        modules: vec![],
        derived: false,
        description: None,
        rationale: None,
        verification_methods: vec!["test".to_string()],
    }];

    let tests = vec![
        TestEntry {
            uid: Some(test_uid_linked.to_string()),
            ns: None,
            id: "TEST-001".to_string(),
            title: "Linked test".to_string(),
            owner: Some("nav-kernel".to_string()),
            sort_key: Some(1),
            traces_to: vec![llr_uid.to_string()],
            description: None,
            category: None,
            test_selector: None,
            source: None,
        },
        TestEntry {
            uid: Some(test_uid_orphan.to_string()),
            ns: None,
            id: "TEST-ORPHAN".to_string(),
            title: "Orphan test with no LLR link".to_string(),
            owner: Some("nav-kernel".to_string()),
            sort_key: Some(2),
            traces_to: vec![],
            description: None,
            category: None,
            test_selector: None,
            source: None,
        },
    ];

    // validate_trace_links should still succeed (orphan tests produce warnings,
    // not hard errors), but the orphan IS detected and reported.
    // The current implementation prints warnings to stderr for orphan tests
    // but does not return an error.
    let result = validate_trace_links(&hlrs, &llrs, &tests);
    // Note: validate_trace_links should succeed because orphans are warnings
    assert!(
        result.is_ok(),
        "Orphan tests should produce warnings, not errors: {:?}",
        result.err()
    );

    // Also verify orphan detection in the traceability matrix output
    let hlr_file = HlrFile {
        schema: make_schema(),
        meta: make_trace_meta(),
        requirements: hlrs,
    };
    let llr_file = LlrFile {
        schema: make_schema(),
        meta: make_trace_meta(),
        requirements: llrs,
    };
    let tests_file = TestsFile {
        schema: make_schema(),
        meta: make_trace_meta(),
        tests,
    };

    let matrix =
        generate_traceability_matrix(&hlr_file, &llr_file, &tests_file, "DOC-001").unwrap();

    // The matrix should report orphan tests in the coverage summary
    assert!(
        matrix.contains("Orphan tests (no LLR link)"),
        "Matrix should report orphan test count"
    );
    assert!(
        matrix.contains("Orphan Tests (no LLR link)"),
        "Matrix should have orphan tests section in gaps"
    );
    assert!(
        matrix.contains("TEST-ORPHAN"),
        "Matrix should list the orphan test by ID"
    );
}

// ============================================================================
// Additional edge case: index.json excluded from SHA256SUMS
// ============================================================================

#[test]
fn test_index_json_excluded_from_sha256sums() {
    let (_tmp, bundle_dir) = create_minimal_bundle("dev");

    let sha256sums_content = fs::read_to_string(bundle_dir.join("SHA256SUMS")).unwrap();

    // index.json must NOT appear in SHA256SUMS (metadata layer separation)
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

// ============================================================================
// Additional: GitSnapshot::capture_with with mock provider
// ============================================================================

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

// ============================================================================
// Additional: SHA256 hash function correctness
// ============================================================================

#[test]
fn test_sha256_known_value() {
    // Verify SHA-256 against a known test vector
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

// ============================================================================
// TEST: TOCTOU detection — git HEAD change between new() and finalize()
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
            // Initial snapshot
            Ok("aabbccdd11223344aabbccdd11223344aabbccdd".to_string())
        } else {
            // Changed HEAD at finalize time
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
        dal_map: std::collections::BTreeMap::new(),
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

    // finalize should detect the changed SHA and bail
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

// ============================================================================
// TEST: Path traversal detection in verify
// ============================================================================

#[test]
fn test_verify_rejects_path_traversal_in_sha256sums() {
    let (_tmp, bundle_dir) = create_minimal_bundle("dev");

    // Tamper SHA256SUMS to include a path-traversal entry
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
    let (_tmp, bundle_dir) = create_minimal_bundle("dev");

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

// ============================================================================
// TEST: index.json field format validation
// ============================================================================

#[test]
fn test_verify_rejects_invalid_profile() {
    let (_tmp, bundle_dir) = create_minimal_bundle("dev");

    // Tamper index.json to have an invalid profile
    let index_path = bundle_dir.join("index.json");
    let content = fs::read_to_string(&index_path).unwrap();
    let tampered = content.replace("\"dev\"", "\"yolo\"");
    fs::write(&index_path, &tampered).unwrap();

    // Also tamper env.json so the cross-file check doesn't confuse matters
    let env_path = bundle_dir.join("env.json");
    let env_content = fs::read_to_string(&env_path).unwrap();
    let env_tampered = env_content.replace("\"dev\"", "\"yolo\"");
    fs::write(&env_path, &env_tampered).unwrap();

    let result = verify_bundle(&bundle_dir).unwrap();
    assert!(result.is_fail(), "Should fail with invalid profile");
    assert!(
        result.summary().contains("profile"),
        "Should mention profile, got: {}",
        result.summary()
    );
}

#[test]
fn test_verify_rejects_bad_git_sha_in_cert_profile() {
    let (_tmp, bundle_dir) = create_minimal_bundle("cert");

    // Tamper index.json git_sha to be invalid
    let index_path = bundle_dir.join("index.json");
    let content = fs::read_to_string(&index_path).unwrap();
    let tampered = content.replace(
        "aabbccdd11223344aabbccdd11223344aabbccdd",
        "not-a-valid-sha",
    );
    fs::write(&index_path, &tampered).unwrap();

    // Also tamper env.json to match so cross-file doesn't trigger
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
    // Dev profile allows "unknown" git_sha — should not trigger FormatError
    let (_tmp, bundle_dir) = create_minimal_bundle("dev");

    // Set git_sha to "unknown" in both files
    let index_path = bundle_dir.join("index.json");
    let content = fs::read_to_string(&index_path).unwrap();
    let tampered = content.replace("aabbccdd11223344aabbccdd11223344aabbccdd", "unknown");
    fs::write(&index_path, &tampered).unwrap();

    let env_path = bundle_dir.join("env.json");
    let env_content = fs::read_to_string(&env_path).unwrap();
    let env_tampered = env_content.replace("aabbccdd11223344aabbccdd11223344aabbccdd", "unknown");
    fs::write(&env_path, &env_tampered).unwrap();

    // Should fail only on content_hash mismatch (tampered file), not git_sha format
    let result = verify_bundle(&bundle_dir).unwrap();
    let summary = result.summary();
    assert!(
        !summary.contains("git_sha"),
        "Dev profile should not flag git_sha='unknown', got: {}",
        summary
    );
}

// ============================================================================
// TEST: engine_build_source cross-check vs engine_git_sha
// ============================================================================

/// Rewrite `index.json` in `bundle_dir` with the given substitution.
/// `index.json` is excluded from SHA256SUMS, so edits don't rotate
/// `content_hash` and the bundle still verifies as long as the
/// envelope stays schema-valid.
fn replace_in_index(bundle_dir: &std::path::Path, from: &str, to: &str) {
    let index_path = bundle_dir.join("index.json");
    let content = fs::read_to_string(&index_path).unwrap();
    assert!(
        content.contains(from),
        "replace_in_index: '{from}' not found in index.json",
    );
    fs::write(&index_path, content.replace(from, to)).unwrap();
}

#[test]
fn test_verify_rejects_git_source_with_nonhex_sha() {
    // source="git" but engine_git_sha is a release-style string —
    // exactly the drift we want the verifier to catch.
    let (_tmp, bundle_dir) = create_minimal_bundle("dev");
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
    let (_tmp, bundle_dir) = create_minimal_bundle("dev");
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
    let (_tmp, bundle_dir) = create_minimal_bundle("cert");
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
    let (_tmp, bundle_dir) = create_minimal_bundle("cert");
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
// TEST: Cross-file consistency env.json vs index.json
// ============================================================================

#[test]
fn test_verify_detects_env_index_profile_mismatch() {
    let (_tmp, bundle_dir) = create_minimal_bundle("dev");

    // Tamper env.json profile to differ from index.json
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
    let (_tmp, bundle_dir) = create_minimal_bundle("dev");

    // Tamper env.json git_sha to differ from index.json
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
