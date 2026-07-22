//! Structural source-graph invariant validation (LLR-157).
//!
//! Corpus graph validation runs these invariants after
//! source-revision lineage, once every source revision and every
//! source-graph file has loaded. Validation is read-only and
//! fails fast in a documented order, so the reported error is
//! deterministic:
//!
//! 1. **Binding** — every source graph's revision uid names a
//!    committed source-revision node.
//! 2. **Per node, in uid order** — the parent chain resolves
//!    inside the same revision (a parent absent everywhere is
//!    dangling; a parent present only in another revision is
//!    cross-revision), the chain reaches a root without
//!    revisiting a node (cycles), the parent/child kind pair is
//!    legal under the closed table, the stored content digest
//!    and fingerprint recompute from the kind, canonical text,
//!    label, and ancestry, and the locator variant agrees with
//!    the revision's media type.
//! 3. **Sibling ordinals** — per parent set, ordinals are unique
//!    and contiguous `0..n` under canonical sibling ordering.
//!
//! Every failure is a flat typed [`SourceGraphError`] variant
//! carrying the source revision, the node uid, the field, and
//! the conflicting values.

use std::collections::{BTreeMap, BTreeSet};

use super::super::graph::{CorpusGraph, Node};
use super::error::SourceGraphError;
use super::normalization::{content_digest, fingerprint};
use super::{SourceGraph, SourceNode, SourceNodeKind};

/// Validate every committed source graph in `graph` against the
/// committed source revisions. See the module docs for the check
/// order.
pub(crate) fn validate_source_graphs(graph: &CorpusGraph) -> Result<(), SourceGraphError> {
    let media_map = revision_media_map(graph);
    let uid_index = global_uid_index(graph);
    for (revision_uid, source_graph) in graph.source_graphs() {
        let Some(media_type) = media_map.get(revision_uid.as_str()) else {
            return Err(SourceGraphError::UnknownSourceRevision {
                revision_uid: revision_uid.clone(),
            });
        };
        for node in source_graph.nodes() {
            validate_node(revision_uid, source_graph, &uid_index, media_type, node)?;
        }
        validate_ordinals(revision_uid, source_graph)?;
    }
    Ok(())
}

