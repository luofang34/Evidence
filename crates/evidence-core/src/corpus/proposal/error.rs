//! Typed errors for the append-only candidate-proposal store
//! (LLR-122, LLR-123, LLR-124).
//!
//! [`ProposalError`] is the single error type of the proposal
//! pipeline: store construction, appends, strict read-back,
//! revision guards, and semantic content validation all fail
//! closed with a typed variant carrying the context needed to fix
//! the input. [`CorpusError::Proposal`](super::super::CorpusError::Proposal)
//! wraps this type so mixed corpus call sites keep one error type,
//! mirroring the [`ReviewError`](super::super::ReviewError) split.

use std::path::PathBuf;

use thiserror::Error;

use super::super::digest::ReviewContentDigest;
use super::super::lifecycle::{LifecycleError, RequirementLifecycle};

/// Errors from proposal store construction, appends, read-back,
/// and revision guards.
///
/// Every degenerate input fails closed with the context needed to
/// fix it; nothing is silently skipped (HLR-079, HLR-080).
#[derive(Debug, Error)]
pub enum ProposalError {
    /// The proposal root does not exist or cannot be resolved; the
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
    /// The revision guard could not evaluate the target's
    /// lifecycle at all — the graph itself is malformed (LLR-124).
    /// The original [`LifecycleError`] is carried as the typed
    /// source, never stringified, so callers can match the exact
    /// broken invariant underneath. It is boxed because
    /// [`LifecycleError::InvalidGraph`] wraps [`CorpusError`], which
    /// wraps this type: the recursion needs indirection.
    ///
    /// [`CorpusError`]: super::super::CorpusError
    #[error("revision proposal guard could not evaluate the target lifecycle: {0}")]
    ProposalLifecycleEvaluation(#[source] Box<LifecycleError>),
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
    /// A corpus-native uid in a proposal lacks its kind's typed
    /// prefix; mirrors
    /// [`CorpusError::NativeUidPrefix`](super::super::CorpusError::NativeUidPrefix)
    /// so proposal validation reports the same degenerate input
    /// identically (LLR-122).
    #[error("corpus-native uid {uid:?} must start with {expected:?}")]
    NativeUidPrefix {
        /// The offending uid.
        uid: String,
        /// Required prefix for the record's kind.
        expected: &'static str,
    },
    /// A corpus-native uid suffix in a proposal is not an RFC 9562
    /// UUIDv4; mirrors
    /// [`CorpusError::NativeUidUuidV4`](super::super::CorpusError::NativeUidUuidV4)
    /// (LLR-122).
    #[error("corpus-native uid {uid:?} must end with an RFC 9562 UUIDv4")]
    NativeUidUuidV4 {
        /// The offending uid.
        uid: String,
    },
    /// A proposal's title is empty or whitespace-only; every
    /// candidate needs a one-line title (LLR-122).
    #[error("proposal {uid} in {path} has an empty title")]
    ProposalContentTitle {
        /// Proposal file path (the would-be path at append time).
        path: PathBuf,
        /// The record's uid.
        uid: String,
    },
    /// A proposal lists the same `derives_from` target twice; the
    /// canonical form is duplicate-free (LLR-122).
    #[error("proposal {uid} in {path} lists derives_from target {target} more than once")]
    ProposalContentDerivesFrom {
        /// Proposal file path (the would-be path at append time).
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The duplicated target uid.
        target: String,
    },
    /// A proposal's verification methods are not sorted as written;
    /// a proposal must arrive in canonical form (LLR-122).
    #[error(
        "proposal {uid} in {path} lists verification methods out of order: \
         {first:?} precedes {second:?}"
    )]
    ProposalContentVerificationMethodsOrder {
        /// Proposal file path (the would-be path at append time).
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The earlier element of the first out-of-order pair.
        first: String,
        /// The later element of the first out-of-order pair.
        second: String,
    },
    /// A proposal lists the same verification method twice; the
    /// canonical form is duplicate-free (LLR-122).
    #[error("proposal {uid} in {path} lists verification method {method:?} more than once")]
    ProposalContentVerificationMethodsDuplicate {
        /// Proposal file path (the would-be path at append time).
        path: PathBuf,
        /// The record's uid.
        uid: String,
        /// The duplicated method.
        method: String,
    },
    /// A proposal file digest string is not exactly 64 lowercase
    /// hexadecimal characters; malformed digests fail closed at
    /// construction boundaries (LLR-123).
    #[error(
        "invalid proposal file digest {input:?}: expected exactly 64 lowercase hexadecimal characters"
    )]
    InvalidFileDigest {
        /// The offending digest string.
        input: String,
    },
}
