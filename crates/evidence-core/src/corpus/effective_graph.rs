//! The approval-gated effective structural graph of one source
//! revision (LLR-174).
//!
//! [`effective_source_graph`] produces the committed parser graph
//! of one source revision plus exactly the *currently approved*
//! curated patches bound to that revision, applied in patch uid
//! order through the atomic precondition-checked application
//! contract ([`apply_patch`]). Candidate, rejected, and stale
//! patches never contribute; a malformed patch cannot load at all;
//! and a patch with conflicting current decisions is `Rejected` by
//! the lifecycle truth table's rejection precedence, so it never
//! contributes either. Approval is the only path curated content
//! has into the effective graph — this function is the approval
//! boundary for curated content, and the returned
//! [`EffectiveSourceGraph`] records the applied patch uids as the
//! approval proof.
//!
//! Application chains in deterministic uid order over a working
//! copy: the committed graph is never mutated, and equivalent
//! layouts and load orders produce identical output. Every
//! approved patch binds the same pre-patch parser graph (corpus
//! validation enforces the binding), so two approved patches on
//! one revision cannot both apply — the second fails closed on
//! its stale pre-graph binding rather than silently composing.
//! An approved patch whose recipe, input, or pre-graph binding no
//! longer matches the presented context, whose preconditions have
//! moved, or whose result fails validation fails closed with a
//! typed error: approved curated content is never silently
//! dropped.
//!
//! The whole corpus graph is validated once per call (through
//! patch lifecycle evaluation); a malformed graph fails closed
//! before any application. The function is pure: no I/O, no
//! environment reads, no module-level mutable state.

use thiserror::Error;

use super::graph::CorpusGraph;
use super::patch_lifecycle::{PatchLifecycle, PatchLifecycleError, evaluate_all_patch_lifecycles};
use super::source_graph::SourceGraph;
use super::source_patch::apply::{PatchBindings, apply_patch};
use super::source_patch::error::SourcePatchError;

/// The effective structural graph of one source revision
/// (LLR-174): the committed parser graph with every currently
/// approved curated patch applied in uid order.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveSourceGraph {
    /// The source revision this graph is effective for.
    pub source_revision_uid: String,
    /// The effective graph after approved patch application.
    pub graph: SourceGraph,
    /// Uids of the approved patches that contributed, in
    /// application (patch uid) order — the approval proof for the
    /// effective curated content.
    pub applied_patch_uids: Vec<String>,
}

/// Errors from effective-graph computation (LLR-174).
///
/// Every failure fails closed with the context needed to fix it;
/// no patch is silently skipped.
#[derive(Debug, Error)]
pub enum EffectiveGraphError {
    /// The graph failed [`CorpusGraph::validate`] before
    /// evaluation; the typed source chain through
    /// [`PatchLifecycleError::InvalidGraph`] is preserved whole.
    #[error("patch lifecycle evaluation failed: {0}")]
    Lifecycle(#[from] PatchLifecycleError),
    /// `source_revision_uid` names no committed source revision.
    #[error("source revision {revision_uid} has no committed source revision node")]
    UnknownSourceRevision {
        /// The uid that names no source revision.
        revision_uid: String,
    },
    /// An approved patch failed to apply — a stale binding, a
    /// stale precondition, a dangling target, an identity
    /// collision, or an invalid post-patch graph. Approved curated
    /// content that cannot become effective fails closed rather
    /// than being silently dropped. Boxed to keep the enum under
    /// clippy's `result_large_err` threshold; the source object is
    /// carried whole either way.
    #[error(
        "approved curated patch {patch_uid} cannot contribute to the effective graph: {source}"
    )]
    ApprovedPatchApplication {
        /// The approved patch's uid.
        patch_uid: String,
        /// The application failure.
        #[source]
        source: Box<SourcePatchError>,
    },
}

/// Produce the effective structural graph of
/// `source_revision_uid` (LLR-174): the committed parser graph
/// plus exactly the currently approved patches of that revision,
/// applied in uid order against the presented recipe and input
/// digest `bindings` and the revision's `media_type`.
///
/// # Errors
///
/// - [`EffectiveGraphError::Lifecycle`] when the graph fails
///   validation inside patch lifecycle evaluation — the typed
///   source chain is preserved, never flattened.
/// - [`EffectiveGraphError::UnknownSourceRevision`] when
///   `source_revision_uid` names no committed source revision.
/// - [`EffectiveGraphError::ApprovedPatchApplication`] when an
///   approved patch fails to apply cleanly.
pub fn effective_source_graph(
    graph: &CorpusGraph,
    source_revision_uid: &str,
    bindings: &PatchBindings,
    media_type: &str,
) -> Result<EffectiveSourceGraph, EffectiveGraphError> {
    let evaluations = evaluate_all_patch_lifecycles(graph)?;
    if !graph.nodes().any(
        |node| matches!(node, super::graph::Node::SourceRevision(revision) if revision.uid == source_revision_uid),
    ) {
        return Err(EffectiveGraphError::UnknownSourceRevision {
            revision_uid: source_revision_uid.to_string(),
        });
    }
    let mut working = graph
        .source_graph(source_revision_uid)
        .cloned()
        .unwrap_or_else(SourceGraph::new);
    let mut applied_patch_uids = Vec::new();
    for patch in graph.source_patches().values() {
        if patch.source_revision_uid != source_revision_uid {
            continue;
        }
        let approved = evaluations
            .get(&patch.uid)
            .map(|evaluation| evaluation.state == PatchLifecycle::Approved)
            // `evaluate_all_patch_lifecycles` covers every committed
            // patch, so a missing entry is impossible; treating it
            // as non-approved keeps the gate fail-closed.
            .unwrap_or(false);
        if !approved {
            continue;
        }
        let application = apply_patch(&working, patch, bindings, media_type).map_err(|source| {
            EffectiveGraphError::ApprovedPatchApplication {
                patch_uid: patch.uid.clone(),
                source: Box::new(source),
            }
        })?;
        working = application.graph;
        applied_patch_uids.push(patch.uid.clone());
    }
    Ok(EffectiveSourceGraph {
        source_revision_uid: source_revision_uid.to_string(),
        graph: working,
        applied_patch_uids,
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "effective_graph/tests.rs"]
mod tests;