/// Map every committed source-revision uid to its declared media
/// type.
fn revision_media_map(graph: &CorpusGraph) -> BTreeMap<String, String> {
    graph
        .nodes()
        .filter_map(|node| match node {
            Node::SourceRevision(revision) => {
                Some((revision.uid.clone(), revision.media_type.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Map every committed source-node uid to the revision it belongs
/// to. Uids legitimately recur across revisions of one document;
/// on a collision the lowest revision uid wins (revision graphs
/// iterate in revision-uid order), so cross-revision diagnostics
/// are deterministic.
fn global_uid_index(graph: &CorpusGraph) -> BTreeMap<&str, &str> {
    let mut index = BTreeMap::new();
    for (revision_uid, source_graph) in graph.source_graphs() {
        for node in source_graph.nodes() {
            index
                .entry(node.uid.as_str())
                .or_insert(revision_uid.as_str());
        }
    }
    index
}

/// Run the per-node checks in the module docs' order: ancestry
/// (dangling, cross-revision, cycle), kind legality, content
/// digest, fingerprint, and locator media agreement.
fn validate_node(
    revision_uid: &str,
    graph: &SourceGraph,
    uid_index: &BTreeMap<&str, &str>,
    media_type: &str,
    node: &SourceNode,
) -> Result<(), SourceGraphError> {
    let ancestry = walk_ancestry(revision_uid, graph, uid_index, node)?;
    if let Some(parent) = ancestry.last() {
        if !parent.kind.may_parent(node.kind) {
            return Err(SourceGraphError::IllegalParentKind {
                revision_uid: revision_uid.to_string(),
                node_uid: node.uid.clone(),
                kind: node.kind,
                parent_uid: parent.uid.clone(),
                parent_kind: parent.kind,
            });
        }
    }
    let recomputed_content = content_digest(node.kind, &node.canonical_text);
    if recomputed_content != node.content_sha256 {
        return Err(SourceGraphError::DigestMismatch {
            revision_uid: revision_uid.to_string(),
            node_uid: node.uid.clone(),
            field: "content_sha256",
            expected: recomputed_content.as_str().to_string(),
            actual: node.content_sha256.as_str().to_string(),
        });
    }
    let ancestry_inputs: Vec<(SourceNodeKind, Option<&str>)> = ancestry
        .iter()
        .map(|ancestor| (ancestor.kind, ancestor.label.as_deref()))
        .collect();
    let recomputed_fingerprint = fingerprint(node.kind, node.label.as_deref(), &ancestry_inputs);
    if recomputed_fingerprint != node.fingerprint {
        return Err(SourceGraphError::DigestMismatch {
            revision_uid: revision_uid.to_string(),
            node_uid: node.uid.clone(),
            field: "fingerprint",
            expected: recomputed_fingerprint.as_str().to_string(),
            actual: node.fingerprint.as_str().to_string(),
        });
    }
    if !node
        .locator
        .expected_media_type()
        .eq_ignore_ascii_case(media_type)
    {
        return Err(SourceGraphError::LocatorMediaMismatch {
            revision_uid: revision_uid.to_string(),
            node_uid: node.uid.clone(),
            locator_format: node.locator.format_str(),
            media_type: media_type.to_string(),
        });
    }
    Ok(())
}

/// Walk `node`'s parent chain to a root, returning the ancestors
/// ordered root to parent. A parent absent from this revision is
/// dangling — or cross-revision when the global index places it
/// in another revision — and a revisited node closes a cycle.
fn walk_ancestry<'g>(
    revision_uid: &str,
    graph: &'g SourceGraph,
    uid_index: &BTreeMap<&str, &str>,
    node: &'g SourceNode,
) -> Result<Vec<&'g SourceNode>, SourceGraphError> {
    let mut chain = Vec::new();
    let mut visited = BTreeSet::from([node.uid.as_str()]);
    let mut current = node;
    while let Some(parent_uid) = &current.parent_uid {
        let Some(parent) = graph.get(parent_uid) else {
            return Err(match uid_index.get(parent_uid.as_str()) {
                Some(parent_revision) => SourceGraphError::CrossRevisionParent {
                    revision_uid: revision_uid.to_string(),
                    node_uid: current.uid.clone(),
                    parent_uid: parent_uid.clone(),
                    parent_revision_uid: (*parent_revision).to_string(),
                },
                None => SourceGraphError::DanglingParent {
                    revision_uid: revision_uid.to_string(),
                    node_uid: current.uid.clone(),
                    parent_uid: parent_uid.clone(),
                },
            });
        };
        if !visited.insert(parent.uid.as_str()) {
            return Err(SourceGraphError::Cycle {
                revision_uid: revision_uid.to_string(),
                node_uid: parent.uid.clone(),
            });
        }
        chain.push(parent);
        current = parent;
    }
    chain.reverse();
    Ok(chain)
}

/// Validate every sibling set: ordinals are unique and contiguous
/// `0..n` under canonical sibling ordering (ordinal, then uid).
/// Runs after per-node validation, so every parent link has
/// already resolved inside the revision.
fn validate_ordinals(revision_uid: &str, graph: &SourceGraph) -> Result<(), SourceGraphError> {
    let mut sets: BTreeMap<Option<&str>, Vec<&SourceNode>> = BTreeMap::new();
    for node in graph.nodes() {
        sets.entry(node.parent_uid.as_deref())
            .or_default()
            .push(node);
    }
    for (parent_uid, mut siblings) in sets {
        siblings.sort_by(|a, b| (a.ordinal, &a.uid).cmp(&(b.ordinal, &b.uid)));
        let mut previous: Option<&SourceNode> = None;
        for (expected, sibling) in siblings.into_iter().enumerate() {
            if let Some(prev) = previous {
                if sibling.ordinal == prev.ordinal {
                    return Err(SourceGraphError::DuplicateOrdinal {
                        revision_uid: revision_uid.to_string(),
                        parent_uid: parent_uid.map(str::to_string),
                        ordinal: sibling.ordinal,
                        first_uid: prev.uid.clone(),
                        duplicate_uid: sibling.uid.clone(),
                    });
                }
            }
            if sibling.ordinal != expected as u32 {
                return Err(SourceGraphError::NonContiguousOrdinals {
                    revision_uid: revision_uid.to_string(),
                    parent_uid: parent_uid.map(str::to_string),
                    expected: expected as u32,
                    found: sibling.ordinal,
                    node_uid: sibling.uid.clone(),
                });
            }
            previous = Some(sibling);
        }
    }
    Ok(())
}
