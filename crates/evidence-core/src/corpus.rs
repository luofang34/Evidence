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
//! - `legacy` — four-file `cert/trace` → graph adapter
//! - `review_content` — versioned canonical projection of the
//!   normative content a review approves (LLR-111)
//! - `digest` — typed lowercase SHA-256 review-content digest
//!   (LLR-112)
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

pub use approval_boundary::{
    ApprovalBoundaryError, ApprovalBoundaryViolation, LifecycleEnforcement, ReferringArtifact,
    validate_approval_boundary,
};
pub use digest::ReviewContentDigest;
pub use error::CorpusError;
pub(crate) use graph::TraceMetadata;
pub use graph::{
    CorpusGraph, EdgeKind, Node, NodeKind, RequirementLayer, RequirementNode, ReviewDecision,
    ReviewNode, TestNode,
};
pub use index::{CorpusIndex, SUPPORTED_INDEX_SCHEMA};
pub use legacy::graph_from_trace_files;
pub(crate) use legacy::graph_from_trace_parts;
pub use lifecycle::{
    LifecycleError, LifecycleEvaluation, RequirementLifecycle, evaluate_all_lifecycles,
    evaluate_lifecycle,
};
pub use proposal::{
    AppendOutcome, PROPOSAL_UID_PREFIX, ProposalAction, ProposalFile, ProposalRecord,
    ProposalStore, ProposedRequirementContent, SUPPORTED_PROPOSAL_SCHEMA,
};
pub use review_content::{
    RequirementReviewContentV1, canonical_bytes_v1, review_content_digest_v1,
};
pub use review_records::error::ReviewError;

#[cfg(test)]
mod tests;
