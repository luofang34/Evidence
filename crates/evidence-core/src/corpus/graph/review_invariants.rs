//! Per-node review invariants enforced on every graph, loaded or
//! programmatically built (LLR-115).
//!
//! Runs inside [`CorpusGraph::validate`] after every edge has
//! resolved and endpoint kinds have been checked, and before
//! supersession chain validation, so a malformed review node fails
//! closed instead of poisoning chain walking. Records loaded through
//! `load_reviews_into` satisfy these invariants by construction —
//! the loader emits exactly one `Reviews` edge naming the record's
//! `requirement_uid` and gates `content_schema` — but a
//! programmatically built [`ReviewNode`] carries no such guarantee,
//! so the graph checks each review node directly, in uid-sorted
//! order:
//!
//! 1. Exactly one outgoing `Reviews` edge: zero edges and more than
//!    one are distinct errors.
//! 2. That edge's target equals the node's `requirement_uid` field.
//! 3. `content_schema` equals [`SUPPORTED_REVIEW_CONTENT_SCHEMA`].

use std::path::PathBuf;

use super::super::review_records::SUPPORTED_REVIEW_CONTENT_SCHEMA;
use super::super::review_records::error::ReviewError;
use super::{CorpusGraph, EdgeKind, Node};

/// Enforce the review node/edge/schema invariants on every review
/// node in `graph`.
pub(super) fn validate_review_nodes(graph: &CorpusGraph) -> Result<(), ReviewError> {
    for node in graph.nodes.values() {
        let Node::Review(review) = node else {
            continue;
        };
        let reviews_edges: Vec<&str> = review
            .edges
            .iter()
            .filter(|(kind, _)| *kind == EdgeKind::Reviews)
            .map(|(_, target)| target.as_str())
            .collect();
        let [edge_requirement_uid] = reviews_edges.as_slice() else {
            return Err(if reviews_edges.is_empty() {
                ReviewError::ReviewMissingReviewsEdge {
                    review_uid: review.uid.clone(),
                }
            } else {
                ReviewError::ReviewDuplicateReviewsEdge {
                    review_uid: review.uid.clone(),
                    count: reviews_edges.len(),
                }
            });
        };
        if *edge_requirement_uid != review.requirement_uid {
            return Err(ReviewError::ReviewRequirementEdgeMismatch {
                review_uid: review.uid.clone(),
                field_requirement_uid: review.requirement_uid.clone(),
                edge_requirement_uid: (*edge_requirement_uid).to_string(),
            });
        }
        if review.content_schema != SUPPORTED_REVIEW_CONTENT_SCHEMA {
            return Err(ReviewError::ReviewContentSchema {
                // A programmatically built review node has no record
                // file; the pseudo-path marks that in the message.
                path: PathBuf::from("<graph>"),
                uid: review.uid.clone(),
                id: review.id.clone(),
                found: review.content_schema,
                supported: SUPPORTED_REVIEW_CONTENT_SCHEMA,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "review_invariants/tests.rs"]
mod tests;
