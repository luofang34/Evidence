//! Deterministic per-requirement lifecycle evaluation over
//! digest-bound reviews (LLR-117, LLR-118).
//!
//! Every requirement has exactly one effective
//! [`RequirementLifecycle`], derived here as a **pure evaluated
//! view**: the state is never stored on a node, never serialized
//! into a record, and never toggled by any caller. A content
//! change turns an approval stale mechanically — the digest
//! comparison alone decides, and the repository history remains
//! the audit trail for the reviewed bytes.
//!
//! # Evaluation
//!
//! [`evaluate_lifecycle`] computes the requirement's current
//! review-content digest ([`review_content_digest_v1`]) and
//! collects the *effective review heads*: every review of the
//! requirement minus any review named as a
//! [`EdgeKind::Supersedes`](super::EdgeKind::Supersedes) target.
//! Only heads decide; a superseded review contributes nothing.
//! The truth table over heads is:
//!
//! 1. **Rejected** — any head rejects the current digest.
//!    Rejection is fail-closed and takes precedence over approval.
//! 2. **Approved** — no current-digest rejection, and at least one
//!    head approves the current digest.
//! 3. **Stale** — no head decides the current digest, but a head
//!    approves an older digest of the same requirement. A
//!    well-formed approval whose digest differs from the current
//!    content produces at most Stale, never Approved.
//! 4. **Candidate** — otherwise: no reviews at all, or revised
//!    content whose only older decisions were rejections. An
//!    older-digest rejection never stigmatizes new content.
//!
//! `reviewed_at`, reviewer identity, node order, file layout, and
//! file order never enter the decision; equivalent graphs evaluate
//! identically on any host. Reporting iterates requirements in uid
//! order ([`evaluate_all_lifecycles`] returns a `BTreeMap`).
//!
//! # Fail-closed boundaries
//!
//! Both public entry points run [`CorpusGraph::validate`] once per
//! call, before any state is derived: a malformed graph — a
//! dangling or kind-mismatched edge, a review whose
//! `requirement_uid` field disagrees with its `Reviews` edge or
//! whose `content_schema` differs from the supported projection
//! version, or supersession malformation (self, dangling, cycle,
//! fork, cross-reviewer/requirement/digest) — fails closed with
//! [`LifecycleError::InvalidGraph`]. That variant carries the
//! [`CorpusError`] as its typed source, never a string, so callers
//! can match the exact broken invariant, including
//! [`CorpusError::Review`] wrapping a
//! [`ReviewError`](super::ReviewError). No malformed review graph
//! can produce a lifecycle state.
//!
//! The cost model is one full-graph validation pass per public
//! call: [`evaluate_all_lifecycles`] validates once, not once per
//! requirement, and a caller whose loader already validated pays
//! the same single pass again — the public boundary owns the
//! guarantee, so no caller can skip it.
//!
//! Per-requirement evaluation then runs on a graph whose
//! invariants hold, so the evaluator compares digests only and
//! needs no runtime schema handling. The local fail-closed checks
//! remain as defense in depth: an absent uid with no reviews is
//! [`LifecycleError::RequirementMissing`], and a review targeting
//! a missing requirement is
//! [`LifecycleError::ApprovalTargetsMissingRequirement`] — though
//! validation already rejects every such graph (the review's
//! `Reviews` edge dangles), so that variant cannot be reached
//! through the public entry points.
//!
//! Both entry points are pure functions of the graph: no I/O, no
//! environment reads, no module-level mutable state.

use std::collections::BTreeMap;

use thiserror::Error;

use super::digest::ReviewContentDigest;
use super::error::CorpusError;
use super::graph::{CorpusGraph, Node, ReviewDecision, ReviewNode};
use super::review_content::review_content_digest_v1;

/// The one effective lifecycle state of a requirement (LLR-117).
///
/// This is an evaluated value: it is computed from digest-bound
/// review heads on every call and is never stored on
/// [`RequirementNode`](super::RequirementNode) or serialized into
/// any record. `as_str` returns the stable wire string for
/// reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequirementLifecycle {
    /// No effective decision binds the current digest and no
    /// effective approval exists for an older digest — a new
    /// requirement, or revised content whose only older decisions
    /// were rejections.
    Candidate,
    /// At least one effective approval binds the current digest
    /// and no effective rejection binds it.
    Approved,
    /// An effective rejection binds the current digest. Fail-closed:
    /// takes precedence over any approval.
    Rejected,
    /// No effective decision binds the current digest, but an
    /// effective approval exists for an older digest of the same
    /// requirement — the content moved after approval.
    Stale,
}

