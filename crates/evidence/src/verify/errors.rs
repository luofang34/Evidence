//! `VerifyError` + `VerifyResult` — the structured result types
//! returned by every verification pass.

use std::path::PathBuf;

use serde::Serialize;

use crate::diagnostic::{DiagnosticCode, Location, Severity};

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
        /// Bundle-relative path whose hash disagreed.
        file: String,
        /// Hash recorded in `SHA256SUMS`.
        expected: String,
        /// Hash computed at verify time.
        actual: String,
    },
    /// A file listed in SHA256SUMS is missing from the bundle
    MissingHashedFile(String),
    /// content_hash in index.json doesn't match SHA256SUMS hash
    ContentHashMismatch {
        /// Value recorded in `index.json.content_hash`.
        index_hash: String,
        /// `SHA-256(SHA256SUMS)` computed at verify time.
        actual_hash: String,
    },
    /// A path in SHA256SUMS or trace_outputs contains path traversal
    /// (`..`, absolute path, or Windows drive prefix)
    UnsafePath(String),
    /// A field in index.json has an invalid format
    FormatError {
        /// Field name that failed the format check.
        field: String,
        /// Description of the expected shape.
        expected: String,
        /// Actual value found in the bundle.
        actual: String,
    },
    /// A field in env.json is inconsistent with the same field in index.json
    CrossFileInconsistency {
        /// Field name that disagreed between `env.json` and `index.json`.
        field: String,
        /// Value read from `index.json`.
        index_value: String,
        /// Value read from `env.json`.
        env_value: String,
    },
    /// `deterministic_hash` in `index.json` does not match the actual
    /// SHA-256 of `deterministic-manifest.json`.
    DeterministicHashMismatch {
        /// Value recorded in `index.json.deterministic_hash`.
        index_hash: String,
        /// `SHA-256(deterministic-manifest.json)` computed at verify time.
        actual_hash: String,
    },
    /// Re-projecting `env.json`'s `DeterministicManifest` subset
    /// does not byte-equal the committed `deterministic-manifest.json`.
    /// Indicates tampering or a CLI bug that let the two drift apart
    /// at generation time.
    ManifestProjectionDrift {
        /// Short description — byte lengths, parse failure, or
        /// serialization failure.
        detail: String,
    },
    /// A trace output path in `index.json.trace_outputs` is not
    /// listed in `SHA256SUMS`. Every generated trace matrix must be
    /// in the content layer; an index-only reference overclaims
    /// coverage because the referenced file would not be integrity-
    /// checked.
    TraceOutputNotHashed(String),
    /// `index.json.test_summary` disagrees with a re-parse of the
    /// captured `tests/cargo_test_stdout.txt` — either the summary
    /// was tampered or the generator's parser drifted from the
    /// verifier's.
    TestSummaryMismatch {
        /// Which counter disagreed (`total` / `passed` / `failed` /
        /// `ignored` / `filtered_out` / `parse`).
        field: &'static str,
        /// Value recorded in `index.json.test_summary`.
        index_value: String,
        /// Value obtained by re-parsing captured stdout.
        parsed_value: String,
    },
    /// `index.json.dal_map[crate]` disagrees with
    /// `compliance/<crate>.json.dal` for the same crate.
    DalMapMismatch {
        /// Crate whose DAL differs across the two files.
        crate_name: String,
        /// DAL level recorded in `index.json.dal_map`.
        index_value: String,
        /// DAL level recorded in the compliance report.
        compliance_value: String,
    },
    /// `compliance/<crate>.json` is present in the bundle but its
    /// crate name is not referenced in `index.json.dal_map`, or
    /// vice versa.
    DalMapOrphan {
        /// Crate whose references don't match.
        crate_name: String,
        /// Which direction of the mismatch fired.
        detail: String,
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
            VerifyError::TraceOutputNotHashed(path) => {
                write!(
                    f,
                    "trace_outputs entry '{}' is not listed in SHA256SUMS",
                    path
                )
            }
            VerifyError::TestSummaryMismatch {
                field,
                index_value,
                parsed_value,
            } => {
                write!(
                    f,
                    "test_summary.{} disagrees with re-parsed stdout: index={}, parsed={}",
                    field, index_value, parsed_value
                )
            }
            VerifyError::DalMapMismatch {
                crate_name,
                index_value,
                compliance_value,
            } => {
                write!(
                    f,
                    "dal_map[{}] disagrees with compliance/{}.json: index={}, compliance={}",
                    crate_name, crate_name, index_value, compliance_value
                )
            }
            VerifyError::DalMapOrphan { crate_name, detail } => {
                write!(f, "dal_map orphan for '{}': {}", crate_name, detail)
            }
        }
    }
}

