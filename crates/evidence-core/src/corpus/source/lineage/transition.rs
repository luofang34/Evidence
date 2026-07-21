//! The pure immutable-superset source baseline transition and the
//! typed projection it compares (LLR-132).
//!
//! [`validate_source_transition`] compares a prior graph with a
//! proposed graph, scoped to the source-revision subgraph so
//! non-source nodes never affect the outcome: the proposed
//! subgraph must be a UID-preserving superset whose retained
//! revisions keep a byte-for-byte-equal [`SourceRevisionProjection`],
//! and a new revision of an existing `document_key` must extend
//! the prior effective head. Both graphs are validated inside the
//! call, so an `Ok` transition implies two valid graphs. The
//! function performs no I/O and reads no environment.

use super::super::super::graph::{CorpusGraph, Node, SourceMaterial, SourceRevisionNode};
use super::super::error::SourceError;
use super::{effective_source_heads, supersedes_target};

/// The canonical typed immutable projection of one source
/// revision: every [`SourceRevisionNode`] field plus its owned
/// source-supersession edge (LLR-132). Two revisions of one uid
/// must project byte-for-byte equal across a baseline transition;
/// any difference is a typed mutation. Derived `PartialEq` compares
/// typed values, never serialized text.
///
/// `supersedes` carries the single owned `Supersedes` target; a
/// validated graph carries at most one per revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRevisionProjection {
    /// Permanent identity, unique across all node kinds.
    pub uid: String,
    /// Human-readable revision identifier.
    pub id: String,
    /// Stable lineage key of the logical document.
    pub document_key: String,
    /// One-line title.
    pub title: String,
    /// RFC 6838 `type/subtype` media type.
    pub media_type: String,
    /// Canonical location, preserved exactly.
    pub canonical_location: String,
    /// Typed material state: retrieval timestamp, digest, and
    /// capture identity for available material; the reason for
    /// unavailable material.
    pub material: SourceMaterial,
    /// Target uid of the owned `Supersedes` edge, if any.
    pub supersedes: Option<String>,
}

impl SourceRevisionProjection {
    /// Project one source-revision node into its immutable
    /// comparison form.
    pub fn project(node: &SourceRevisionNode) -> Self {
        Self {
            uid: node.uid.clone(),
            id: node.id.clone(),
            document_key: node.document_key.clone(),
            title: node.title.clone(),
            media_type: node.media_type.clone(),
            canonical_location: node.canonical_location.clone(),
            material: node.material.clone(),
            supersedes: supersedes_target(node).map(str::to_string),
        }
    }
}

