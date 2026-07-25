//! The flat typed error taxonomy of re-ingestion drift comparison
//! (LLR-176).
//!
//! Every variant is a *prerequisite* failure: the comparison
//! refuses to run before any finding is computed. Per-item
//! malformation inside otherwise valid planes — one bad candidate
//! patch record, one unappliable approved patch — degrades to a
//! finding instead (LLR-178); only broken planes refuse.
//!
//! The family is deliberately uncoded (no
//! [`crate::diagnostic::DiagnosticCode`] impl), following the
//! [`super::CorpusError`] precedent: comparison runs inside flows
//! that already own the diagnostic surface.

use thiserror::Error;

use super::super::effective_graph::EffectiveGraphError;
use super::super::error::CorpusError;
use super::super::source_graph::error::SourceGraphError;

/// Every fail-closed prerequisite failure of
/// [`compare_reingestion`](super::compare_reingestion) (LLR-176).
#[derive(Debug, Error)]
pub enum DriftError {
    /// The committed corpus graph failed [`CorpusGraph::validate`]
    /// before comparison; the [`CorpusError`] is carried whole as
    /// the typed source.
    #[error("committed baseline corpus graph failed validation: {0}")]
    InvalidBaseline(#[source] Box<CorpusError>),
    /// `source_revision_uid` names no committed source revision
    /// node, so no media type or baseline plane exists.
    #[error("source revision {revision_uid} has no committed source revision node")]
    UnknownSourceRevision {
        /// The uid that names no source revision.
        revision_uid: String,
    },
    /// A candidate parser-graph node binds a source revision other
    /// than the compared one.
    #[error(
        "candidate parser graph node {node_uid} binds source revision {found}, \
         not the compared revision {revision_uid}"
    )]
    CandidateRevisionMismatch {
        /// The compared revision.
        revision_uid: String,
        /// The offending candidate node.
        node_uid: String,
        /// The revision the node actually binds.
        found: String,
    },
    /// The candidate parser graph failed the standalone
    /// source-graph validator under the revision's media type; the
    /// [`SourceGraphError`] is carried whole as the typed source.
    #[error("candidate parser graph failed standalone validation: {0}")]
    InvalidCandidateGraph(#[source] Box<SourceGraphError>),
    /// A patch of one plane has no review evaluation on that
    /// plane — the review plane presented is incomplete.
    #[error("{plane} patch {patch_uid} has no review evaluation")]
    MissingPatchEvaluation {
        /// The plane missing the evaluation: `committed` or
        /// `candidate`.
        plane: &'static str,
        /// The unevaluated patch's uid.
        patch_uid: String,
    },
    /// The committed effective graph cannot be produced from the
    /// validated baseline — an approved committed patch fails
    /// against its own committed plane. The candidate side never
    /// reaches this variant: its per-patch failures degrade to
    /// findings. The [`EffectiveGraphError`] is carried whole as
    /// the typed source.
    #[error("committed effective graph cannot be produced: {0}")]
    InvalidEffectiveBaseline(#[source] Box<EffectiveGraphError>),
}
