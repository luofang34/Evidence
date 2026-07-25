//! Deterministic per-patch lifecycle evaluation over digest-bound
//! reviews (LLR-173).
//!
//! Every committed curated patch has exactly one effective
//! [`PatchLifecycle`], derived here as a **pure evaluated view**
//! under the same digest and supersession principles as
//! requirements (LLR-117): the state is never stored on a record,
//! never serialized, and never toggled by any caller.
//!
//! # Evaluation
//!
//! [`evaluate_patch_lifecycle`] takes the patch record's current
//! reviewed-content digest — the `evidence/curated-patch/v1`
//! projection (LLR-167) over the bindings, the ordered operations,
//! and every precondition — and collects the *effective review
//! heads*: every review targeting the patch minus any review named
//! as a [`EdgeKind::Supersedes`](super::EdgeKind::Supersedes)
//! target. Because the projection covers content, preconditions,
//! operation order, and the source, recipe, input, and pre-graph
//! bindings, any semantic patch mutation moves the digest and makes
//! an approval stale mechanically. The truth table is the
//! requirement one, shared with [`super::lifecycle::derive_state`]:
//!
//! 1. **Rejected** — any head rejects the current digest.
//!    Rejection is fail-closed and takes precedence over approval,
//!    so a patch with conflicting current decisions is never
//!    approved.
//! 2. **Approved** — no current-digest rejection, and at least one
//!    head approves the current digest.
//! 3. **Stale** — no head decides the current digest, but a head
//!    approves an older digest of the same patch.
//! 4. **Candidate** — otherwise: no reviews at all, or revised
//!    content whose only older decisions were rejections.
//!
//! Reviewer composition is the requirement rule: independent
//! reviewers' heads compose, one reviewer's correction supersedes
//! only their own earlier decision, and `reviewed_at`, reviewer
//! identity, node order, file layout, and record order never enter
//! the decision. Reporting iterates patches in uid order.
//!
//! # Fail-closed boundaries
//!
//! Both public entry points run [`CorpusGraph::validate`] once per
//! call, before any state is derived; a malformed graph fails
//! closed with [`PatchLifecycleError::InvalidGraph`] carrying the
//! [`CorpusError`] as its typed source. The local checks remain as
//! defense in depth: an absent uid with no reviews is
//! [`PatchLifecycleError::PatchMissing`], and a review targeting a
//! missing patch is [`PatchLifecycleError::ReviewTargetsMissingPatch`]
//! — though validation already rejects every such graph (the
//! review's `Reviews` edge dangles). Both entry points are pure:
//! no I/O, no environment reads, no module-level mutable state.

use std::collections::BTreeMap;

use thiserror::Error;

use super::digest::StructuralContentDigest;
use super::error::CorpusError;
use super::graph::{CorpusGraph, Node, ReviewNode, ReviewTarget};
use super::lifecycle::{RequirementLifecycle, derive_state};

/// The one effective lifecycle state of a curated patch
/// (LLR-173) — the requirement lifecycle's four states under the
/// same digest and supersession principles, so one type pins one
/// truth table for every reviewable target kind.
pub type PatchLifecycle = RequirementLifecycle;

/// The evaluated lifecycle of one curated patch (LLR-173).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchLifecycleEvaluation {
    /// Uid of the evaluated patch.
    pub patch_uid: String,
    /// The derived effective state.
    pub state: PatchLifecycle,
    /// The patch record's current reviewed-content digest — the
    /// digest every effective head was compared against.
    pub current_digest: StructuralContentDigest,
    /// Uids of the non-superseded review heads used to reach the
    /// result, sorted (uid order).
    pub effective_review_uids: Vec<String>,
}

/// Errors from patch lifecycle evaluation (LLR-173).
///
/// Evaluation never skips degenerate graph data silently: a
/// malformed graph fails validation with the [`CorpusError`]
/// preserved as the typed source, and the per-patch variants name
/// the patch uid and, where a review is involved, the review uid.
#[derive(Debug, Error)]
pub enum PatchLifecycleError {
    /// The graph failed [`CorpusGraph::validate`] before
    /// evaluation. Both entry points validate the whole graph once
    /// per call and fail closed; the [`CorpusError`] is carried as
    /// the typed source (never stringified), so callers can match
    /// the exact broken invariant. Boxed because `CorpusError` is
    /// large enough to trip `result_large_err` on some platforms;
    /// the source object is carried whole either way.
    #[error("corpus graph failed validation: {0}")]
    InvalidGraph(#[source] Box<CorpusError>),
    /// `evaluate_patch_lifecycle` named a uid with no committed
    /// patch record, and no review targets it either.
    #[error("curated patch {patch_uid} has no committed patch record in the graph")]
    PatchMissing {
        /// The uid that names no committed patch.
        patch_uid: String,
    },
    /// A review targets a patch uid with no committed patch record
    /// — invalid graph data; evaluation fails closed. Whole-graph
    /// validation rejects every such graph first (the review's
    /// `Reviews` edge dangles), so this variant is defense in
    /// depth, unreachable through the public entry points.
    #[error(
        "review {review_uid} targets curated patch {patch_uid}, \
         which has no committed patch record in the graph"
    )]
    ReviewTargetsMissingPatch {
        /// The missing patch's uid.
        patch_uid: String,
        /// The offending review's uid.
        review_uid: String,
    },
}