impl RequirementLifecycle {
    /// The stable lowercase wire string for this state
    /// (`"candidate"` / `"approved"` / `"rejected"` / `"stale"`).
    pub fn as_str(self) -> &'static str {
        match self {
            RequirementLifecycle::Candidate => "candidate",
            RequirementLifecycle::Approved => "approved",
            RequirementLifecycle::Rejected => "rejected",
            RequirementLifecycle::Stale => "stale",
        }
    }
}

/// The evaluated lifecycle of one requirement (LLR-117).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleEvaluation {
    /// Uid of the evaluated requirement.
    pub requirement_uid: String,
    /// The derived effective state.
    pub state: RequirementLifecycle,
    /// Digest of the requirement's current review content — the
    /// digest every effective head was compared against.
    pub current_digest: ReviewContentDigest,
    /// Uids of the non-superseded review heads used to reach the
    /// result, sorted (uid order).
    pub effective_review_uids: Vec<String>,
}

/// Errors from lifecycle evaluation (LLR-118).
///
/// Evaluation never skips degenerate graph data silently: a
/// malformed graph fails validation with the [`CorpusError`]
/// preserved as the typed source, and the per-requirement variants
/// name the requirement uid and, where a review is involved, the
/// review uid.
#[derive(Debug, Error)]
pub enum LifecycleError {
    /// The graph failed [`CorpusGraph::validate`] before
    /// evaluation. Both entry points validate the whole graph once
    /// per call and fail closed; the [`CorpusError`] is carried as
    /// the typed source (never stringified), so callers can match
    /// the exact broken invariant — including
    /// [`CorpusError::Review`] wrapping a
    /// [`ReviewError`](super::ReviewError). Boxed because
    /// `CorpusError` is large enough to trip `result_large_err` on
    /// some platforms; the source object is carried whole either
    /// way.
    #[error("corpus graph failed validation: {0}")]
    InvalidGraph(#[source] Box<CorpusError>),
    /// `evaluate_lifecycle` named a uid with no requirement node
    /// in the graph (absent, or naming another node kind), and no
    /// review targets it either.
    #[error("requirement {requirement_uid} has no requirement node in the graph")]
    RequirementMissing {
        /// The uid that names no requirement node.
        requirement_uid: String,
    },
    /// A review targets a requirement uid with no requirement node
    /// in the graph — invalid graph data; evaluation fails closed.
    /// Whole-graph validation rejects every such graph first (the
    /// review's `Reviews` edge dangles), so this variant is
    /// defense in depth, unreachable through the public entry
    /// points.
    #[error(
        "review {review_uid} targets requirement {requirement_uid}, \
         which has no requirement node in the graph"
    )]
    ApprovalTargetsMissingRequirement {
        /// The missing requirement's uid.
        requirement_uid: String,
        /// The offending review's uid.
        review_uid: String,
    },
}

/// Evaluate the effective lifecycle of one requirement (LLR-117).
///
/// The whole graph is validated once, first; per-requirement
/// evaluation runs only on a graph whose invariants hold.
///
/// # Errors
///
/// - [`LifecycleError::InvalidGraph`] when the graph fails
///   [`CorpusGraph::validate`]; the underlying [`CorpusError`] is
///   preserved as the typed source.
/// - [`LifecycleError::RequirementMissing`] when `requirement_uid`
///   names no requirement node and no review targets it.
/// - [`LifecycleError::ApprovalTargetsMissingRequirement`] when a
///   review targets `requirement_uid` but no requirement node
///   carries that uid (invalid graph data — validation rejects it
///   first; the check is defense in depth).
pub fn evaluate_lifecycle(
    graph: &CorpusGraph,
    requirement_uid: &str,
) -> Result<LifecycleEvaluation, LifecycleError> {
    graph
        .validate()
        .map_err(|e| LifecycleError::InvalidGraph(Box::new(e)))?;
    evaluate_lifecycle_validated(graph, requirement_uid)
}

