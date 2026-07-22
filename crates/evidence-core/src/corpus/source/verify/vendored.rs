//! Vendored payload verification (LLR-137): lock-entry agreement,
//! safe path resolution beneath the fixed `<corpus-root>/sources/`
//! payload root, and exact-byte digest comparison.
//!
//! Split from `verify.rs`: the batch facade owns per-head
//! dispatch; this sibling owns everything downstream of
//! `SourceCapture::Vendored`.

use std::path::{Path, PathBuf};

use super::super::super::digest::SourceContentDigest;
use super::super::super::graph::SourceRevisionNode;
use super::super::lock::{LockCapture, LockMaterial, SourceLockEntry};
use super::super::records::validate_vendored_wire_path;
use super::PAYLOAD_ROOT_DIR;
use super::{DigestMismatchDetail, SourcePayloadError, SourceVerificationState};

/// Verify one vendored head (LLR-137): the lock entry must agree
/// with the record, the payload must resolve safely beneath the
/// fixed payload root, and the exact raw bytes must digest to the
/// declared value.
pub(super) fn verify_vendored_head(
    corpus_root: &Path,
    document_key: &str,
    revision: &SourceRevisionNode,
    wire_path: &str,
    record_digest: &SourceContentDigest,
    lock_entry: &SourceLockEntry,
) -> Result<SourceVerificationState, SourcePayloadError> {
    let disagreement = |field: &'static str| SourcePayloadError::LockDisagreement {
        source_uid: revision.uid.clone(),
        document_key: document_key.to_string(),
        field,
    };
    let LockMaterial::Available {
        sha256: lock_digest,
        capture,
    } = &lock_entry.material
    else {
        return Err(disagreement("availability"));
    };
    if !matches!(capture, LockCapture::Vendored) {
        return Err(disagreement("capture_mode"));
    }
    if lock_digest != record_digest {
        return Err(disagreement("digest"));
    }

    let candidate = resolve_vendored_path(corpus_root, document_key, &revision.uid, wire_path)?;
    let actual = crate::hash::sha256_file(&candidate).map_err(|err| {
        let source = match err {
            crate::hash::HashError::Open { source, .. }
            | crate::hash::HashError::Read { source, .. } => source,
            // `sha256_file` only opens and reads; the remaining
            // `HashError` variants belong to other helpers and are
            // unreachable here, but degrade to a typed I/O finding
            // rather than a panic if that ever changes.
            other => std::io::Error::other(other),
        };
        SourcePayloadError::Io {
            source_uid: revision.uid.clone(),
            document_key: document_key.to_string(),
            path: candidate.clone(),
            source,
        }
    })?;
    let actual = SourceContentDigest::from_hasher_output(actual);
    if &actual != record_digest {
        return Err(SourcePayloadError::DigestMismatch(Box::new(
            DigestMismatchDetail {
                source_uid: revision.uid.clone(),
                document_key: document_key.to_string(),
                path: candidate,
                expected: record_digest.clone(),
                actual,
            },
        )));
    }
    Ok(SourceVerificationState::VerifiedBytes)
}

/// Resolve a vendored wire path to a verified regular file beneath
/// the fixed `<corpus-root>/sources/` payload root (LLR-137).
///
/// The wire path is corpus-root-relative with a leading `sources/`
/// component (the stored wire form), so the candidate is
/// `corpus_root.join(wire_path)`. Every step fails closed: the
/// record-load lexical check is re-run (defense in depth for
/// programmatically built graphs), the path must sit beneath the
/// payload root, the root itself must not be a symlink, no
/// component may be a symlink, the final component must be a
/// regular file, and the canonicalized target must stay beneath the
/// canonicalized root.
pub(super) fn resolve_vendored_path(
    corpus_root: &Path,
    document_key: &str,
    source_uid: &str,
    wire_path: &str,
) -> Result<PathBuf, SourcePayloadError> {
    let (payload_root, candidate, relative) =
        check_wire_and_root(corpus_root, document_key, source_uid, wire_path)?;
    walk_components(
        document_key,
        source_uid,
        &candidate,
        &payload_root,
        &relative,
    )?;
    check_canonical_containment(document_key, source_uid, &candidate, &payload_root)?;
    Ok(candidate)
}

fn escape_error(document_key: &str, source_uid: &str, path: PathBuf) -> SourcePayloadError {
    SourcePayloadError::PathEscape {
        source_uid: source_uid.to_string(),
        document_key: document_key.to_string(),
        path,
    }
}

fn missing_error(document_key: &str, source_uid: &str, path: PathBuf) -> SourcePayloadError {
    SourcePayloadError::MissingPayload {
        source_uid: source_uid.to_string(),
        document_key: document_key.to_string(),
        path,
    }
}

