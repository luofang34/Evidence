//! Offline verification of the material behind each effective
//! source revision (LLR-136, LLR-137, LLR-138).
//!
//! [`verify_effective_sources`] validates the material behind every
//! effective source head without network access and without
//! overstating what each capture mode proves. The result
//! distinguishes four typed states — see
//! [`SourceVerificationState`] — of which only
//! [`SourceVerificationState::VerifiedBytes`] is local byte
//! verification: schema-valid, locked, and byte-verified are
//! separate facts, and an unavailable head is reportable but can
//! never satisfy a byte-verified policy.
//!
//! # Batch contract
//!
//! Graph validation, strict lock parsing, lock canonicality, and
//! graph-lock equality are GLOBAL PREREQUISITES, applied in order
//! by delegating to
//! [`validate_committed_lock`](super::lock::validate_committed_lock)
//! so the four gates keep one implementation. A prerequisite
//! failure stops the batch with the typed [`SourceError`] before
//! any payload is read. After the gates pass, every effective head
//! reported by
//! [`effective_source_heads`](super::lineage::effective_source_heads)
//! yields exactly one [`SourceVerification`] — its state or its
//! typed [`SourcePayloadError`] finding — and a payload finding
//! never aborts the loop, so one bad payload cannot hide findings
//! for later heads. `effective_source_heads` iterates a `BTreeMap`,
//! so the returned vector is sorted by document key, then source
//! uid, with no re-sort.
//!
//! # Vendored path resolution
//!
//! A vendored record stores its path as the canonical
//! `/`-separated corpus-root-relative wire form whose leading
//! component is the fixed payload-root directory `sources/` — e.g.
//! `sources/doc-1/rev-c.pdf`. Resolution is
//! `corpus_root.join(wire_path)`, always contained beneath the
//! FIXED payload root `<corpus-root>/sources/`, never resolved
//! against an arbitrary caller-selected directory. The record-load
//! lexical check (`validate_vendored_wire_path`, LLR-125) is re-run
//! as defense in depth — a programmatically built graph bypasses
//! record loading — then every filesystem step uses
//! `symlink_metadata`: the payload root itself must not be a
//! symlink, no path component may be a symlink, the final component
//! must be a regular file, and the canonicalized target must stay
//! beneath the canonicalized payload root. Only then are the exact
//! raw bytes streamed through SHA-256 and compared with the record
//! digest and the lock entry digest.
//!
//! # What verification never does
//!
//! - Never touches the network: hash-only verification never
//!   fetches, external-controlled verification never contacts the
//!   external system, and this module's dependency path carries no
//!   HTTP stack — only `std::fs` reads.
//! - Never mutates: registry records, the committed lock, review
//!   files, and payload bytes are read-only inputs.
//! - Never lets retrieval timestamps affect ordering or state
//!   selection: `retrieved_at` is audit metadata (LLR-125).
//!
//! Module map:
//!
//! - `error` — the [`SourcePayloadError`] taxonomy every per-head
//!   payload finding reports through (LLR-137)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::super::digest::SourceContentDigest;
use super::super::graph::{CorpusGraph, Node, SourceCapture, SourceMaterial, SourceRevisionNode};
use super::error::SourceError;
use super::lineage::effective_source_heads;
use super::lock::{
    LockAvailability, LockCaptureMode, SourceLockEntry, parse_lock, validate_committed_lock,
};
use super::records::validate_vendored_wire_path;

pub(super) mod error;

pub use error::SourcePayloadError;

/// The fixed payload-root directory name beneath the corpus root.
/// Vendored payloads resolve beneath `<corpus-root>/sources/` and
/// nowhere else.
const PAYLOAD_ROOT_DIR: &str = "sources";

/// The deterministic typed verification state of one effective
/// source head (LLR-136).
///
/// The states are assurance-ranked, and only the first is local
/// byte verification:
///
/// 1. [`VerifiedBytes`](SourceVerificationState::VerifiedBytes) —
///    the vendored bytes were read from disk and matched the locked
///    digest. This is the ONLY state that proves local bytes.
/// 2. [`DigestDeclared`](SourceVerificationState::DigestDeclared) —
///    hash-only material: a canonical location and a valid declared
///    digest exist, but no local bytes were verified. Nothing was
///    fetched.
/// 3. [`ExternallyControlled`](SourceVerificationState::ExternallyControlled)
///    — a non-empty controlling system, an immutable revision
///    identity, and a declared digest exist, but no local byte claim
///    is made. The external system was never contacted.
/// 4. [`Unavailable`](SourceVerificationState::Unavailable) — the
///    material is explicitly unavailable and carries its recorded
///    reason; reportable and lintable, but never byte-verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceVerificationState {
    /// Vendored bytes were read and matched the locked digest — the
    /// only local byte verification state.
    VerifiedBytes,
    /// Hash-only material: the declared digest is valid, but no
    /// local bytes exist to verify.
    DigestDeclared,
    /// External-controlled material: the immutable control identity
    /// and declared digest are valid, but no local byte claim is
    /// made.
    ExternallyControlled,
    /// The material is explicitly unavailable and carries its
    /// recorded reason.
    Unavailable {
        /// Why the material is unavailable, exactly as recorded.
        reason: String,
    },
}