impl DiagnosticCode for VerifyError {
    fn code(&self) -> &'static str {
        // Exhaustive match: adding a new `VerifyError` variant without
        // a stable code here fails compilation — Schema Rule 3.
        match self {
            VerifyError::UnexpectedFile(_) => "VERIFY_UNEXPECTED_FILE",
            VerifyError::HmacFailure => "VERIFY_HMAC_FAILURE",
            VerifyError::HashMismatch { .. } => "VERIFY_HASH_MISMATCH",
            VerifyError::MissingHashedFile(_) => "VERIFY_MISSING_HASHED_FILE",
            VerifyError::ContentHashMismatch { .. } => "VERIFY_CONTENT_HASH_MISMATCH",
            VerifyError::UnsafePath(_) => "VERIFY_UNSAFE_PATH",
            VerifyError::FormatError { .. } => "VERIFY_INVALID_FORMAT",
            VerifyError::CrossFileInconsistency { .. } => "VERIFY_CROSS_FILE_INCONSISTENCY",
            VerifyError::DeterministicHashMismatch { .. } => "VERIFY_DETERMINISTIC_HASH_MISMATCH",
            VerifyError::ManifestProjectionDrift { .. } => "VERIFY_MANIFEST_PROJECTION_DRIFT",
            VerifyError::TraceOutputNotHashed(_) => "VERIFY_TRACE_OUTPUT_NOT_HASHED",
            VerifyError::TestSummaryMismatch { .. } => "VERIFY_TEST_SUMMARY_MISMATCH",
            VerifyError::DalMapMismatch { .. } => "VERIFY_DAL_MAP_MISMATCH",
            VerifyError::DalMapOrphan { .. } => "VERIFY_DAL_MAP_ORPHAN",
        }
    }

    fn severity(&self) -> Severity {
        // Every VerifyError is a finding, not a progress event.
        Severity::Error
    }

    fn location(&self) -> Option<Location> {
        // Populate Location.file from bundle-relative paths the error
        // already carries. Agents can match on toml_path when the
        // underlying emitter has enough structure, but VerifyError
        // lives one layer above TOML — bundle-file paths are the
        // strongest locators available. `line`/`col` stay None
        // because verify operates on binary/JSON content, not
        // human-edited source.
        let file_path = match self {
            VerifyError::UnexpectedFile(p)
            | VerifyError::MissingHashedFile(p)
            | VerifyError::UnsafePath(p)
            | VerifyError::TraceOutputNotHashed(p) => Some(PathBuf::from(p)),
            VerifyError::HashMismatch { file, .. } => Some(PathBuf::from(file)),
            VerifyError::DalMapMismatch { crate_name, .. }
            | VerifyError::DalMapOrphan { crate_name, .. } => {
                // The DAL-map mismatches point at a compliance file
                // keyed by crate name. Surface that path so an agent
                // can jump straight to the offending report.
                Some(PathBuf::from(format!("compliance/{}.json", crate_name)))
            }
            // The remaining variants are bundle-wide invariants; no
            // single file "owns" the failure.
            VerifyError::HmacFailure
            | VerifyError::ContentHashMismatch { .. }
            | VerifyError::FormatError { .. }
            | VerifyError::CrossFileInconsistency { .. }
            | VerifyError::DeterministicHashMismatch { .. }
            | VerifyError::ManifestProjectionDrift { .. }
            | VerifyError::TestSummaryMismatch { .. } => None,
        };

        file_path.map(|file| Location {
            file: Some(file),
            ..Location::default()
        })
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
}
