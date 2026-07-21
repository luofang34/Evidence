//! General edge and identity validation for the corpus graph.
//!
//! Insert-time checks reject duplicate identities — uid globally,
//! human id within a node kind — and canonicalize outgoing edges
//! (sorted, duplicate-free) so graph equality and derived views are
//! independent of input edge order (HLR-080). `validate`-time checks
//! resolve every edge and enforce each edge kind's source/target
//! endpoint contract. Review-specific invariants and supersession
//! chain validation live in their own sibling modules and run after
//! these general checks inside [`CorpusGraph::validate`].

use std::collections::BTreeMap;

use super::super::error::CorpusError;
use super::{CorpusGraph, EdgeKind, Node, NodeKind};

/// Reject inserting `node` when its uid is already present in
/// `nodes`, or when its human id repeats within its node kind. The
/// uid check runs first so a fully duplicate node reports the uid,
/// and the human-id error names both the existing and the duplicate
/// uid so a loader can point at both offending records.
pub(super) fn check_identity_uniqueness(
    nodes: &BTreeMap<String, Node>,
    node: &Node,
) -> Result<(), CorpusError> {
    let uid = node.uid();
    if nodes.contains_key(uid) {
        return Err(CorpusError::DuplicateUid {
            uid: uid.to_string(),
        });
    }
    if let Some(existing) = nodes
        .values()
        .find(|existing| existing.kind() == node.kind() && existing.id() == node.id())
    {
        return Err(CorpusError::DuplicateHumanId {
            id: node.id().to_string(),
            kind: node.kind(),
            first_uid: existing.uid().to_string(),
            duplicate_uid: uid.to_string(),
        });
    }
    Ok(())
}

/// Sort `node`'s outgoing edges and reject duplicates, so graph
/// equality and derived views are independent of input edge order.
pub(super) fn canonicalize_edges(node: &mut Node) -> Result<(), CorpusError> {
    let from = node.uid().to_string();
    let edges = node.edges_mut();
    edges.sort();
    if let Some(pair) = edges.windows(2).find(|pair| pair[0] == pair[1]) {
        return Err(CorpusError::DuplicateEdge {
            from,
            to: pair[0].1.clone(),
            kind: pair[0].0,
        });
    }
    Ok(())
}

/// Check every edge in `graph` resolves to a present node and obeys
/// its source/target endpoint-kind contract.
pub(super) fn validate_edges(graph: &CorpusGraph) -> Result<(), CorpusError> {
    for node in graph.nodes.values() {
        for (kind, target) in node.edges() {
            let target_node = graph
                .nodes
                .get(target)
                .ok_or_else(|| CorpusError::DanglingEdge {
                    from: node.uid().to_string(),
                    to: target.clone(),
                    kind: *kind,
                })?;
            if !edge_kinds_match(node.kind(), *kind, target_node.kind()) {
                return Err(CorpusError::InvalidEdgeKinds {
                    from: node.uid().to_string(),
                    to: target.clone(),
                    kind: *kind,
                    source_kind: node.kind(),
                    target_kind: target_node.kind(),
                });
            }
        }
    }
    Ok(())
}

/// The endpoint-kind contract per edge kind: a requirement derives
/// from a requirement, a test verifies a requirement, a review
/// decides on a requirement and supersedes a review (LLR-115), and
/// a source revision supersedes a source revision (LLR-129).
fn edge_kinds_match(source: NodeKind, edge: EdgeKind, target: NodeKind) -> bool {
    matches!(
        (source, edge, target),
        (
            NodeKind::Requirement,
            EdgeKind::DerivesFrom,
            NodeKind::Requirement
        ) | (NodeKind::Test, EdgeKind::Verifies, NodeKind::Requirement)
            | (NodeKind::Review, EdgeKind::Reviews, NodeKind::Requirement)
            | (NodeKind::Review, EdgeKind::Supersedes, NodeKind::Review)
            | (
                NodeKind::SourceRevision,
                EdgeKind::Supersedes,
                NodeKind::SourceRevision
            )
    )
}