impl SourceVerificationState {
    /// The stable wire string for this state.
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceVerificationState::VerifiedBytes => "verified_bytes",
            SourceVerificationState::DigestDeclared => "digest_declared",
            SourceVerificationState::ExternallyControlled => "externally_controlled",
            SourceVerificationState::Unavailable { .. } => "unavailable",
        }
    }
}

/// One effective head's verification entry (LLR-136): the head's
/// identity plus its outcome — a verification state or a typed
/// payload finding. A finding is per-head data, never a batch
/// abort.
#[derive(Debug)]
pub struct SourceVerification {
    /// Stable lineage key of the logical document.
    pub document_key: String,
    /// Uid of the effective source revision — the document's head.
    pub source_uid: String,
    /// The head's verification state, or its typed payload finding.
    pub outcome: Result<SourceVerificationState, SourcePayloadError>,
}

/// Verify the material behind every effective source head of
/// `graph` against the committed `lock_bytes`, reading vendored
/// payloads beneath `<corpus-root>/sources/` (LLR-138).
///
/// The global prerequisites run first through
/// [`validate_committed_lock`](super::lock::validate_committed_lock)
/// — graph validation, strict lock parse, canonicality, graph-lock
/// equality — and any failure stops the batch before any payload is
/// read. After they pass, one [`SourceVerification`] per effective
/// head is collected in `BTreeMap` iteration order, so the result
/// is sorted by document key, then source uid, and one bad payload
/// never hides later findings. Read-only: no writes, no network.
///
/// # Errors
///
/// Returns the typed [`SourceError`] of the first failing global
/// prerequisite. Per-head payload failures are NOT errors of this
/// function; they are data inside each entry's `outcome`.
pub fn verify_effective_sources(
    corpus_root: &Path,
    graph: &CorpusGraph,
    lock_bytes: &[u8],
) -> Result<Vec<SourceVerification>, SourceError> {
    // Global prerequisites, in order: `CorpusGraph::validate`,
    // strict parse, canonical bytes, graph-lock equality — one call,
    // one implementation. Any failure stops payload verification
    // here.
    validate_committed_lock(lock_bytes, graph)?;
    // The gates just parsed these exact bytes, so this parse cannot
    // fail; the parsed value is needed to match each head to its
    // lock entry.
    let lock = parse_lock(lock_bytes)?;
    let entries_by_key: BTreeMap<&str, &SourceLockEntry> = lock
        .entries
        .iter()
        .map(|entry| (entry.document_key.as_str(), entry))
        .collect();

    let mut results = Vec::new();
    for (document_key, source_uid) in effective_source_heads(graph) {
        let outcome = match graph.get(&source_uid) {
            Some(Node::SourceRevision(revision)) => {
                verify_head(corpus_root, &document_key, revision, &entries_by_key)
            }
            // `effective_source_heads` maps document keys to uids of
            // source-revision nodes it iterated, so the lookup always
            // succeeds; the `else` guards a future derivation change
            // without panicking.
            _ => continue,
        };
        results.push(SourceVerification {
            document_key,
            source_uid,
            outcome,
        });
    }
    Ok(results)
}

/// Verify one effective head by capture mode (LLR-136). Hash-only,
/// external-controlled, and unavailable material resolve to their
/// weaker states without touching the filesystem — the record-load
/// validation (LLR-125) guarantees their required fields (canonical
/// location and digest; non-empty system and immutable id;
/// non-blank reason). Only vendored material performs I/O.
fn verify_head(
    corpus_root: &Path,
    document_key: &str,
    revision: &SourceRevisionNode,
    entries_by_key: &BTreeMap<&str, &SourceLockEntry>,
) -> Result<SourceVerificationState, SourcePayloadError> {
    match &revision.material {
        SourceMaterial::Unavailable { reason } => Ok(SourceVerificationState::Unavailable {
            reason: reason.clone(),
        }),
        SourceMaterial::Available {
            sha256, capture, ..
        } => match capture {
            SourceCapture::HashOnly {} => Ok(SourceVerificationState::DigestDeclared),
            SourceCapture::ExternalControlled { .. } => {
                Ok(SourceVerificationState::ExternallyControlled)
            }
            SourceCapture::Vendored { path } => {
                // Graph-lock equality (gate 3) guarantees the entry
                // exists and agrees; the lookup and per-field check
                // are defense in depth so a future gate change
                // degrades to a typed finding, never a panic or a
                // wrong `VerifiedBytes`.
                let Some(lock_entry) = entries_by_key.get(document_key).copied() else {
                    return Err(SourcePayloadError::LockDisagreement {
                        source_uid: revision.uid.clone(),
                        document_key: document_key.to_string(),
                        field: "uid",
                    });
                };
                verify_vendored_head(
                    corpus_root,
                    document_key,
                    revision,
                    path,
                    sha256,
                    lock_entry,
                )
            }
        },
    }
}

