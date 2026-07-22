//! Typed per-head payload findings for offline source verification
//! (LLR-137).
//!
//! [`SourcePayloadError`] is the payload-finding taxonomy: every
//! degenerate vendored input fails closed with the source uid, the
//! document key, and the path or digests needed to fix it, and the
//! variant alone names the failed check. A finding is per-head
//! data — it rides inside
//! [`SourceVerification::outcome`](super::SourceVerification::outcome)
//! and never aborts the batch, so one bad payload cannot hide
//! findings for later heads. Global prerequisite failures (graph
//! validation, lock parsing, canonicality, graph-lock equality) are
//! not here; they report through the [`SourceError`] /
//! [`SourceLockError`] taxonomy of the lock pipeline.
//!
//! [`SourceError`]: super::super::error::SourceError
//! [`SourceLockError`]: super::super::lock::SourceLockError

use std::path::PathBuf;

use thiserror::Error;

use super::super::super::digest::SourceContentDigest;

/// A typed payload finding for one effective source head (LLR-137).
///
/// Every variant fails closed: the head cannot be reported
/// byte-verified, and the finding carries the context needed to
/// locate and fix the input. Only vendored material produces payload
/// findings; the other capture modes resolve to their weaker states
/// without touching the filesystem.
#[derive(Debug, Error)]
pub enum SourcePayloadError {
    /// The vendored payload path does not exist beneath the payload
    /// root — the file is absent, or a directory on the path is
    /// absent or is itself not a directory.
    #[error(
        "vendored payload for source {source_uid} (document key {document_key}) \
         is missing at {path}"
    )]
    MissingPayload {
        /// Uid of the source revision whose payload is missing.
        source_uid: String,
        /// Document key of the source revision.
        document_key: String,
        /// Resolved payload path that does not exist.
        path: PathBuf,
    },
    /// The vendored payload path resolves to something other than a
    /// regular file — a directory, a FIFO, a device node.
    #[error(
        "vendored payload for source {source_uid} (document key {document_key}) \
         at {path} is not a regular file"
    )]
    NotAFile {
        /// Uid of the source revision whose payload is not a file.
        source_uid: String,
        /// Document key of the source revision.
        document_key: String,
        /// Resolved payload path that is not a regular file.
        path: PathBuf,
    },
    /// The payload root `<corpus-root>/sources/` itself is a
    /// symlink; the payload root must be a real directory so no
    /// attacker-selected target can redirect resolution. The root is
    /// corpus-wide state, not one head's record, so the finding
    /// names the root rather than a source uid.
    #[error("vendored payload root {root} is a symlink; the payload root must be a real directory")]
    SymlinkRoot {
        /// The payload root that is a symlink.
        root: PathBuf,
    },
    /// A component of the vendored payload path beneath the payload
    /// root is a symlink; payload paths must resolve through real
    /// directories only.
    #[error(
        "vendored payload path for source {source_uid} (document key {document_key}) \
         traverses symlinked component {component}"
    )]
    SymlinkComponent {
        /// Uid of the source revision whose payload path is unsafe.
        source_uid: String,
        /// Document key of the source revision.
        document_key: String,
        /// The offending path component, as resolved so far.
        component: PathBuf,
    },
    /// The vendored path cannot stay beneath the fixed payload root:
    /// it fails the lexical wire-form check (absolute, drive or UNC
    /// prefixed, backslash, or an empty, `.`, or `..` component —
    /// re-checked here as defense in depth for programmatically
    /// built graphs), its leading component is not the payload-root
    /// directory, or its canonicalized filesystem resolution escapes
    /// the canonicalized payload root.
    #[error(
        "vendored path for source {source_uid} (document key {document_key}) \
         escapes the fixed payload root: {path}"
    )]
    PathEscape {
        /// Uid of the source revision whose path escapes.
        source_uid: String,
        /// Document key of the source revision.
        document_key: String,
        /// The offending path: the stored wire path for lexical
        /// rejections, the resolved path for containment failures.
        path: PathBuf,
    },
    /// The vendored payload bytes were read but digest differently
    /// than the source revision declares.
    #[error(
        "vendored payload for source {source_uid} (document key {document_key}) \
         at {path} digests to {actual}, but the record declares {expected}"
    )]
    DigestMismatch {
        /// Uid of the source revision whose payload mismatches.
        source_uid: String,
        /// Document key of the source revision.
        document_key: String,
        /// Resolved payload path whose bytes were hashed.
        path: PathBuf,
        /// The digest the source revision declares.
        expected: SourceContentDigest,
        /// The digest of the exact bytes read from disk.
        actual: SourceContentDigest,
    },
    /// The `sources.lock` entry for the head disagrees with the
    /// source revision record — the availability, capture mode, or
    /// digest bound in the lock differs from the record. Unreachable
    /// after the global graph-lock equality gate passes; the per-head
    /// check is defense in depth so a future gate change degrades to
    /// a typed finding instead of a wrong `VerifiedBytes`.
    #[error(
        "sources.lock disagrees with the record for source {source_uid} \
         (document key {document_key}) in field {field}"
    )]
    LockDisagreement {
        /// Uid of the source revision the lock disagrees with.
        source_uid: String,
        /// Document key of the source revision.
        document_key: String,
        /// The field that disagrees: `uid`, `availability`,
        /// `capture_mode`, or `digest`.
        field: &'static str,
    },
    /// Reading the vendored payload failed at the filesystem layer —
    /// permission denied, a component that is not a directory, or a
    /// read failure mid-stream.
    #[error(
        "reading vendored payload for source {source_uid} (document key {document_key}) at {path}"
    )]
    Io {
        /// Uid of the source revision whose payload could not be
        /// read.
        source_uid: String,
        /// Document key of the source revision.
        document_key: String,
        /// Path being resolved or read when the failure occurred.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
}