/// Evaluate the effective lifecycle of one curated patch
/// (LLR-173). The whole graph is validated once, first.
///
/// # Errors
///
/// - [`PatchLifecycleError::InvalidGraph`] when the graph fails
///   [`CorpusGraph::validate`]; the underlying [`CorpusError`] is
///   preserved as the typed source.
/// - [`PatchLifecycleError::PatchMissing`] when `patch_uid` names
///   no committed patch and no review targets it.
/// - [`PatchLifecycleError::ReviewTargetsMissingPatch`] when a
///   review targets `patch_uid` but no committed patch carries
///   that uid (invalid graph data — validation rejects it first;
///   the check is defense in depth).
pub fn evaluate_patch_lifecycle(
    graph: &CorpusGraph,
    patch_uid: &str,
) -> Result<PatchLifecycleEvaluation, PatchLifecycleError> {
    graph
        .validate()
        .map_err(|e| PatchLifecycleError::InvalidGraph(Box::new(e)))?;
    evaluate_patch_lifecycle_validated(graph, patch_uid)
}

/// Evaluate every committed patch in the graph, keyed by patch uid
/// (LLR-173). `BTreeMap` iteration makes reporting order
/// deterministic. The whole graph is validated once per call.
///
/// # Errors
///
/// - [`PatchLifecycleError::InvalidGraph`] when the graph fails
///   [`CorpusGraph::validate`]; the underlying [`CorpusError`] is
///   preserved as the typed source.
///
/// On a valid graph the per-patch checks below cannot fire; they
/// remain as defense in depth (see the module docs).
pub fn evaluate_all_patch_lifecycles(
    graph: &CorpusGraph,
) -> Result<BTreeMap<String, PatchLifecycleEvaluation>, PatchLifecycleError> {
    graph
        .validate()
        .map_err(|e| PatchLifecycleError::InvalidGraph(Box::new(e)))?;
    for node in graph.nodes() {
        if let Node::Review(review) = node
            && let ReviewTarget::CuratedPatch(patch_uid) = &review.target
            && graph.source_patch(patch_uid).is_none()
        {
            return Err(PatchLifecycleError::ReviewTargetsMissingPatch {
                patch_uid: patch_uid.clone(),
                review_uid: review.uid.clone(),
            });
        }
    }
    let mut evaluations = BTreeMap::new();
    for patch_uid in graph.source_patches().keys() {
        let evaluation = evaluate_patch_lifecycle_validated(graph, patch_uid)?;
        evaluations.insert(patch_uid.clone(), evaluation);
    }
    Ok(evaluations)
}

/// Per-patch evaluation on an already-validated graph. The public
/// entry points validate once per call, so the per-patch work
/// never re-validates.
fn evaluate_patch_lifecycle_validated(
    graph: &CorpusGraph,
    patch_uid: &str,
) -> Result<PatchLifecycleEvaluation, PatchLifecycleError> {
    let Some(patch) = graph.source_patch(patch_uid) else {
        return Err(missing_patch_error(graph, patch_uid));
    };
    let current_digest = patch.reviewed_content_digest.clone();
    let superseded = graph.superseded_review_uids();
    let heads: Vec<&ReviewNode> = graph
        .reviews_for_patch(patch_uid)
        .into_iter()
        .filter(|review| !superseded.contains(review.uid.as_str()))
        .collect();
    Ok(PatchLifecycleEvaluation {
        patch_uid: patch_uid.to_string(),
        state: derive_state(&heads, current_digest.as_str()),
        current_digest,
        effective_review_uids: heads.iter().map(|review| review.uid.clone()).collect(),
    })
}

/// A missing patch with reviews targeting it is invalid graph
/// data (fail closed on the uid-ordered first review); without
/// reviews it is simply absent. `reviews_for_patch` iterates in
/// uid order, so the named review is deterministic.
fn missing_patch_error(graph: &CorpusGraph, patch_uid: &str) -> PatchLifecycleError {
    if let Some(review) = graph.reviews_for_patch(patch_uid).first() {
        return PatchLifecycleError::ReviewTargetsMissingPatch {
            patch_uid: patch_uid.to_string(),
            review_uid: review.uid.clone(),
        };
    }
    PatchLifecycleError::PatchMissing {
        patch_uid: patch_uid.to_string(),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "patch_lifecycle/tests.rs"]
mod tests;
