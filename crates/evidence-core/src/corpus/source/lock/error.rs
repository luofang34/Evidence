//! Typed errors for `sources.lock` parsing, canonicality
//! validation, projection comparison, and the blocking reader
//! (LLR-135).
//!
//! [`SourceLockError`] is the lock taxonomy: gate 1 (strict parse)
//! reports [`SourceLockError::Parse`],
//! [`SourceLockError::SchemaTooNew`], and
//! [`SourceLockError::DuplicateKey`]; gate 2 (canonical bytes)
//! reports [`SourceLockError::NonCanonical`]; gate 3 (projection
//! equality) reports [`SourceLockError::Missing`],
//! [`SourceLockError::Extra`], and [`SourceLockError::Changed`]; a
//! graph that fails validation before the gates run reports
//! [`SourceLockError::InvalidGraph`]; the blocking reader reports
//! [`SourceLockError::Read`]. [`SourceError::Lock`] wraps this type
//! so the whole lock pipeline keeps one error type and
//! `CorpusError::Source` stays the only corpus wrapper.
//!
//! [`SourceError::Lock`]: super::super::error::SourceError::Lock

use std::path::PathBuf;

use thiserror::Error;

use super::super::super::error::CorpusError;

/// Errors from parsing, validating, and reading a committed
/// `sources.lock`.
///
/// Every degenerate input fails closed with the context needed to
/// fix it; the gate that failed is recoverable from the variant
/// alone (HLR-106).
#[derive(Debug, Error)]
pub enum SourceLockError {
    /// Gate 1: the committed bytes did not parse under the strict
    /// versioned lock schema — malformed TOML, an unknown field, an
    /// unknown availability or capture-mode tag, a malformed digest,
    /// a digest or capture field on an unavailable entry, or an
    /// external-control table that does not match the capture mode.
    #[error("sources.lock does not parse under the strict schema: {source}")]
    Parse {
        /// Underlying TOML error.
        #[source]
        source: toml::de::Error,
    },
    /// Gate 1: the lock declares a schema newer than this tool
    /// supports.
    #[error(
        "sources.lock declares schema_version {found}; \
         this tool supports up to {supported}"
    )]
    SchemaTooNew {
        /// Declared `schema_version`.
        found: u32,
        /// Highest version this tool loads.
        supported: u32,
    },
    /// Gate 1: one document key appears on more than one entry; the
    /// lock binds one entry per effective document key.
    #[error("sources.lock carries document key {document_key} on more than one entry")]
    DuplicateKey {
        /// The duplicated document key.
        document_key: String,
    },
    /// Gate 2: the committed bytes parse but differ from the
    /// canonical rendering of the parsed value — non-canonical
    /// entry order, field order, whitespace, quoting, comments, or
    /// trailing-newline form, even when the parsed values are
    /// equivalent.
    #[error("sources.lock is not in canonical form: {detail}")]
    NonCanonical {
        /// Where the committed bytes first differ from the canonical
        /// rendering.
        detail: String,
    },
    /// Gate 3: an effective source head of the validated graph has
    /// no entry in the committed lock.
    #[error("sources.lock is missing the entry for effective document key {document_key}")]
    Missing {
        /// The missing entry's document key.
        document_key: String,
    },
    /// Gate 3: the committed lock carries an entry for a document
    /// key that is not an effective source head of the validated
    /// graph.
    #[error(
        "sources.lock carries an entry for document key {document_key}, \
         which is not an effective source head of the corpus graph"
    )]
    Extra {
        /// The extra entry's document key.
        document_key: String,
    },
    /// Gate 3: the committed entry for an effective document key
    /// differs from the derived projection in one bound field.
    #[error("sources.lock entry for document key {document_key} differs in field {field}")]
    Changed {
        /// The document key whose entry changed.
        document_key: String,
        /// The field that differs: `uid`, `availability`, `digest`,
        /// `capture_mode`, or `external_identity`.
        field: &'static str,
    },
    /// The corpus graph failed [`CorpusGraph::validate`] before the
    /// lock gates could run; the [`CorpusError`] is carried as the
    /// typed source (never stringified), so callers can match the
    /// exact broken invariant. Boxed because `CorpusError` is large
    /// enough to trip `result_large_err` on some platforms.
    ///
    /// [`CorpusGraph::validate`]: super::super::super::graph::CorpusGraph::validate
    #[error("corpus graph failed validation: {source}")]
    InvalidGraph {
        /// The validation error, carried whole.
        #[source]
        source: Box<CorpusError>,
    },
    /// The blocking reader could not read the committed lock file.
    #[error("reading sources.lock file {path}")]
    Read {
        /// Lock file path.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
}
