//! Evidence bundle verification.
//!
//! This module provides functionality for verifying evidence bundles
//! including hash verification, completeness checks, and schema validation.

use anyhow::{Context, Result, bail};
use log;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::bundle::EvidenceIndex;
use crate::hash::sha256_file;
use chrono;

// ============================================================================
// Verification Result
// ============================================================================

/// Structured verification error codes for programmatic handling.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "code", content = "detail")]
pub enum VerifyError {
    /// A file exists in the bundle but is not listed in SHA256SUMS
    UnexpectedFile(String),
    /// HMAC signature verification failed
    HmacFailure,
    /// A hash in SHA256SUMS does not match the actual file hash
    HashMismatch {
        file: String,
        expected: String,
        actual: String,
    },
    /// A file listed in SHA256SUMS is missing from the bundle
    MissingHashedFile(String),
    /// content_hash in index.json doesn't match SHA256SUMS hash
    ContentHashMismatch {
        index_hash: String,
        actual_hash: String,
    },
    /// A path in SHA256SUMS or trace_outputs contains path traversal
    /// (`..`, absolute path, or Windows drive prefix)
    UnsafePath(String),
    /// A field in index.json has an invalid format
    FormatError {
        field: String,
        expected: String,
        actual: String,
    },
    /// A field in env.json is inconsistent with the same field in index.json
    CrossFileInconsistency {
        field: String,
        index_value: String,
        env_value: String,
    },
    /// `deterministic_hash` in `index.json` does not match the actual
    /// SHA-256 of `deterministic-manifest.json`.
    DeterministicHashMismatch {
        index_hash: String,
        actual_hash: String,
    },
    /// Re-projecting `env.json`'s `DeterministicManifest` subset
    /// does not byte-equal the committed `deterministic-manifest.json`.
    /// Indicates tampering or a CLI bug that let the two drift apart
    /// at generation time.
    ManifestProjectionDrift { detail: String },
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerifyError::UnexpectedFile(file) => write!(f, "unexpected file: {}", file),
            VerifyError::HmacFailure => write!(f, "HMAC signature verification failed"),
            VerifyError::HashMismatch {
                file,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "hash mismatch for {}: expected {}, got {}",
                    file, expected, actual
                )
            }
            VerifyError::MissingHashedFile(file) => {
                write!(f, "file in SHA256SUMS not found: {}", file)
            }
            VerifyError::ContentHashMismatch {
                index_hash,
                actual_hash,
            } => {
                write!(
                    f,
                    "content_hash mismatch: index={}, actual={}",
                    index_hash, actual_hash
                )
            }
            VerifyError::UnsafePath(path) => {
                write!(f, "unsafe path in bundle: {}", path)
            }
            VerifyError::FormatError {
                field,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "invalid format for {}: expected {}, got {}",
                    field, expected, actual
                )
            }
            VerifyError::CrossFileInconsistency {
                field,
                index_value,
                env_value,
            } => {
                write!(
                    f,
                    "env.json vs index.json mismatch for {}: index={}, env={}",
                    field, index_value, env_value
                )
            }
            VerifyError::DeterministicHashMismatch {
                index_hash,
                actual_hash,
            } => {
                write!(
                    f,
                    "deterministic_hash mismatch: index={}, actual={}",
                    index_hash, actual_hash
                )
            }
            VerifyError::ManifestProjectionDrift { detail } => {
                write!(
                    f,
                    "deterministic-manifest.json is not a valid projection of env.json: {}",
                    detail
                )
            }
        }
    }
}

/// Result of a verification operation.
#[derive(Debug, Clone)]
pub enum VerifyResult {
    /// Verification passed
    Pass,
    /// Verification failed with structured error(s)
    Fail(Vec<VerifyError>),
    /// Verification skipped with a reason
    Skipped(String),
}

impl VerifyResult {
    /// Check if verification passed.
    pub fn is_pass(&self) -> bool {
        matches!(self, VerifyResult::Pass)
    }

    /// Check if verification failed.
    pub fn is_fail(&self) -> bool {
        matches!(self, VerifyResult::Fail(_))
    }

