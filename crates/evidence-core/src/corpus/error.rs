//! Typed errors for corpus index parsing, graph construction, and the
//! legacy trace adapter. Review record loading and review graph
//! validation report through [`ReviewError`], wrapped here by
//! [`CorpusError::Review`] (LLR-114, LLR-115).

use std::path::PathBuf;

use thiserror::Error;

use super::digest::ReviewContentDigest;
use super::graph::{EdgeKind, NodeKind};
use super::lifecycle::RequirementLifecycle;
use super::review_records::error::ReviewError;

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
    /// A review record failed to load or a review graph invariant
    /// failed validation (LLR-114, LLR-115). Display forwards to the
    /// wrapped review error unchanged.
    #[error(transparent)]
    Review(#[from] ReviewError),
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
    /// store fails closed at construction (LLR-123).
    #[error("proposal root {path} does not exist")]
    ProposalRootMissing {
        /// The offending root path.
        path: PathBuf,
    },
    /// A proposal root exists but is not a directory (LLR-123).
    #[error("proposal root {path} is not a directory")]
    ProposalRootNotADirectory {
        /// The offending root path.
        path: PathBuf,
    },
    /// A proposal root is a symlink; roots must be real directories
    /// so no write can escape through a link (LLR-123).
    #[error("proposal root {path} is a symlink")]
    ProposalRootSymlink {
        /// The offending root path.
        path: PathBuf,
    },
    /// Failed to read a proposal record file (LLR-122).
    #[error("reading proposal file {path}")]
    ProposalRead {
        /// Proposal file path.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// Failed to create or write a proposal record file; any
    /// partial file is removed best-effort (LLR-123).
    #[error("writing proposal file {path}")]
    ProposalWrite {
        /// Proposal file path.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// Serializing a proposal record failed before any file was
    /// created (LLR-123).
    #[error("serializing proposal for {path}")]
    ProposalSerialize {
        /// Proposal file path the record was bound for.
        path: PathBuf,
        /// Underlying TOML error.
        #[source]
        source: toml::ser::Error,
    },
    /// A proposal record file did not parse — malformed TOML, an
    /// unknown field, an unknown action tag, or a malformed digest;
    /// malformed and partially written proposals fail closed
    /// (LLR-122).
    #[error("parsing proposal file {path}")]
    ProposalParse {
        /// Proposal file path.
        path: PathBuf,
        /// Underlying TOML error.
        #[source]
        source: toml::de::Error,
    },
    /// A proposal file declares a schema newer than this tool
    /// supports (LLR-122).
    #[error(
        "proposal file {path} declares schema_version {found}; \
         this tool supports up to {supported}"
    )]
    ProposalSchema {
        /// Proposal file path.
        path: PathBuf,
        /// Declared `schema_version`.
        found: u32,
        /// Highest version this tool loads.
        supported: u32,
    },
    /// A proposal names an empty submitter identity; the submitter
    /// is audit metadata and must be present (LLR-122).
    #[error("proposal {uid} in {path} names an empty submitter identity")]
    ProposalSubmitter {
        /// Proposal file path (the would-be path at append time).
        path: PathBuf,
        /// The record's uid.
        uid: String,
    },
    /// A proposal's `submitted_at` does not parse as an RFC 3339
    /// timestamp (LLR-122).
    #[error(
        "proposal {uid} in {path} has submitted_at {value:?}, \
         which is not an RFC 3339 timestamp"
    )]
    ProposalTimestamp {
        /// Proposal file path.
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The offending timestamp string.
        value: String,
    },
    /// A revision proposal targets a uid with no requirement node
    /// in the graph (LLR-124).
    #[error(
        "revision proposal targets requirement {uid}, \
         which has no requirement node in the graph"
    )]
    ProposalTargetMissing {
        /// The missing target uid.
        uid: String,
    },
    /// A revision proposal targets a requirement whose evaluated
    /// lifecycle is not candidate; a proposal can never demote
    /// approved content (LLR-124). `state` is never `Candidate`.
    #[error(
        "revision proposal targets requirement {uid} whose lifecycle is {state:?}; \
         only candidate requirements can be revised"
    )]
    ProposalLifecycle {
        /// The rejected target uid.
        uid: String,
        /// The evaluated lifecycle state: `Approved`, `Rejected`,
        /// or `Stale`.
        state: RequirementLifecycle,
    },
    /// A revision proposal's expected digest does not equal the
    /// requirement's current review-content digest — optimistic
    /// concurrency fails closed (LLR-124).
    #[error(
        "revision proposal targets requirement {uid}: expected current digest \
         {expected} but the current digest is {actual}"
    )]
    ProposalDigestMismatch {
        /// The target uid.
        uid: String,
        /// The digest the submitter supplied.
        expected: ReviewContentDigest,
        /// The requirement's current review-content digest.
        actual: ReviewContentDigest,
    },
    /// Exclusive creation found an existing file at the generated
    /// proposal path; an existing proposal is never overwritten
    /// (LLR-123).
    #[error("proposal file {path} already exists; proposals are never overwritten")]
    ProposalExists {
        /// The colliding proposal file path.
        path: PathBuf,
    },
}
