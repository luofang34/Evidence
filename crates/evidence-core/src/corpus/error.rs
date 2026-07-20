//! Typed errors for corpus index parsing, graph construction, and the
//! legacy trace adapter.

use std::path::PathBuf;

use thiserror::Error;

use super::graph::{EdgeKind, NodeKind};

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
        /// Index key of the unsupported kind (e.g. `sources`).
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
    /// A review record's human identifier is empty; every record
    /// needs one for audit cross-reference (LLR-114).
    #[error("review record with uid {uid} in {path} has an empty human id")]
    ReviewHumanId {
        /// Review file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
    },
    /// A review record declares a content schema other than the
    /// supported review-content projection version (LLR-114).
    #[error(
        "review record {id:?} ({uid}) in {path} declares content_schema {found}; \
         this tool supports {supported}"
    )]
    ReviewContentSchema {
        /// Review file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The record's human identifier.
        id: String,
        /// Declared `content_schema`.
        found: u32,
        /// The content schema this tool loads.
        supported: u32,
    },
    /// A review record's `reviewed_at` does not parse as an
    /// RFC 3339 timestamp (LLR-114).
    #[error(
        "review record {id:?} ({uid}) in {path} has reviewed_at {value:?}, \
         which is not an RFC 3339 timestamp"
    )]
    ReviewTimestamp {
        /// Review file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The record's human identifier.
        id: String,
        /// The offending timestamp string.
        value: String,
    },
    /// A review record names an empty reviewer identity; the
    /// reviewer is audit metadata and must be present (LLR-114).
    #[error("review record {id:?} ({uid}) in {path} names an empty reviewer identity")]
    ReviewReviewer {
        /// Review file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The record's human identifier.
        id: String,
    },
    /// A rejected review carries no non-empty rationale; rejections
    /// require one (LLR-114).
    #[error("rejected review record {id:?} ({uid}) in {path} requires a non-empty rationale")]
    ReviewRationale {
        /// Review file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The record's human identifier.
        id: String,
    },
    /// A review supersedes itself (LLR-115).
    #[error("review {uid} supersedes itself")]
    ReviewSupersessionSelf {
        /// The self-referential review's uid.
        uid: String,
    },
    /// A superseding review names a different reviewer than its
    /// predecessor; supersession corrects one reviewer's own
    /// earlier decision (LLR-115).
    #[error(
        "superseding review {uid} names a different reviewer than its predecessor {predecessor_uid}"
    )]
    ReviewSupersessionReviewer {
        /// The superseding review's uid.
        uid: String,
        /// The superseded review's uid.
        predecessor_uid: String,
    },
    /// A superseding review covers a different requirement than its
    /// predecessor (LLR-115).
    #[error(
        "superseding review {uid} covers a different requirement than its predecessor {predecessor_uid}"
    )]
    ReviewSupersessionRequirement {
        /// The superseding review's uid.
        uid: String,
        /// The superseded review's uid.
        predecessor_uid: String,
    },
    /// A superseding review covers different reviewed content than
    /// its predecessor (LLR-115).
    #[error(
        "superseding review {uid} covers different reviewed content than its predecessor {predecessor_uid}"
    )]
    ReviewSupersessionDigest {
        /// The superseding review's uid.
        uid: String,
        /// The superseded review's uid.
        predecessor_uid: String,
    },
    /// A review is superseded by more than one review; supersession
    /// is a chain, not a tree (LLR-115).
    #[error(
        "review {uid} is superseded by both {first_uid} and {second_uid}; \
         a review may be superseded at most once"
    )]
    ReviewSupersessionFork {
        /// The superseded review's uid.
        uid: String,
        /// Uid of the first superseding review (uid order).
        first_uid: String,
        /// Uid of the second superseding review (uid order).
        second_uid: String,
    },
    /// Supersession edges form a cycle (LLR-115).
    #[error("supersession chain cycles back to review {uid}")]
    ReviewSupersessionCycle {
        /// Uid at which the walk revisits a review.
        uid: String,
    },
}
