//! Cross-check `engine_build_source` against the shape of `engine_git_sha`.
//!
//! A qualified (cert/record) bundle cannot honestly carry
//! `engine_build_source = "release"` or `"unknown"`: it must be pinned
//! to a specific engine commit. Dev bundles are permissive.

use super::errors::VerifyError;

/// Cross-check `engine_build_source` against the shape of
/// `engine_git_sha`, and reject combinations that a qualified bundle
/// cannot honestly carry.
pub(super) fn check_engine_source(
    source: &str,
    sha: &str,
    profile: &str,
    errors: &mut Vec<VerifyError>,
) {
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
            // Legacy bundle written before `engine_build_source` was
            // added (serde default fills it in on read). Dev tolerates
            // it for backward compatibility; cert/record cannot accept
            // a bundle whose engine provenance is unlabeled.
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