    /// Human-readable summary of any failure reasons.
    pub fn summary(&self) -> String {
        match self {
            VerifyResult::Pass => "PASS".to_string(),
            VerifyResult::Fail(errors) => errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; "),
            VerifyResult::Skipped(reason) => format!("SKIPPED: {}", reason),
        }
    }
}

// ============================================================================
// Bundle Verification
// ============================================================================

/// Check whether a relative path from SHA256SUMS or trace_outputs is safe
/// to join onto the bundle root.
///
/// Rejects absolute paths, `..` components, and Windows drive prefixes.
/// This prevents a crafted SHA256SUMS from causing the verifier to read
/// files outside the bundle directory.
fn is_safe_bundle_path(filename: &str) -> bool {
    use std::path::{Component, Path};
    let path = Path::new(filename);
    if path.is_absolute() {
        return false;
    }
    for component in path.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
            _ => {}
        }
    }
    // Also reject leading backslash (Windows UNC without drive letter)
    !filename.starts_with('\\')
}

/// Valid profile values for index.json.
const VALID_PROFILES: &[&str] = &["dev", "cert", "record"];

/// Required files that must exist in every evidence bundle.
pub const REQUIRED_FILES: &[&str] = &[
    "index.json",
    "inputs_hashes.json",
    "outputs_hashes.json",
    "commands.json",
    "env.json",
    "deterministic-manifest.json",
    "SHA256SUMS",
];

/// Files that are allowed in a bundle but not listed in SHA256SUMS.
const KNOWN_META_FILES: &[&str] = &["index.json", "SHA256SUMS", "BUNDLE.sig"];

/// Verify an evidence bundle at the given path.
///
/// Performs the following checks:
/// 1. Bundle directory exists
/// 2. All required files exist
/// 3. index.json is valid and bundle_complete is true
/// 4. All trace outputs referenced in index exist
/// 5. All SHA256SUMS entries match actual file hashes
/// 6. content_hash matches actual SHA256SUMS hash
/// 7. No unexpected files exist outside SHA256SUMS and known metadata
/// 8. If `verify_key` is provided, verify BUNDLE.sig HMAC
pub fn verify_bundle(bundle: &Path) -> Result<VerifyResult> {
    verify_bundle_with_key(bundle, None)
}

/// Cross-check `engine_build_source` against the shape of
/// `engine_git_sha`, and reject combinations that a qualified bundle
/// cannot honestly carry.
fn check_engine_source(source: &str, sha: &str, profile: &str, errors: &mut Vec<VerifyError>) {
    match source {
        "git" => {
            if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
                errors.push(VerifyError::FormatError {
                    field: "engine_git_sha".to_string(),
                    expected: "40-character hex SHA when engine_build_source=\"git\"".to_string(),
                    actual: sha.to_string(),
                });
            }
        }
        "release" => {
            // Expect "release-v<semver>" shape. Permissive on the
            // trailer so pre-release / build metadata (v0.1.0-rc.1+sha)
            // passes, but the `release-v` prefix is load-bearing.
            let ok = sha.starts_with("release-v")
                && sha.len() > "release-v".len()
                && sha.as_bytes()["release-v".len()..]
                    .iter()
                    .next()
                    .is_some_and(|b| b.is_ascii_digit());
            if !ok {
                errors.push(VerifyError::FormatError {
                    field: "engine_git_sha".to_string(),
                    expected: "release-v<version> string when engine_build_source=\"release\""
                        .to_string(),
                    actual: sha.to_string(),
                });
            }
            // cert/record bundles must be pinned to a commit, not a
            // release tag; reject "release" for those profiles.
            if profile != "dev" {
                errors.push(VerifyError::FormatError {
                    field: "engine_build_source".to_string(),
                    expected: "\"git\" (required for cert/record profiles)".to_string(),
                    actual: source.to_string(),
                });
            }
        }
        "unknown" => {
            // Pre-0.0.2 bundle or one whose writer skipped the field.
            // Dev tolerates it for backward compatibility; cert/record
            // cannot accept a bundle whose engine provenance is
            // unlabeled.
            if profile != "dev" {
                errors.push(VerifyError::FormatError {
                    field: "engine_build_source".to_string(),
                    expected: "\"git\" (cert/record cannot accept unlabeled engine provenance)"
                        .to_string(),
                    actual: source.to_string(),
                });
            }
        }
        other => {
            errors.push(VerifyError::FormatError {
                field: "engine_build_source".to_string(),
                expected: "one of: git, release, unknown".to_string(),
                actual: other.to_string(),
            });
        }
    }
}

