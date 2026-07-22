//! Typed errors for corpus index parsing, graph construction, and the
//! legacy trace adapter. Review record loading and review graph
//! validation report through [`ReviewError`], wrapped here by
//! [`CorpusError::Review`] (LLR-114, LLR-115); the proposal store
//! reports through [`ProposalError`], wrapped here by
//! [`CorpusError::Proposal`] (LLR-122, LLR-123, LLR-124); source
//! record loading reports through [`SourceError`], wrapped here by
//! [`CorpusError::Source`] (LLR-125); source-graph record loading
//! and structural validation report through [`SourceGraphError`],
//! wrapped here by [`CorpusError::SourceGraph`] (LLR-152, LLR-157).

use std::path::PathBuf;

use thiserror::Error;

use super::graph::{EdgeKind, NodeKind};
use super::proposal::ProposalError;
use super::review_records::error::ReviewError;
use super::source::error::SourceError;
use super::source_graph::error::SourceGraphError;

/// Errors from loading, building, or validating the corpus graph.
///
/// Every degenerate input fails closed with the context needed to fix
/// it; nothing is silently skipped (HLR-079, HLR-080).
#[derive(Debug, Error)]
pub enum CorpusError {
    /// Failed to read the `corpus.toml` index file.
    #[error("reading corpus index {path}")]
    IndexRead {
        /// Index path as given by the caller.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// The `corpus.toml` index did not parse (malformed TOML or an
    /// unknown field — the index schema is strict).
    #[error("parsing corpus index {path}")]
    IndexParse {
        /// Index path as given by the caller.
        path: PathBuf,
        /// Underlying TOML error.
        #[source]
        source: toml::de::Error,
    },
    /// The index declares a schema newer than this tool supports; the
    /// tool refuses to load rather than skipping unknown structure.
    #[error(
        "corpus index {path} declares schema_version {found}; \
         this tool supports up to {supported}"
    )]
    IndexSchemaTooNew {
        /// Index path as given by the caller.
        path: PathBuf,
        /// Declared `schema_version`.
        found: u32,
        /// Highest version this tool loads.
        supported: u32,
    },
    /// An indexed path or `<dir>/**/*.toml` pattern resolved to no
    /// files — an index that names nothing is a configuration error,
    /// not an empty corpus.
    #[error("corpus index entry {entry:?} resolved to no files")]
    EmptyIndexEntry {
        /// The literal path or pattern as written in the index.
        entry: String,
    },
    /// The index lists entries for a node kind outside the supported
    /// record schemas.
    #[error("corpus index lists unsupported {kind} entries")]
    UnsupportedKind {
        /// Index key of the unsupported kind (e.g. `source_graphs`).
        kind: &'static str,
    },
    /// Failed walking a `<dir>/**/*.toml` index pattern.
    #[error("walking corpus index pattern under {dir}")]
    PatternWalk {
        /// Directory the pattern roots at.
        dir: PathBuf,
        /// Underlying walk error.
        #[source]
        source: walkdir::Error,
    },
    /// Failed to read a corpus record file named by the index.
    #[error("reading corpus record file {path}")]
    RecordRead {
        /// Record file path.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// A corpus record file did not parse (malformed TOML or an
    /// unknown field — record schemas are strict).
    #[error("parsing corpus record file {path}")]
    RecordParse {
        /// Record file path.
        path: PathBuf,
        /// Underlying TOML error.
        #[source]
        source: toml::de::Error,
    },
    /// A record file declares a schema newer than this tool supports.
    #[error(
        "corpus record file {path} declares schema_version {found}; \
         this tool supports up to {supported}"
    )]
    RecordSchemaTooNew {
        /// Record file path.
        path: PathBuf,
        /// Declared `schema_version`.
        found: u32,
        /// Highest version this tool loads.
        supported: u32,
    },
    /// A corpus-native record's uid lacks its kind's typed prefix.
    #[error("corpus-native uid {uid:?} must start with {expected:?}")]
    NativeUidPrefix {
        /// The offending uid.
        uid: String,
        /// Required prefix for the record's kind.
        expected: &'static str,
    },
    /// A corpus-native uid suffix is not an RFC 9562 UUIDv4.
    #[error("corpus-native uid {uid:?} must end with an RFC 9562 UUIDv4")]
    NativeUidUuidV4 {
        /// The offending uid.
        uid: String,
    },
    /// Two nodes claimed the same uid; uids are unique across all
    /// node kinds.
    #[error("duplicate corpus uid {uid}")]
    DuplicateUid {
        /// The colliding uid.
        uid: String,
    },
    /// Two nodes of the same kind claimed the same human identifier.
    #[error("duplicate {kind:?} id {id:?}: first uid {first_uid}, duplicate uid {duplicate_uid}")]
    DuplicateHumanId {
        /// The colliding human identifier.
        id: String,
        /// Node kind within which the identifier must be unique.
        kind: NodeKind,
        /// Uid of the node inserted first.
        first_uid: String,
        /// Uid of the rejected node.
        duplicate_uid: String,
    },
    /// One node declared the same typed edge more than once.
    #[error("duplicate {kind:?} edge from {from} to {to}")]
    DuplicateEdge {
        /// Uid of the node that owns the edge.
        from: String,
        /// Duplicate target uid.
        to: String,
        /// Edge kind.
        kind: EdgeKind,
    },
    /// An edge points at a uid absent from the graph.
    #[error("dangling {kind:?} edge from {from} to missing node {to}")]
    DanglingEdge {
        /// Uid of the node that owns the edge.
        from: String,
        /// Missing target uid.
        to: String,
        /// Edge kind.
        kind: EdgeKind,
    },
    /// An edge's source or target node kind violates its typed
    /// endpoint contract.
    #[error("invalid {kind:?} edge from {from} ({source_kind:?}) to {to} ({target_kind:?})")]
    InvalidEdgeKinds {
        /// Uid of the node that owns the edge.
        from: String,
        /// Target uid.
        to: String,
        /// Edge kind.
        kind: EdgeKind,
        /// Actual source node kind.
        source_kind: NodeKind,
        /// Actual target node kind.
        target_kind: NodeKind,
    },
    /// A review record failed to load or a review graph invariant
    /// failed validation (LLR-114, LLR-115). Display forwards to the
    /// wrapped review error unchanged.
    #[error(transparent)]
    Review(#[from] ReviewError),
    /// A proposal store operation failed (LLR-122, LLR-123,
    /// LLR-124). Display forwards to the wrapped proposal error
    /// unchanged.
    #[error(transparent)]
    Proposal(#[from] ProposalError),
    /// A source-revision record failed to load (LLR-125). Display
    /// forwards to the wrapped source error unchanged.
    #[error(transparent)]
    Source(#[from] SourceError),
    /// A source-graph record failed to load or a structural
    /// source-graph invariant failed validation (LLR-152,
    /// LLR-157). Display forwards to the wrapped source-graph
    /// error unchanged.
    #[error(transparent)]
    SourceGraph(#[from] SourceGraphError),
    /// A legacy trace entry has no uid; every corpus node requires a
    /// permanent identity, so the adapter refuses rather than skips.
    #[error("legacy trace entry {id} has no uid; corpus nodes require one")]
    LegacyMissingUid {
        /// Human-readable id of the offending entry.
        id: String,
    },
    /// A review-content digest string is not exactly 64 lowercase
    /// hexadecimal characters; malformed digests fail closed at
    /// construction boundaries (LLR-112).
    #[error(
        "invalid review content digest {input:?}: expected exactly 64 lowercase hexadecimal characters"
    )]
    InvalidDigest {
        /// The offending digest string.
        input: String,
    },
}
