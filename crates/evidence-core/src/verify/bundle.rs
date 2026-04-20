//! Top-level orchestrator: `verify_bundle` + `verify_bundle_with_key`.
//!
//! Sequences the check passes that live in sibling modules. Each
//! pass pushes structured errors onto a shared `Vec<VerifyError>`;
//! only I/O errors and parse failures bail early. Everything else
//! accumulates so one run surfaces every problem.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::bundle::{EvidenceIndex, SigningError};
use crate::diagnostic::{DiagnosticCode, Location, Severity};
use crate::hash::{HashError, sha256_file};
use crate::policy::Profile;

use super::consistency::{check_dal_map, check_test_summary, check_trace_outputs_hashed};
use super::cross_file::check_env_vs_index;
use super::engine_source::check_engine_source;
use super::errors::{VerifyError, VerifyResult};
use super::paths::{KNOWN_META_FILES, REQUIRED_FILES, is_safe_bundle_path};

/// Catastrophic errors that abort verification before per-check
/// accumulation begins.
///
/// Distinct from [`VerifyError`]: `VerifyError` records a *validation
/// finding* (a bundle field or hash disagrees); `VerifyRuntimeError`
/// records an *I/O or parsing fault* where the verifier can't even
/// get far enough to make a finding.
#[derive(Debug, Error)]
pub enum VerifyRuntimeError {
    /// Bundle path does not exist or is not a directory.
    #[error("Bundle path does not exist or is not a directory: {0}")]
    BundleNotFound(PathBuf),
    /// I/O failure reading a bundle file.
    #[error("reading {path}")]
    ReadFile {
        /// Bundle-relative path whose read failed.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// `index.json` failed to parse as JSON.
    #[error("parsing index.json")]
    ParseIndex(#[source] serde_json::Error),
    /// File tree walk raised an error (bundle contained an
    /// unreadable directory or symlink).
    #[error("walking bundle tree")]
    Walk(#[source] walkdir::Error),
    /// A file hash couldn't be computed.
    #[error(transparent)]
    Hash(#[from] HashError),
    /// HMAC signature verification had an I/O or envelope error
    /// (distinct from the signature being invalid, which is a
    /// [`VerifyError::HmacFailure`]).
    #[error(transparent)]
    Signing(#[from] SigningError),
}

impl DiagnosticCode for VerifyRuntimeError {
    fn code(&self) -> &'static str {
        // Runtime faults that abort verify before it can make any
        // findings. These map to exit code 1 (not 2), per the
        // Schema Rule 1 contract.
        match self {
            VerifyRuntimeError::BundleNotFound(_) => "VERIFY_RUNTIME_BUNDLE_NOT_FOUND",
            VerifyRuntimeError::ReadFile { .. } => "VERIFY_RUNTIME_READ_FILE",
            VerifyRuntimeError::ParseIndex(_) => "VERIFY_RUNTIME_PARSE_INDEX",
            VerifyRuntimeError::Walk(_) => "VERIFY_RUNTIME_WALK",
            VerifyRuntimeError::Hash(_) => "VERIFY_RUNTIME_HASH",
            VerifyRuntimeError::Signing(_) => "VERIFY_RUNTIME_SIGNING",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn location(&self) -> Option<Location> {
        match self {
            VerifyRuntimeError::BundleNotFound(p) => Some(Location {
                file: Some(p.clone()),
                ..Location::default()
            }),
            VerifyRuntimeError::ReadFile { path, .. } => Some(Location {
                file: Some(path.clone()),
                ..Location::default()
            }),
            // Remaining variants wrap inner errors without surfacing
            // a path at this layer. `Hash(HashError)` and friends
            // would grow their own `DiagnosticCode` impls and the
            // wrapper would then forward via `source()`.
            VerifyRuntimeError::ParseIndex(_)
            | VerifyRuntimeError::Walk(_)
            | VerifyRuntimeError::Hash(_)
            | VerifyRuntimeError::Signing(_) => None,
        }
    }
}

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
pub fn verify_bundle(bundle: &Path) -> Result<VerifyResult, VerifyRuntimeError> {
    verify_bundle_with_key(bundle, None)
}

/// Verify an evidence bundle, optionally checking HMAC signature.
pub fn verify_bundle_with_key(
    bundle: &Path,
    verify_key: Option<&[u8]>,
) -> Result<VerifyResult, VerifyRuntimeError> {
    tracing::info!("verify: checking bundle at {:?}", bundle);

    // Collect all verification errors rather than bailing on first failure.
    // VerifyRuntimeError is reserved for I/O errors and parse failures.
    // VerifyResult::Fail is used for all verification-level findings.
    let mut verify_errors: Vec<VerifyError> = Vec::new();

    // 1. Check bundle directory exists (runtime fault — bail)
    if !bundle.is_dir() {
        return Err(VerifyRuntimeError::BundleNotFound(bundle.to_path_buf()));
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
        fs::read_to_string(&index_path).map_err(|source| VerifyRuntimeError::ReadFile {
            path: index_path.clone(),
            source,
        })?;
    let index: EvidenceIndex =
        serde_json::from_str(&index_content).map_err(VerifyRuntimeError::ParseIndex)?;

    if !index.bundle_complete {
        verify_errors.push(VerifyError::ContentHashMismatch {
            index_hash: "bundle_complete=false".to_string(),
            actual_hash: "bundle_complete=true required".to_string(),
        });
        return Ok(VerifyResult::Fail(verify_errors));
    }

    // 3b. Validate index.json field formats (semantic checks beyond serde)
    validate_index_fields(&index, &mut verify_errors);

    // 3c. Cross-file consistency: env.json vs index.json
    check_env_vs_index(bundle, &index, &mut verify_errors);

    // 3d. Pre-release tool refusal (SYS-017 / HLR-049 / LLR-049).
    // Library stays policy-free — push the finding on every profile
    // regardless. The CLI partitions severity by `(code, profile)`:
    // Dev downgrades this code to Warning; Cert/Record keeps Error.
    check_prerelease_tool(bundle, &index, &mut verify_errors);

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
    let sha256sums_content =
        fs::read_to_string(&sha256sums_path).map_err(|source| VerifyRuntimeError::ReadFile {
            path: sha256sums_path.clone(),
            source,
        })?;
    let listed_files = hash_sha256sums_entries(bundle, &sha256sums_content, &mut verify_errors)?;

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

    // 6a + 6b. deterministic_hash == SHA-256(deterministic-manifest.json)
    // and the manifest is a faithful projection of env.json.
    check_deterministic_manifest(bundle, &index, &mut verify_errors)?;

    // 6c/6d/6e. Consistency cross-checks (trace_outputs in SHA256SUMS,
    // test_summary re-parse, dal_map ↔ compliance reports).
    check_trace_outputs_hashed(&index, &listed_files, &mut verify_errors);
    check_test_summary(bundle, &index, &mut verify_errors);
    check_dal_map(bundle, &index, &mut verify_errors);

    // 7. Extra-file detection: walk all files in bundle and flag unexpected ones
    for entry in walkdir::WalkDir::new(bundle).follow_links(false) {
        let entry = entry.map_err(VerifyRuntimeError::Walk)?;
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

/// Step 3b — semantic field-format checks on a parsed `EvidenceIndex`.
fn validate_index_fields(index: &EvidenceIndex, errors: &mut Vec<VerifyError>) {
    // `profile` is typed `Profile`, so a bogus string would have
    // failed serde deserialization before we got here; no runtime
    // FormatError branch needed.
    //
    // git_sha must be 40-char hex for cert/record profiles; dev allows "unknown".
    if !matches!(index.profile, Profile::Dev)
        && (index.git_sha.len() != 40 || !index.git_sha.chars().all(|c| c.is_ascii_hexdigit()))
    {
        errors.push(VerifyError::FormatError {
            field: "git_sha".to_string(),
            expected: "40-character hex (required for cert/record profiles)".to_string(),
            actual: index.git_sha.clone(),
        });
    }
    check_engine_source(
        &index.engine_build_source,
        &index.engine_git_sha,
        index.profile,
        errors,
    );
    if chrono::DateTime::parse_from_rfc3339(&index.timestamp_rfc3339).is_err() {
        errors.push(VerifyError::FormatError {
            field: "timestamp_rfc3339".to_string(),
            expected: "valid RFC3339 datetime".to_string(),
            actual: index.timestamp_rfc3339.clone(),
        });
    }
    if index.content_hash.len() != 64
        || !index
            .content_hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        errors.push(VerifyError::FormatError {
            field: "content_hash".to_string(),
            expected: "64-character lowercase hex (SHA-256)".to_string(),
            actual: index.content_hash.clone(),
        });
    }
    if index.deterministic_hash.len() != 64
        || !index
            .deterministic_hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        errors.push(VerifyError::FormatError {
            field: "deterministic_hash".to_string(),
            expected: "64-character lowercase hex (SHA-256)".to_string(),
            actual: index.deterministic_hash.clone(),
        });
    }
}

/// Step 5 — walk the SHA256SUMS file, hashing every referenced file
/// and comparing against the recorded hash. Returns the set of file
/// paths listed so downstream checks can cross-reference it.
fn hash_sha256sums_entries(
    bundle: &Path,
    sha256sums_content: &str,
    errors: &mut Vec<VerifyError>,
) -> Result<BTreeSet<String>, VerifyRuntimeError> {
    let mut listed_files: BTreeSet<String> = BTreeSet::new();

    for line in sha256sums_content.lines() {
        if line.is_empty() {
            continue;
        }
        // Format: <hash>  <filename> (two spaces)
        let parts: Vec<&str> = line.splitn(2, "  ").collect();
        if parts.len() != 2 {
            errors.push(VerifyError::HashMismatch {
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
            errors.push(VerifyError::HashMismatch {
                file: filename.to_string(),
                expected: format!("64-char hex, got {} chars", expected_hash.len()),
                actual: expected_hash.to_string(),
            });
            continue;
        }

        // Path safety: reject traversal, absolute, or drive-prefixed paths
        // BEFORE any bundle.join() to prevent reads outside the bundle.
        if !is_safe_bundle_path(filename) {
            errors.push(VerifyError::UnsafePath(filename.to_string()));
            continue;
        }

        listed_files.insert(filename.to_string());
        let file_path = bundle.join(filename);

        if !file_path.exists() {
            errors.push(VerifyError::MissingHashedFile(filename.to_string()));
            continue;
        }

        let actual_hash = sha256_file(&file_path)?;
        if actual_hash != expected_hash {
            errors.push(VerifyError::HashMismatch {
                file: filename.to_string(),
                expected: expected_hash.to_string(),
                actual: actual_hash,
            });
        }
    }

    Ok(listed_files)
}

/// Steps 6a + 6b — deterministic_hash matches the manifest file's
/// SHA-256, AND the manifest is a faithful projection of env.json.
///
/// The two checks are paired: without 6b, a tamperer could produce a
/// well-formed bundle where `deterministic-manifest.json` and
/// `index.json.deterministic_hash` agree with each other but disagree
/// with `env.json`. 6b closes that gap by re-projecting env.json and
/// byte-comparing to the committed manifest.
fn check_deterministic_manifest(
    bundle: &Path,
    index: &EvidenceIndex,
    errors: &mut Vec<VerifyError>,
) -> Result<(), VerifyRuntimeError> {
    let manifest_path = bundle.join("deterministic-manifest.json");
    if !manifest_path.exists() {
        return Ok(());
    }

    let actual_manifest_hash = sha256_file(&manifest_path)?;
    if index.deterministic_hash != actual_manifest_hash {
        errors.push(VerifyError::DeterministicHashMismatch {
            index_hash: index.deterministic_hash.clone(),
            actual_hash: actual_manifest_hash,
        });
    }

    let env_path = bundle.join("env.json");
    if !env_path.exists() {
        return Ok(());
    }
    let (env_bytes, manifest_bytes) = match (fs::read(&env_path), fs::read(&manifest_path)) {
        (Ok(a), Ok(b)) => (a, b),
        _ => {
            // One or both files missing already produced a
            // MissingHashedFile error upstream. Nothing to do here.
            return Ok(());
        }
    };
    match serde_json::from_slice::<crate::env::EnvFingerprint>(&env_bytes) {
        Ok(env_fp) => match serde_json::to_vec_pretty(&env_fp.deterministic_manifest()) {
            Ok(reprojected) => {
                if reprojected != manifest_bytes {
                    errors.push(VerifyError::ManifestProjectionDrift {
                        detail: format!(
                            "manifest {} bytes, reprojection {} bytes",
                            manifest_bytes.len(),
                            reprojected.len()
                        ),
                    });
                }
            }
            Err(e) => {
                errors.push(VerifyError::ManifestProjectionDrift {
                    detail: format!("serialize reprojection: {}", e),
                });
            }
        },
        Err(e) => {
            errors.push(VerifyError::ManifestProjectionDrift {
                detail: format!("parse env.json: {}", e),
            });
        }
    }
    Ok(())
}

/// Push `VerifyError::PrereleaseToolDetected` when `env.json`
/// reports `tool_prerelease = true`. Library reports what's true;
/// the CLI partitions severity by profile. Missing/unparseable
/// env.json is already caught upstream — silent return here
/// avoids double-firing on the same root cause.
fn check_prerelease_tool(bundle: &Path, index: &EvidenceIndex, errors: &mut Vec<VerifyError>) {
    let Ok(env_bytes) = fs::read(bundle.join("env.json")) else {
        return;
    };
    let Ok(env_fp) = serde_json::from_slice::<crate::env::EnvFingerprint>(&env_bytes) else {
        return;
    };
    if env_fp.tool_prerelease {
        errors.push(VerifyError::PrereleaseToolDetected {
            profile: index.profile.to_string(),
            engine_crate_version: index.engine_crate_version.clone(),
        });
    }
}
