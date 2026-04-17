//! Evidence bundle verification.
//!
//! This module provides functionality for verifying evidence bundles
//! including hash verification, completeness checks, and schema validation.

use anyhow::{Context, Result, bail};
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

/// Verify an evidence bundle, optionally checking HMAC signature.
pub fn verify_bundle_with_key(bundle: &Path, verify_key: Option<&[u8]>) -> Result<VerifyResult> {
    tracing::info!("verify: checking bundle at {:?}", bundle);

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
                tracing::info!("verify: HMAC signature OK");
            }
        }
    } else if sig_path.exists() {
        tracing::info!(
            "verify: BUNDLE.sig present but no --verify-key provided, skipping HMAC check"
        );
    }

    // Return collected errors or pass
    if !verify_errors.is_empty() {
        for e in &verify_errors {
            tracing::error!("  VERIFY ERROR: {}", e);
        }
        return Ok(VerifyResult::Fail(verify_errors));
    }

    tracing::info!("verify: OK");
    tracing::info!("  profile: {}", index.profile);
    tracing::info!(
        "  git_sha: {}",
        index.git_sha.get(..8).unwrap_or(&index.git_sha)
    );
    tracing::info!("  timestamp: {}", index.timestamp_rfc3339);
    tracing::info!(
        "  content_hash: {}",
        index.content_hash.get(..16).unwrap_or(&index.content_hash)
    );
    tracing::info!("  trace_outputs: {}", index.trace_outputs.len());

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
        assert_eq!(REQUIRED_FILES.len(), 6);
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
