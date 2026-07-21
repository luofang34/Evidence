//! Typed errors for review record loading and review graph
//! validation (LLR-114, LLR-115).
//!
//! [`ReviewError`] is the single error type of the review pipeline:
//! `load_reviews_into` fails closed on unreadable/malformed files,
//! newer schemas, invalid uids, invalid record fields, and graph
//! identity collisions, while [`CorpusGraph::validate`] fails closed
//! on malformed review nodes and invalid supersession chains. The
//! record-loading variants mirror their [`CorpusError`] counterparts
//! field for field so a review file and a requirement file report
//! the same degenerate input identically; [`CorpusError::Review`]
//! wraps this type so `CorpusIndex::load_graph` keeps one error
//! type.
//!
//! [`CorpusGraph::validate`]: super::super::graph::CorpusGraph::validate

use std::path::PathBuf;

use thiserror::Error;

use super::super::error::CorpusError;
use super::super::graph::{EdgeKind, NodeKind};

/// Errors from loading review records or validating review nodes
/// and supersession chains in the corpus graph.
///
/// Every degenerate input fails closed with the context needed to
/// fix it; nothing is silently skipped (HLR-079, HLR-080).
#[derive(Debug, Error)]
pub enum ReviewError {
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
    /// A review record's human identifier is empty; every record
    /// needs one for audit cross-reference (LLR-114).
    #[error("review record with uid {uid} in {path} has an empty human id")]
    ReviewHumanId {
        /// Review file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
    },
    /// A review declares a content schema other than the supported
    /// review-content projection version — rejected at record load
    /// and again as a graph invariant for programmatically built
    /// graphs (LLR-114).
    #[error(
        "review record {id:?} ({uid}) in {path} declares content_schema {found}; \
         this tool supports {supported}"
    )]
    ReviewContentSchema {
        /// Review file path; `"<graph>"` when the review node was
        /// built programmatically rather than loaded from a file.
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
    /// A review carries no `Reviews` edge; every review must decide
    /// on exactly one requirement's content (LLR-115).
    #[error("review {review_uid} has no Reviews edge to its reviewed requirement")]
    ReviewMissingReviewsEdge {
        /// The malformed review's uid.
        review_uid: String,
    },
    /// A review carries more than one `Reviews` edge; every review
    /// must decide on exactly one requirement's content (LLR-115).
    #[error(
        "review {review_uid} has {count} Reviews edges; \
         a review decides on exactly one requirement"
    )]
    ReviewDuplicateReviewsEdge {
        /// The malformed review's uid.
        review_uid: String,
        /// Number of `Reviews` edges the node declares.
        count: usize,
    },
    /// A review's `requirement_uid` field disagrees with the target
    /// of its `Reviews` edge; both name the reviewed requirement and
    /// must match (LLR-115).
    #[error(
        "review {review_uid} names requirement {field_requirement_uid} but its \
         Reviews edge targets {edge_requirement_uid}"
    )]
    ReviewRequirementEdgeMismatch {
        /// The malformed review's uid.
        review_uid: String,
        /// The node's `requirement_uid` field.
        field_requirement_uid: String,
        /// The node's `Reviews` edge target.
        edge_requirement_uid: String,
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

impl ReviewError {
    /// Lift a [`CorpusGraph::insert`] failure into the review error
    /// type. `insert` fails only on identity collisions and duplicate
    /// edges, so those are the only variants that can reach the
    /// review loader.
    ///
    /// [`CorpusGraph::insert`]: super::super::graph::CorpusGraph::insert
    pub(super) fn from_insert(err: CorpusError) -> Self {
        match err {
            CorpusError::DuplicateUid { uid } => ReviewError::DuplicateUid { uid },
            CorpusError::DuplicateHumanId {
                id,
                kind,
                first_uid,
                duplicate_uid,
            } => ReviewError::DuplicateHumanId {
                id,
                kind,
                first_uid,
                duplicate_uid,
            },
            CorpusError::DuplicateEdge { from, to, kind } => {
                ReviewError::DuplicateEdge { from, to, kind }
            }
            other => unreachable!(
                "CorpusGraph::insert fails only on identity collisions and duplicate edges: {other:?}"
            ),
        }
    }
}