/// Verify an evidence bundle, optionally checking HMAC signature.
pub fn verify_bundle_with_key(bundle: &Path, verify_key: Option<&[u8]>) -> Result<VerifyResult> {
    log::info!("verify: checking bundle at {:?}", bundle);

    // Collect all verification errors rather than bailing on first failure.
    // bail!() is reserved for I/O errors and parse failures (runtime faults).
    // VerifyResult::Fail is used for all verification-level failures.
    let mut verify_errors: Vec<VerifyError> = Vec::new();

    // 1. Check bundle directory exists (runtime fault — bail)
    if !bundle.is_dir() {
        bail!(
            "Bundle path does not exist or is not a directory: {:?}",
            bundle
        );
    }

    // 2. Check required files exist
    for f in REQUIRED_FILES {
        if !bundle.join(f).exists() {
            verify_errors.push(VerifyError::MissingHashedFile(f.to_string()));
        }
    }
    if !verify_errors.is_empty() {
        return Ok(VerifyResult::Fail(verify_errors));
    }

    // 3. Load and validate index.json structure (parse failure — bail)
    let index_path = bundle.join("index.json");
    let index_content =
        fs::read_to_string(&index_path).with_context(|| format!("Reading {:?}", index_path))?;
    let index: EvidenceIndex =
        serde_json::from_str(&index_content).with_context(|| "Parsing index.json")?;

    if !index.bundle_complete {
        verify_errors.push(VerifyError::ContentHashMismatch {
            index_hash: "bundle_complete=false".to_string(),
            actual_hash: "bundle_complete=true required".to_string(),
        });
        return Ok(VerifyResult::Fail(verify_errors));
    }

    // 3b. Validate index.json field formats (semantic checks beyond serde)
    if !VALID_PROFILES.contains(&index.profile.as_str()) {
        verify_errors.push(VerifyError::FormatError {
            field: "profile".to_string(),
            expected: "one of: dev, cert, record".to_string(),
            actual: index.profile.clone(),
        });
    }
    // git_sha must be 40-char hex for cert/record profiles; dev allows "unknown"
    if index.profile != "dev"
        && (index.git_sha.len() != 40 || !index.git_sha.chars().all(|c| c.is_ascii_hexdigit()))
    {
        verify_errors.push(VerifyError::FormatError {
            field: "git_sha".to_string(),
            expected: "40-character hex (required for cert/record profiles)".to_string(),
            actual: index.git_sha.clone(),
        });
    }
    // engine_build_source must be a known label; engine_git_sha must be
    // shaped consistently with it. cert/record profiles cannot ship
    // "release" or "unknown" — a qualified bundle must be traceable
    // to a specific engine commit, not a release-fallback string or a
    // legacy bundle that predates the source field.
    check_engine_source(
        &index.engine_build_source,
        &index.engine_git_sha,
        &index.profile,
        &mut verify_errors,
    );
    // timestamp_rfc3339 must be a valid RFC3339 datetime
    if chrono::DateTime::parse_from_rfc3339(&index.timestamp_rfc3339).is_err() {
        verify_errors.push(VerifyError::FormatError {
            field: "timestamp_rfc3339".to_string(),
            expected: "valid RFC3339 datetime".to_string(),
            actual: index.timestamp_rfc3339.clone(),
        });
    }
    // content_hash must be 64-char lowercase hex (SHA-256)
    if index.content_hash.len() != 64
        || !index
            .content_hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        verify_errors.push(VerifyError::FormatError {
            field: "content_hash".to_string(),
            expected: "64-character lowercase hex (SHA-256)".to_string(),
            actual: index.content_hash.clone(),
        });
    }
    // deterministic_hash must also be 64-char lowercase hex
    if index.deterministic_hash.len() != 64
        || !index
            .deterministic_hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        verify_errors.push(VerifyError::FormatError {
            field: "deterministic_hash".to_string(),
            expected: "64-character lowercase hex (SHA-256)".to_string(),
            actual: index.deterministic_hash.clone(),
        });
    }

    // 3c. Cross-file consistency: env.json vs index.json
    // If env.json is unreadable or unparseable, skip this check —
    // the SHA256SUMS hash check (step 5) will catch corruption anyway.
    {
        let env_path = bundle.join("env.json");
        if let Ok(env_content) = fs::read_to_string(&env_path) {
            if let Ok(env_value) = serde_json::from_str::<serde_json::Value>(&env_content) {
                if let Some(env_profile) = env_value.get("profile").and_then(|v| v.as_str()) {
                    if env_profile != index.profile {
                        verify_errors.push(VerifyError::CrossFileInconsistency {
                            field: "profile".to_string(),
                            index_value: index.profile.clone(),
                            env_value: env_profile.to_string(),
                        });
                    }
                }
                if let Some(env_git_sha) = env_value.get("git_sha").and_then(|v| v.as_str()) {
                    if env_git_sha != index.git_sha {
                        verify_errors.push(VerifyError::CrossFileInconsistency {
                            field: "git_sha".to_string(),
                            index_value: index.git_sha.clone(),
                            env_value: env_git_sha.to_string(),
                        });
                    }
                }
                if let Some(env_branch) = env_value.get("git_branch").and_then(|v| v.as_str()) {
                    if env_branch != index.git_branch {
                        verify_errors.push(VerifyError::CrossFileInconsistency {
                            field: "git_branch".to_string(),
                            index_value: index.git_branch.clone(),
                            env_value: env_branch.to_string(),
                        });
                    }
                }
                if let Some(env_dirty) = env_value.get("git_dirty").and_then(|v| v.as_bool()) {
                    if env_dirty != index.git_dirty {
                        verify_errors.push(VerifyError::CrossFileInconsistency {
                            field: "git_dirty".to_string(),
                            index_value: index.git_dirty.to_string(),
                            env_value: env_dirty.to_string(),
                        });
                    }
                }
            }
        }
    }

    // 4. Verify trace outputs exist (with path safety)
    for trace_out in &index.trace_outputs {
        if !is_safe_bundle_path(trace_out) {
            verify_errors.push(VerifyError::UnsafePath(trace_out.clone()));
            continue;
        }
        let trace_path = bundle.join(trace_out);
        if !trace_path.exists() {
            verify_errors.push(VerifyError::MissingHashedFile(format!(
                "trace output: {}",
                trace_out
            )));
        }
    }

    // 5. Verify SHA256SUMS integrity
    let sha256sums_path = bundle.join("SHA256SUMS");
    let sha256sums_content = fs::read_to_string(&sha256sums_path)?;
    let mut listed_files: BTreeSet<String> = BTreeSet::new();

    for line in sha256sums_content.lines() {
        if line.is_empty() {
            continue;
        }
        // Format: <hash>  <filename> (two spaces)
        let parts: Vec<&str> = line.splitn(2, "  ").collect();
        if parts.len() != 2 {
            verify_errors.push(VerifyError::HashMismatch {
                file: "(malformed line)".to_string(),
                expected: "".to_string(),
                actual: format!("malformed SHA256SUMS line: {}", line),
            });
            continue;
        }
        let expected_hash = parts[0];
        let filename = parts[1];

        // Validate hash format: must be 64-char lowercase hex (SHA-256)
        if expected_hash.len() != 64 || !expected_hash.chars().all(|c| c.is_ascii_hexdigit()) {
            verify_errors.push(VerifyError::HashMismatch {
                file: filename.to_string(),
                expected: format!("64-char hex, got {} chars", expected_hash.len()),
                actual: expected_hash.to_string(),
            });
            continue;
        }

        // Path safety: reject traversal, absolute, or drive-prefixed paths
        // BEFORE any bundle.join() to prevent reads outside the bundle.
        if !is_safe_bundle_path(filename) {
            verify_errors.push(VerifyError::UnsafePath(filename.to_string()));
            continue;
        }

        listed_files.insert(filename.to_string());
        let file_path = bundle.join(filename);

        if !file_path.exists() {
            verify_errors.push(VerifyError::MissingHashedFile(filename.to_string()));
            continue;
        }

        let actual_hash = sha256_file(&file_path)?;
        if actual_hash != expected_hash {
            verify_errors.push(VerifyError::HashMismatch {
                file: filename.to_string(),
                expected: expected_hash.to_string(),
                actual: actual_hash,
            });
        }
    }

    // index.json is in the metadata layer and excluded from SHA256SUMS.
    // Verify that it is NOT listed in SHA256SUMS (design invariant).
    if listed_files.contains("index.json") {
        verify_errors.push(VerifyError::UnexpectedFile(
            "index.json must NOT be listed in SHA256SUMS (metadata layer violation)".to_string(),
        ));
    }

    // 6. Verify content_hash matches actual SHA256SUMS hash
    let actual_content_hash = sha256_file(&sha256sums_path)?;
    if index.content_hash != actual_content_hash {
        verify_errors.push(VerifyError::ContentHashMismatch {
            index_hash: index.content_hash.clone(),
            actual_hash: actual_content_hash,
        });
    }

    // 6a. Verify deterministic_hash matches actual SHA-256 of
    //     deterministic-manifest.json.
    //
    //     The manifest file is already covered by SHA256SUMS (so the
    //     content_hash check above would catch a tampered manifest
    //     against its SHA256SUMS entry). This additional check
    //     protects against a malformed `index.json` whose
    //     deterministic_hash has been changed to claim a different
    //     cross-host identity than the committed manifest actually
    //     produces.
    let manifest_path = bundle.join("deterministic-manifest.json");
    if manifest_path.exists() {
        let actual_manifest_hash = sha256_file(&manifest_path)?;
        if index.deterministic_hash != actual_manifest_hash {
            verify_errors.push(VerifyError::DeterministicHashMismatch {
                index_hash: index.deterministic_hash.clone(),
                actual_hash: actual_manifest_hash,
            });
        }

        // 6b. Verify the manifest is a faithful projection of env.json.
        //
        //     Without this check, a tamperer could produce a
        //     well-formed bundle where `deterministic-manifest.json`
        //     and `index.json.deterministic_hash` agree with each
        //     other but disagree with `env.json`. The projection
        //     check closes that gap: we deserialize env.json into
        //     `EnvFingerprint`, run `.deterministic_manifest()`,
        //     serialize with the same writer the bundler used, and
        //     compare bytes.
        let env_path = bundle.join("env.json");
        if env_path.exists() {
            match (fs::read(&env_path), fs::read(&manifest_path)) {
                (Ok(env_bytes), Ok(manifest_bytes)) => {
                    match serde_json::from_slice::<crate::env::EnvFingerprint>(&env_bytes) {
                        Ok(env_fp) => {
                            match serde_json::to_vec_pretty(&env_fp.deterministic_manifest()) {
                                Ok(reprojected) => {
                                    if reprojected != manifest_bytes {
                                        verify_errors.push(VerifyError::ManifestProjectionDrift {
                                            detail: format!(
                                                "manifest {} bytes, reprojection {} bytes",
                                                manifest_bytes.len(),
                                                reprojected.len()
                                            ),
                                        });
                                    }
                                }
                                Err(e) => {
                                    verify_errors.push(VerifyError::ManifestProjectionDrift {
                                        detail: format!("serialize reprojection: {}", e),
                                    });
                                }
                            }
                        }
                        Err(e) => {
                            verify_errors.push(VerifyError::ManifestProjectionDrift {
                                detail: format!("parse env.json: {}", e),
                            });
                        }
                    }
                }
                _ => {
                    // Covered by the REQUIRED_FILES check above —
                    // one or both files missing already produced a
                    // MissingHashedFile error. Nothing to do here.
                }
            }
        }
    }

    // 7. Extra-file detection: walk all files in bundle and flag unexpected ones
    for entry in walkdir::WalkDir::new(bundle) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(bundle)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");

        if listed_files.contains(&rel) {
            continue;
        }
        if KNOWN_META_FILES.contains(&rel.as_str()) {
            continue;
        }
        verify_errors.push(VerifyError::UnexpectedFile(rel));
    }

    // 8. HMAC signature verification (if key provided)
    let sig_path = bundle.join("BUNDLE.sig");
    if let Some(key) = verify_key {
        if !sig_path.exists() {
            verify_errors.push(VerifyError::HmacFailure);
        } else {
            let valid = crate::bundle::verify_bundle_signature(bundle, key)?;
            if !valid {
                verify_errors.push(VerifyError::HmacFailure);
            } else {
                log::info!("verify: HMAC signature OK");
            }
        }
    } else if sig_path.exists() {
        log::info!("verify: BUNDLE.sig present but no --verify-key provided, skipping HMAC check");
    }

    // Return collected errors or pass
    if !verify_errors.is_empty() {
        for e in &verify_errors {
            log::error!("  VERIFY ERROR: {}", e);
        }
        return Ok(VerifyResult::Fail(verify_errors));
    }

    log::info!("verify: OK");
    log::info!("  profile: {}", index.profile);
    log::info!(
        "  git_sha: {}",
        index.git_sha.get(..8).unwrap_or(&index.git_sha)
    );
    log::info!("  timestamp: {}", index.timestamp_rfc3339);
    log::info!(
        "  content_hash: {}",
        index.content_hash.get(..16).unwrap_or(&index.content_hash)
    );
    log::info!("  trace_outputs: {}", index.trace_outputs.len());

    Ok(VerifyResult::Pass)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_result_methods() {
        assert!(VerifyResult::Pass.is_pass());
        assert!(!VerifyResult::Pass.is_fail());

        assert!(VerifyResult::Fail(vec![VerifyError::HmacFailure]).is_fail());
        assert!(!VerifyResult::Fail(vec![VerifyError::HmacFailure]).is_pass());

        assert!(!VerifyResult::Skipped("reason".to_string()).is_pass());
        assert!(!VerifyResult::Skipped("reason".to_string()).is_fail());
    }

    #[test]
    fn test_required_files_list() {
        assert!(REQUIRED_FILES.contains(&"index.json"));
        assert!(REQUIRED_FILES.contains(&"SHA256SUMS"));
        assert!(REQUIRED_FILES.contains(&"deterministic-manifest.json"));
        assert_eq!(REQUIRED_FILES.len(), 7);
    }

    #[test]
    fn test_is_safe_bundle_path_valid() {
        assert!(is_safe_bundle_path("env.json"));
        assert!(is_safe_bundle_path("tests/cargo_test_stdout.txt"));
        assert!(is_safe_bundle_path("trace/matrix.md"));
        assert!(is_safe_bundle_path("sub/dir/file.txt"));
    }

    #[test]
    fn test_is_safe_bundle_path_rejects_traversal() {
        assert!(!is_safe_bundle_path("../../../etc/passwd"));
        assert!(!is_safe_bundle_path("sub/../../../etc/shadow"));
        assert!(!is_safe_bundle_path(".."));
    }

    #[test]
    fn test_is_safe_bundle_path_rejects_absolute() {
        assert!(!is_safe_bundle_path("/etc/passwd"));
        assert!(!is_safe_bundle_path("/tmp/file.txt"));
    }

    #[test]
    fn test_is_safe_bundle_path_rejects_backslash_prefix() {
        assert!(!is_safe_bundle_path("\\\\server\\share\\file"));
    }
}