/// Verify one vendored head (LLR-137): the lock entry must agree
/// with the record, the payload must resolve safely beneath the
/// fixed payload root, and the exact raw bytes must digest to the
/// declared value.
fn verify_vendored_head(
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
    if lock_entry.availability != LockAvailability::Available {
        return Err(disagreement("availability"));
    }
    if lock_entry.capture_mode != Some(LockCaptureMode::Vendored) {
        return Err(disagreement("capture_mode"));
    }
    if lock_entry.sha256.as_ref() != Some(record_digest) {
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
        return Err(SourcePayloadError::DigestMismatch {
            source_uid: revision.uid.clone(),
            document_key: document_key.to_string(),
            path: candidate,
            expected: record_digest.clone(),
            actual,
        });
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
fn resolve_vendored_path(
    corpus_root: &Path,
    document_key: &str,
    source_uid: &str,
    wire_path: &str,
) -> Result<PathBuf, SourcePayloadError> {
    let escape = |path: PathBuf| SourcePayloadError::PathEscape {
        source_uid: source_uid.to_string(),
        document_key: document_key.to_string(),
        path,
    };
    let missing = |path: PathBuf| SourcePayloadError::MissingPayload {
        source_uid: source_uid.to_string(),
        document_key: document_key.to_string(),
        path,
    };
    let io_failure = |path: PathBuf, source: std::io::Error| SourcePayloadError::Io {
        source_uid: source_uid.to_string(),
        document_key: document_key.to_string(),
        path,
        source,
    };

    // Defense in depth: record loading validated the wire form
    // lexically (LLR-125); a programmatically built graph bypasses
    // that gate, so re-check before touching the filesystem.
    if validate_vendored_wire_path(wire_path).is_err() {
        return Err(escape(PathBuf::from(wire_path)));
    }
    let payload_root = corpus_root.join(PAYLOAD_ROOT_DIR);
    let candidate = corpus_root.join(wire_path);
    // The wire path must name a path strictly beneath the payload
    // root: its leading component is the payload-root directory
    // name. `strip_prefix` is component-wise, so a sibling like
    // `sources-extra/x` never prefixes.
    let relative = candidate
        .strip_prefix(&payload_root)
        .map_err(|_| escape(PathBuf::from(wire_path)))?;
    if relative.as_os_str().is_empty() {
        // The wire path names the payload root itself — a
        // directory, never a payload file.
        return Err(SourcePayloadError::NotAFile {
            source_uid: source_uid.to_string(),
            document_key: document_key.to_string(),
            path: candidate,
        });
    }

    let root_metadata = std::fs::symlink_metadata(&payload_root).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            missing(candidate.clone())
        } else {
            io_failure(payload_root.clone(), err)
        }
    })?;
    if root_metadata.file_type().is_symlink() {
        return Err(SourcePayloadError::SymlinkRoot { root: payload_root });
    }
    if !root_metadata.is_dir() {
        // A non-directory root cannot contain payloads.
        return Err(missing(candidate));
    }

    let mut current = payload_root.clone();
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
                missing(candidate.clone())
            } else {
                io_failure(current.clone(), err)
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
            return Err(SourcePayloadError::NotAFile {
                source_uid: source_uid.to_string(),
                document_key: document_key.to_string(),
                path: candidate,
            });
        }
    }

    // Belt and suspenders: the lexical checks above leave no room
    // for a symlink or `..` escape, but canonical containment is the
    // filesystem-level proof that resolution stayed beneath the
    // payload root.
    let canonical_root = std::fs::canonicalize(&payload_root)
        .map_err(|err| io_failure(payload_root.clone(), err))?;
    let canonical_candidate =
        std::fs::canonicalize(&candidate).map_err(|err| io_failure(candidate.clone(), err))?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(escape(candidate));
    }
    Ok(candidate)
}

// Tests live in sibling files pulled in via `#[path]`: shared
// fixtures plus one module per TEST entry.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test setup failures should panic immediately"
)]
#[path = "verify/batch_tests.rs"]
mod batch_tests;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test setup failures should panic immediately"
)]
#[path = "verify/fixtures.rs"]
mod fixtures;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "verify/tests.rs"]
mod tests;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test setup failures should panic immediately"
)]
#[path = "verify/vendored_tests.rs"]
mod vendored_tests;