/// Evaluate every requirement in the graph, keyed by requirement
/// uid (LLR-118). `BTreeMap` iteration makes reporting order
/// deterministic.
///
/// The whole graph is validated once per call — not once per
/// requirement — before any evaluation.
///
/// # Errors
///
/// - [`LifecycleError::InvalidGraph`] when the graph fails
///   [`CorpusGraph::validate`]; the underlying [`CorpusError`] is
///   preserved as the typed source.
///
/// On a valid graph the per-requirement checks below cannot fire;
/// they remain as defense in depth (see the module docs).
pub fn evaluate_all_lifecycles(
    graph: &CorpusGraph,
) -> Result<BTreeMap<String, LifecycleEvaluation>, LifecycleError> {
    graph
        .validate()
        .map_err(|e| LifecycleError::InvalidGraph(Box::new(e)))?;
    for node in graph.nodes() {
        if let Node::Review(review) = node
            && !matches!(
                graph.get(&review.requirement_uid),
                Some(Node::Requirement(_))
            )
        {
            return Err(LifecycleError::ApprovalTargetsMissingRequirement {
                requirement_uid: review.requirement_uid.clone(),
                review_uid: review.uid.clone(),
            });
        }
    }
    let mut evaluations = BTreeMap::new();
    for node in graph.nodes() {
        if let Node::Requirement(requirement) = node {
            let evaluation = evaluate_lifecycle_validated(graph, &requirement.uid)?;
            evaluations.insert(requirement.uid.clone(), evaluation);
        }
    }
    Ok(evaluations)
}

/// Per-requirement evaluation on an already-validated graph. The
/// public entry points validate once per call, so the per-node
/// work never re-validates.
fn evaluate_lifecycle_validated(
    graph: &CorpusGraph,
    requirement_uid: &str,
) -> Result<LifecycleEvaluation, LifecycleError> {
    let Some(content) = graph.review_content(requirement_uid) else {
        return Err(missing_requirement_error(graph, requirement_uid));
    };
    let current_digest = review_content_digest_v1(&content);
    let superseded = graph.superseded_review_uids();
    let heads: Vec<&ReviewNode> = graph
        .reviews_for_requirement(requirement_uid)
        .into_iter()
        .filter(|review| !superseded.contains(review.uid.as_str()))
        .collect();
    Ok(LifecycleEvaluation {
        requirement_uid: requirement_uid.to_string(),
        state: derive_state(&heads, &current_digest),
        current_digest,
        effective_review_uids: heads.iter().map(|review| review.uid.clone()).collect(),
    })
}

/// The truth table over effective heads: current-digest rejection
/// beats current-digest approval beats older-digest approval beats
/// nothing (LLR-117). Older-digest rejections decide nothing.
fn derive_state(
    heads: &[&ReviewNode],
    current_digest: &ReviewContentDigest,
) -> RequirementLifecycle {
    let mut current_rejection = false;
    let mut current_approval = false;
    let mut older_approval = false;
    for head in heads {
        let binds_current = head.reviewed_content_sha256 == *current_digest;
        match head.decision {
            ReviewDecision::Reject if binds_current => current_rejection = true,
            ReviewDecision::Approve if binds_current => current_approval = true,
            ReviewDecision::Approve => older_approval = true,
            ReviewDecision::Reject => {}
        }
    }
    if current_rejection {
        RequirementLifecycle::Rejected
    } else if current_approval {
        RequirementLifecycle::Approved
    } else if older_approval {
        RequirementLifecycle::Stale
    } else {
        RequirementLifecycle::Candidate
    }
}

/// A missing requirement with reviews targeting it is invalid
/// graph data (fail closed on the uid-ordered first review);
/// without reviews it is simply absent. `reviews_for_requirement`
/// iterates in uid order, so the named review is deterministic.
fn missing_requirement_error(graph: &CorpusGraph, requirement_uid: &str) -> LifecycleError {
    if let Some(review) = graph.reviews_for_requirement(requirement_uid).first() {
        return LifecycleError::ApprovalTargetsMissingRequirement {
            requirement_uid: requirement_uid.to_string(),
            review_uid: review.uid.clone(),
        };
    }
    LifecycleError::RequirementMissing {
        requirement_uid: requirement_uid.to_string(),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "lifecycle/adversarial_tests.rs"]
mod adversarial_tests;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "lifecycle/fixtures.rs"]
mod fixtures;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "lifecycle/tests.rs"]
mod tests;