/// Compare a prior source baseline with a proposed one as a pure
/// function of both graphs, scoped to the source-revision subgraph
/// (LLR-132). Non-source nodes never affect the outcome.
///
/// The prior graph is validated first — the trusted baseline —
/// then every prior source revision must remain present with a
/// byte-for-byte-equal [`SourceRevisionProjection`]: a missing
/// revision is [`SourceError::SourceTransitionRemoval`], and any
/// projection difference is
/// [`SourceError::SourceTransitionMutation`] naming the revision
/// and the differing field, so a replaced digest is a mutation,
/// never an update. A new revision whose `document_key` existed in
/// the prior baseline must own a `Supersedes` edge to that
/// document's prior effective head or fail as
/// [`SourceError::SourceTransitionCompetingHead`]; new document
/// keys are free. This check runs before proposed-side validation
/// so an unrelated competing head reports the precise typed error;
/// the proposed graph itself is validated last, so an `Ok`
/// transition implies two valid graphs.
///
/// # Errors
///
/// - [`SourceError::SourceTransitionInvalidGraph`] when either
///   graph fails [`CorpusGraph::validate`]; the
///   [`CorpusError`](super::super::super::error::CorpusError) is
///   carried as the typed source.
/// - [`SourceError::SourceTransitionRemoval`] when a prior source
///   revision is absent from the proposed graph.
/// - [`SourceError::SourceTransitionMutation`] when a retained
///   revision's immutable projection differs.
/// - [`SourceError::SourceTransitionCompetingHead`] when a new
///   revision of an existing document key does not extend the
///   prior effective head.
pub fn validate_source_transition(
    prior: &CorpusGraph,
    proposed: &CorpusGraph,
) -> Result<(), SourceError> {
    prior
        .validate()
        .map_err(|source| SourceError::SourceTransitionInvalidGraph {
            graph: "prior",
            source: Box::new(source),
        })?;
    let prior_heads = effective_source_heads(prior);
    for node in prior.nodes() {
        let Node::SourceRevision(prior_revision) = node else {
            continue;
        };
        let uid = prior_revision.uid.as_str();
        let Some(proposed_node) = proposed.get(uid) else {
            return Err(SourceError::SourceTransitionRemoval {
                uid: uid.to_string(),
            });
        };
        let Node::SourceRevision(proposed_revision) = proposed_node else {
            return Err(SourceError::SourceTransitionMutation {
                uid: uid.to_string(),
                field: "kind",
            });
        };
        if let Some(field) = projection_diff(
            &SourceRevisionProjection::project(prior_revision),
            &SourceRevisionProjection::project(proposed_revision),
        ) {
            return Err(SourceError::SourceTransitionMutation {
                uid: uid.to_string(),
                field,
            });
        }
    }
    for node in proposed.nodes() {
        let Node::SourceRevision(new_revision) = node else {
            continue;
        };
        if prior.get(new_revision.uid.as_str()).is_some() {
            continue;
        }
        let Some(prior_head_uid) = prior_heads.get(new_revision.document_key.as_str()) else {
            // A new document key is free: no prior lineage to extend.
            continue;
        };
        if supersedes_target(new_revision) != Some(prior_head_uid.as_str()) {
            return Err(SourceError::SourceTransitionCompetingHead {
                uid: new_revision.uid.clone(),
                document_key: new_revision.document_key.clone(),
                prior_head_uid: prior_head_uid.clone(),
            });
        }
    }
    proposed
        .validate()
        .map_err(|source| SourceError::SourceTransitionInvalidGraph {
            graph: "proposed",
            source: Box::new(source),
        })?;
    Ok(())
}

/// The first differing projection field between two revisions of
/// one uid, in declaration order, or `None` when the projections
/// are equal. The reported name feeds
/// [`SourceError::SourceTransitionMutation`].
fn projection_diff(
    prior: &SourceRevisionProjection,
    proposed: &SourceRevisionProjection,
) -> Option<&'static str> {
    if prior.id != proposed.id {
        return Some("id");
    }
    if prior.document_key != proposed.document_key {
        return Some("document_key");
    }
    if prior.title != proposed.title {
        return Some("title");
    }
    if prior.media_type != proposed.media_type {
        return Some("media_type");
    }
    if prior.canonical_location != proposed.canonical_location {
        return Some("canonical_location");
    }
    if let Some(field) = material_diff(&prior.material, &proposed.material) {
        return Some(field);
    }
    if prior.supersedes != proposed.supersedes {
        return Some("supersedes");
    }
    None
}

/// The first differing field inside two material states, or
/// `None` when they are equal. A state flip between available and
/// unavailable reports `material` itself; within one state the
/// differing sub-field is named.
fn material_diff(prior: &SourceMaterial, proposed: &SourceMaterial) -> Option<&'static str> {
    match (prior, proposed) {
        (
            SourceMaterial::Available {
                retrieved_at: prior_at,
                sha256: prior_sha256,
                capture: prior_capture,
            },
            SourceMaterial::Available {
                retrieved_at: proposed_at,
                sha256: proposed_sha256,
                capture: proposed_capture,
            },
        ) => {
            if prior_at != proposed_at {
                Some("retrieved_at")
            } else if prior_sha256 != proposed_sha256 {
                Some("sha256")
            } else if prior_capture != proposed_capture {
                Some("capture")
            } else {
                None
            }
        }
        (
            SourceMaterial::Unavailable {
                reason: prior_reason,
            },
            SourceMaterial::Unavailable {
                reason: proposed_reason,
            },
        ) => (prior_reason != proposed_reason).then_some("reason"),
        _ => Some("material"),
    }
}
