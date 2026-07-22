//! Typed corpus graph over linked TOML files — the v0.2 data model.
//!
//! Certification artifacts load from files indexed by `cert/corpus.toml`
//! into one uid-keyed, typed graph; traceability reports are derived
//! views of that graph, and file layout carries no semantic meaning
//! (SYS-035). Legacy `cert/trace` documents load into the same graph
//! through [`graph_from_trace_files`] with the same identities and
//! edge sets.
//!
//! Module map:
//!
//! - `index` — `corpus.toml` parsing + per-kind file resolution
//! - `graph` — node/edge types and the uid-keyed graph
//! - `records` — corpus-native record file schemas
//! - `review_records` — corpus-native human review decision records
//!   (LLR-114)
//! - `source` — corpus-native frozen source-revision records
//!   (LLR-125)
//! - `source::lock` — the canonical derived `sources.lock`
//!   inventory of effective source heads (LLR-133)
//! - `source::verify` — offline verification of the material
//!   behind each effective source head (LLR-136)
//! - `source_graph` — the parser-independent committed
//!   structural source graph: `snode_` identity, canonical text
//!   and digests, closed locators, forest invariants, and
//!   structural-key reconciliation (LLR-156)
//! - `legacy` — four-file `cert/trace` → graph adapter
//! - `review_content` — versioned canonical projection of the
//!   normative content a review approves (LLR-111)
//! - `digest` — typed lowercase SHA-256 digest domains (LLR-112,
//!   LLR-125, LLR-155)
//! - `lifecycle` — deterministic per-requirement lifecycle
//!   evaluation over digest-bound review heads (LLR-117)
//! - `approval_boundary` — strict validation that implementation and
//!   verification evidence claims only approved requirements under
//!   explicit lifecycle enforcement (LLR-119)
//! - `proposal` — append-only candidate-proposal store; the one
//!   agent-facing write capability (LLR-122)
//!
//! Design record:
//! `docs/superpowers/specs/2026-07-18-corpus-model-v0.2-design.md`.

mod approval_boundary;
mod digest;
mod error;
mod graph;
mod index;
mod legacy;
mod lifecycle;
mod proposal;
mod records;
mod review_content;
mod review_records;
mod source;
mod source_graph;

pub use approval_boundary::{
    ApprovalBoundaryError, ApprovalBoundaryViolation, LifecycleEnforcement, ReferringArtifact,
    validate_approval_boundary,
};
pub use digest::{ReviewContentDigest, SourceContentDigest, StructuralContentDigest};
pub use error::CorpusError;
pub(crate) use graph::TraceMetadata;
pub use graph::{
    CorpusGraph, EdgeKind, Node, NodeKind, RequirementLayer, RequirementNode, ReviewDecision,
    ReviewNode, SourceCapture, SourceMaterial, SourceRevisionNode, TestNode,
};
pub use index::{CorpusIndex, SUPPORTED_INDEX_SCHEMA};
pub use legacy::graph_from_trace_files;
pub(crate) use legacy::graph_from_trace_parts;
pub use lifecycle::{
    LifecycleError, LifecycleEvaluation, RequirementLifecycle, evaluate_all_lifecycles,
    evaluate_lifecycle,
};
pub use proposal::{
    AppendOutcome, PROPOSAL_UID_PREFIX, ProposalAction, ProposalError, ProposalFile,
    ProposalFileDigest, ProposalRecord, ProposalStore, ProposedRequirementContent,
    SUPPORTED_PROPOSAL_SCHEMA,
};
pub use review_content::{
    RequirementReviewContentV1, canonical_bytes_v1, review_content_digest_v1,
};
pub use review_records::error::ReviewError;
pub use source::error::{SourceError, VendoredPathRule};
pub use source::lineage::{
    SourceRevisionProjection, effective_source_heads, validate_source_lineage,
    validate_source_transition,
};
pub use source::lock::{
    ExternalControlId, LockCapture, LockMaterial, SUPPORTED_LOCK_SCHEMA, SourceLock,
    SourceLockEntry, SourceLockError, derive_lock, parse_lock, read_lock_blocking,
    render_lock_canonical, validate_committed_lock, validate_lock_file_blocking,
};
pub use source::verify::{
    DigestMismatchDetail, SourcePayloadError, SourceVerification, SourceVerificationState,
    verify_effective_sources,
};
pub use source_graph::error::SourceGraphError;
pub use source_graph::identity::{
    CandidateNode, ReconciledNode, StructuralKey, mint_node_uid, reconcile, structural_key,
};
pub use source_graph::locator::{LocatorRule, SafeRelPath, SourceLocator};
pub use source_graph::normalization::{
    content_digest, fingerprint, normalize_code, normalize_prose,
};
pub use source_graph::records::{
    SNODE_UID_PREFIX, SUPPORTED_SOURCE_GRAPH_SCHEMA, SourceGraphFile, SourceNodeRecord,
};
pub use source_graph::render::render_source_graph_canonical;
pub use source_graph::{SourceGraph, SourceNode, SourceNodeKind};

#[cfg(test)]
mod tests;
