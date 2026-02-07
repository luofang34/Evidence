//! Evidence bundle verification.
//!
//! This module provides functionality for verifying evidence bundles
//! including hash verification, completeness checks, and schema validation.

use anyhow::{bail, Context, Result};
use log;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::bundle::EvidenceIndex;
use crate::hash::sha256_file;

// ============================================================================
// Verification Result
// ============================================================================

/// Result of a verification operation.
#[derive(Debug, Clone)]
pub enum VerifyResult {
    /// Verification passed
    Pass,
    /// Verification failed with a reason
    Fail(String),
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
}

// ============================================================================
// Bundle Verification
// ============================================================================

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
    log::info!("verify: checking bundle at {:?}", bundle);

    // 1. Check bundle directory exists
    if !bundle.is_dir() {
        bail!(
            "Bundle path does not exist or is not a directory: {:?}",
            bundle
        );
    }

    // 2. Check required files exist
    let mut missing = Vec::new();
    for f in REQUIRED_FILES {
        if !bundle.join(f).exists() {
            missing.push(*f);
        }
    }
    if !missing.is_empty() {
        bail!("Bundle incomplete. Missing files: {}", missing.join(", "));
    }

    // 3. Load and validate index.json structure
    let index_path = bundle.join("index.json");
    let index_content =
        fs::read_to_string(&index_path).with_context(|| format!("Reading {:?}", index_path))?;
    let index: EvidenceIndex =
        serde_json::from_str(&index_content).with_context(|| "Parsing index.json")?;

    if !index.bundle_complete {
        bail!("Bundle marked as incomplete (bundle_complete = false)");
    }

    // 4. Verify trace outputs exist
    for trace_out in &index.trace_outputs {
        let trace_path = bundle.join(trace_out);
        if !trace_path.exists() {
            bail!(
                "Missing trace output referenced in index: {}",
                trace_out
            );
        }
    }

    // 5. Verify SHA256SUMS integrity
    let sha256sums_path = bundle.join("SHA256SUMS");
    let sha256sums_content = fs::read_to_string(&sha256sums_path)?;
    let mut hash_errors = Vec::new();
    let mut listed_files: BTreeSet<String> = BTreeSet::new();

    for line in sha256sums_content.lines() {
        if line.is_empty() {
            continue;
        }
        // Format: <hash>  <filename> (two spaces)
        let parts: Vec<&str> = line.splitn(2, "  ").collect();
        if parts.len() != 2 {
            hash_errors.push(format!("Malformed SHA256SUMS line: {}", line));
            continue;
        }
        let expected_hash = parts[0];
        let filename = parts[1];
        listed_files.insert(filename.to_string());
        let file_path = bundle.join(filename);

        if !file_path.exists() {
            hash_errors.push(format!("File in SHA256SUMS not found: {}", filename));
            continue;
        }

        let actual_hash = sha256_file(&file_path)?;
        if actual_hash != expected_hash {
            hash_errors.push(format!(
                "Hash mismatch for {}: expected {}, got {}",
                filename, expected_hash, actual_hash
            ));
        }
    }

    // index.json is in the metadata layer and excluded from SHA256SUMS.
    // Verify that it is NOT listed in SHA256SUMS (design invariant).
    if listed_files.contains("index.json") {
        hash_errors.push(
            "index.json must NOT be listed in SHA256SUMS (metadata layer violation)"
                .to_string(),
        );
    }

    if !hash_errors.is_empty() {
        for e in &hash_errors {
            log::error!("  HASH ERROR: {}", e);
        }
        bail!(
            "SHA256 verification failed with {} errors",
            hash_errors.len()
        );
    }

    // 6. Verify content_hash matches actual SHA256SUMS hash
    let actual_content_hash = sha256_file(&sha256sums_path)?;
    if index.content_hash != actual_content_hash {
        bail!(
            "content_hash mismatch: index says {}, SHA256SUMS hashes to {}",
            index.content_hash,
            actual_content_hash
        );
    }

    // 7. Extra-file detection: walk all files in bundle and flag unexpected ones
    let mut unexpected_files = Vec::new();
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
        unexpected_files.push(rel);
    }

    if !unexpected_files.is_empty() {
        unexpected_files.sort();
        let list = unexpected_files.join(", ");
        return Ok(VerifyResult::Fail(format!(
            "Unexpected files not in SHA256SUMS: {}",
            list
        )));
    }

    // 8. HMAC signature verification (if key provided or BUNDLE.sig exists with key)
    let sig_path = bundle.join("BUNDLE.sig");
    if let Some(key) = verify_key {
        if !sig_path.exists() {
            bail!("--verify-key provided but BUNDLE.sig not found in bundle");
        }
        let valid = crate::bundle::verify_bundle_signature(bundle, key)?;
        if !valid {
            return Ok(VerifyResult::Fail(
                "BUNDLE.sig HMAC verification failed".to_string(),
            ));
        }
        log::info!("verify: HMAC signature OK");
    } else if sig_path.exists() {
        log::info!("verify: BUNDLE.sig present but no --verify-key provided, skipping HMAC check");
    }

    log::info!("verify: OK");
    log::info!("  profile: {}", index.profile);
    log::info!("  git_sha: {}", &index.git_sha[..8.min(index.git_sha.len())]);
    log::info!("  timestamp: {}", index.timestamp_rfc3339);
    log::info!("  content_hash: {}", &index.content_hash[..16.min(index.content_hash.len())]);
    log::info!("  trace_outputs: {}", index.trace_outputs.len());

    Ok(VerifyResult::Pass)
}