fn io_error(
    document_key: &str,
    source_uid: &str,
    path: PathBuf,
    source: std::io::Error,
) -> SourcePayloadError {
    SourcePayloadError::Io {
        source_uid: source_uid.to_string(),
        document_key: document_key.to_string(),
        path,
        source,
    }
}

fn not_a_file_error(document_key: &str, source_uid: &str, path: PathBuf) -> SourcePayloadError {
    SourcePayloadError::NotAFile {
        source_uid: source_uid.to_string(),
        document_key: document_key.to_string(),
        path,
    }
}

/// Lexical and payload-root validation: the wire form is re-checked
/// (defense in depth for programmatically built graphs — record
/// loading already gates it, LLR-125), the candidate must sit
/// strictly beneath `<corpus-root>/sources/`, and the root itself
/// must be a real directory, not a symlink. Returns the payload
/// root, the candidate path, and the root-relative remainder.
fn check_wire_and_root(
    corpus_root: &Path,
    document_key: &str,
    source_uid: &str,
    wire_path: &str,
) -> Result<(PathBuf, PathBuf, PathBuf), SourcePayloadError> {
    if validate_vendored_wire_path(wire_path).is_err() {
        return Err(escape_error(
            document_key,
            source_uid,
            PathBuf::from(wire_path),
        ));
    }
    let payload_root = corpus_root.join(PAYLOAD_ROOT_DIR);
    let candidate = corpus_root.join(wire_path);
    // The wire path must name a path strictly beneath the payload
    // root: its leading component is the payload-root directory
    // name. `strip_prefix` is component-wise, so a sibling like
    // `sources-extra/x` never prefixes.
    let relative = candidate
        .strip_prefix(&payload_root)
        .map_err(|_| escape_error(document_key, source_uid, PathBuf::from(wire_path)))?
        .to_path_buf();
    if relative.as_os_str().is_empty() {
        // The wire path names the payload root itself — a
        // directory, never a payload file.
        return Err(not_a_file_error(document_key, source_uid, candidate));
    }

    let root_metadata = std::fs::symlink_metadata(&payload_root).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            missing_error(document_key, source_uid, candidate.clone())
        } else {
            io_error(document_key, source_uid, payload_root.clone(), err)
        }
    })?;
    if root_metadata.file_type().is_symlink() {
        return Err(SourcePayloadError::SymlinkRoot { root: payload_root });
    }
    if !root_metadata.is_dir() {
        // A non-directory root cannot contain payloads.
        return Err(missing_error(document_key, source_uid, candidate));
    }
    Ok((payload_root, candidate, relative))
}

/// Component walk: no path component may be a symlink, and the
/// final component must be a regular file.
fn walk_components(
    document_key: &str,
    source_uid: &str,
    candidate: &Path,
    payload_root: &Path,
    relative: &Path,
) -> Result<(), SourcePayloadError> {
    let mut current = payload_root.to_path_buf();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        current.push(component.as_os_str());
        let metadata = std::fs::symlink_metadata(&current).map_err(|err| {
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) {
                // Missing, or a mid-path component is not a
                // directory: the payload does not exist either way.
                missing_error(document_key, source_uid, candidate.to_path_buf())
            } else {
                io_error(document_key, source_uid, current.clone(), err)
            }
        })?;
        if metadata.file_type().is_symlink() {
            return Err(SourcePayloadError::SymlinkComponent {
                source_uid: source_uid.to_string(),
                document_key: document_key.to_string(),
                component: current.clone(),
            });
        }
        if components.peek().is_none() && !metadata.is_file() {
            return Err(not_a_file_error(
                document_key,
                source_uid,
                candidate.to_path_buf(),
            ));
        }
    }
    Ok(())
}

/// Canonical containment: the filesystem-level proof that
/// resolution stayed beneath the payload root. The lexical checks
/// leave no room for a symlink or `..` escape; this is belt and
/// suspenders against anything they missed.
fn check_canonical_containment(
    document_key: &str,
    source_uid: &str,
    candidate: &Path,
    payload_root: &Path,
) -> Result<(), SourcePayloadError> {
    let canonical_root = std::fs::canonicalize(payload_root)
        .map_err(|err| io_error(document_key, source_uid, payload_root.to_path_buf(), err))?;
    let canonical_candidate = std::fs::canonicalize(candidate)
        .map_err(|err| io_error(document_key, source_uid, candidate.to_path_buf(), err))?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(escape_error(
            document_key,
            source_uid,
            candidate.to_path_buf(),
        ));
    }
    Ok(())
}
