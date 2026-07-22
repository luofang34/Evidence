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
use std::path::Path;

use super::super::graph::{CorpusGraph, Node, SourceCapture, SourceMaterial, SourceRevisionNode};
use super::error::SourceError;
use super::lineage::effective_source_heads;
use super::lock::{SourceLockEntry, parse_lock, validate_committed_lock};

pub(super) mod error;
pub(super) mod vendored;

pub use error::{DigestMismatchDetail, SourcePayloadError};

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
                vendored::verify_vendored_head(
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