/// Verify only the SHA256SUMS file integrity.
///
/// This is a lighter-weight check that only verifies file hashes
/// without parsing the full index.
pub fn verify_sha256sums(bundle: &Path) -> Result<()> {
    let sha256sums_path = bundle.join("SHA256SUMS");
    if !sha256sums_path.exists() {
        bail!("SHA256SUMS file not found in bundle");
    }

    let content = fs::read_to_string(&sha256sums_path)?;
    let mut errors = Vec::new();

    for line in content.lines() {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, "  ").collect();
        if parts.len() != 2 {
            errors.push(format!("Malformed line: {}", line));
            continue;
        }

        let expected = parts[0];
        let filename = parts[1];
        let file_path = bundle.join(filename);

        if !file_path.exists() {
            errors.push(format!("Missing: {}", filename));
            continue;
        }

        let actual = sha256_file(&file_path)?;
        if actual != expected {
            errors.push(format!("Mismatch: {}", filename));
        }
    }

    if !errors.is_empty() {
        for e in &errors {
            log::error!("  {}", e);
        }
        bail!("SHA256 verification failed");
    }

    Ok(())
}

/// Check if a bundle is complete (has all required files).
pub fn is_bundle_complete(bundle: &Path) -> bool {
    if !bundle.is_dir() {
        return false;
    }
    REQUIRED_FILES.iter().all(|f| bundle.join(f).exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_result_methods() {
        assert!(VerifyResult::Pass.is_pass());
        assert!(!VerifyResult::Pass.is_fail());

        assert!(VerifyResult::Fail("reason".to_string()).is_fail());
        assert!(!VerifyResult::Fail("reason".to_string()).is_pass());

        assert!(!VerifyResult::Skipped("reason".to_string()).is_pass());
        assert!(!VerifyResult::Skipped("reason".to_string()).is_fail());
    }

    #[test]
    fn test_is_bundle_complete_nonexistent() {
        assert!(!is_bundle_complete(Path::new("/nonexistent/bundle")));
    }

    #[test]
    fn test_required_files_list() {
        assert!(REQUIRED_FILES.contains(&"index.json"));
        assert!(REQUIRED_FILES.contains(&"SHA256SUMS"));
        assert_eq!(REQUIRED_FILES.len(), 6);
    }
}
