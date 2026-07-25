//! Corpus-level validation of the committed curated-patch plane
//! (LLR-169).
//!
//! Corpus graph validation runs these checks after the committed
//! structural source graphs validate, once every source revision,
//! source-graph file, and curated-patch file has loaded.
//! Validation is read-only and fails fast in patch uid order, so
//! the reported error is deterministic:
//!
//! 1. **Binding** — the patch's source-revision uid names a
//!    committed source-revision node, and the pre-patch graph
//!    digest recomputes from that revision's committed parser
//!    graph (an empty committed graph digests as the canonical
//!    rendering of zero nodes).
//! 2. **Targets** — every non-insert operation target and every
//!    parent context resolves inside the bound revision's
//!    committed graph or among the patch's own inserted nodes; a
//!    target present only in another revision is a cross-document
//!    edit, a target another patch inserts is an implicit
//!    cross-patch cascade, and a target resolving nowhere is
//!    dangling.
//!
//! Deeper semantic checks — stale preconditions, post-patch graph
//! validity — are candidate-application concerns (`apply`
//! module); committed-plane validation guarantees only that every
//! committed patch is well-bound and well-targeted against the
//! committed parser graphs.

use std::collections::BTreeMap;

use super::super::graph::{CorpusGraph, Node};
use super::PatchOperation;
use super::digest::source_graph_digest;
use super::error::SourcePatchError;
use super::records::SourcePatchRecord;

/// Validate every committed curated patch against the committed
/// source revisions and parser graphs. See the module docs for
/// the check order.
pub(crate) fn validate_source_patches(graph: &CorpusGraph) -> Result<(), SourcePatchError> {
    for patch in graph.source_patches().values() {
        validate_binding(graph, patch)?;
        validate_targets(graph, patch)?;
    }
    Ok(())
}

/// Check the revision binding and the pre-patch graph digest.
fn validate_binding(
    graph: &CorpusGraph,
    patch: &SourcePatchRecord,
) -> Result<(), SourcePatchError> {
    let revision_known = graph.nodes().any(
        |node| matches!(node, Node::SourceRevision(revision) if revision.uid == patch.source_revision_uid),
    );
    if !revision_known {
        return Err(SourcePatchError::UnknownSourceRevision {
            patch_uid: patch.uid.clone(),
            revision_uid: patch.source_revision_uid.clone(),
        });
    }
    let empty;
    let parser_graph = match graph.source_graph(&patch.source_revision_uid) {
        Some(committed) => committed,
        None => {
            empty = super::super::source_graph::SourceGraph::new();
            &empty
        }
    };
    let recomputed = source_graph_digest(parser_graph);
    if recomputed != patch.pre_patch_graph_digest {
        return Err(SourcePatchError::PrePatchGraphDigestMismatch {
            patch_uid: patch.uid.clone(),
            expected: recomputed.as_str().to_string(),
            actual: patch.pre_patch_graph_digest.as_str().to_string(),
        });
    }
    Ok(())
}

/// Check every operation target and parent context resolves
/// against the bound revision, the patch's own inserted nodes,
/// and no other plane.
fn validate_targets(
    graph: &CorpusGraph,
    patch: &SourcePatchRecord,
) -> Result<(), SourcePatchError> {
    let inserted: Vec<&str> = patch
        .operations
        .iter()
        .filter_map(|operation| match operation {
            PatchOperation::Insert { node, .. } => Some(node.uid.as_str()),
            _ => None,
        })
        .collect();
    let other_inserted: BTreeMap<&str, &str> = graph
        .source_patches()
        .values()
        .filter(|other| other.uid != patch.uid)
        .flat_map(|other| {
            other
                .operations
                .iter()
                .filter_map(|operation| match operation {
                    PatchOperation::Insert { node, .. } => {
                        Some((node.uid.as_str(), other.uid.as_str()))
                    }
                    _ => None,
                })
        })
        .collect();
    for operation in &patch.operations {
        for target in operation_targets(operation) {
            if inserted.contains(&target) {
                continue;
            }
            match locate(graph, target) {
                Some(revision) if revision == patch.source_revision_uid => {}
                Some(revision) => {
                    return Err(SourcePatchError::CrossRevisionTarget {
                        patch_uid: patch.uid.clone(),
                        target_uid: target.to_string(),
                        target_revision_uid: revision,
                    });
                }
                None => {
                    if let Some(other_patch) = other_inserted.get(target) {
                        return Err(SourcePatchError::CrossPatchTarget {
                            patch_uid: patch.uid.clone(),
                            target_uid: target.to_string(),
                            other_patch_uid: (*other_patch).to_string(),
                        });
                    }
                    return Err(SourcePatchError::DanglingTarget {
                        patch_uid: patch.uid.clone(),
                        target_uid: target.to_string(),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Every uid an operation targets or parents under. An insert's
/// node uid is not a target — it is the node the patch creates.
fn operation_targets(operation: &PatchOperation) -> Vec<&str> {
    match operation {
        PatchOperation::ReplaceContent { target_uid, .. }
        | PatchOperation::Reclassify { target_uid, .. }
        | PatchOperation::Remove { target_uid, .. } => vec![target_uid.as_str()],
        PatchOperation::Reparent {
            target_uid,
            expected_parent_uid,
            new_parent_uid,
            ..
        } => {
            let mut targets = vec![target_uid.as_str()];
            targets.extend(expected_parent_uid.as_deref());
            targets.extend(new_parent_uid.as_deref());
            targets
        }
        PatchOperation::Insert {
            expected_parent_uid,
            ..
        } => expected_parent_uid
            .as_deref()
            .map_or_else(Vec::new, |parent| vec![parent]),
    }
}

/// The revision a node uid belongs to, when it exists in any
/// committed source graph. Uids legitimately recur across
/// revisions of one document; on a collision the lowest revision
/// uid wins (revision graphs iterate in revision-uid order), so
/// cross-revision diagnostics are deterministic.
fn locate(graph: &CorpusGraph, uid: &str) -> Option<String> {
    for (revision_uid, source_graph) in graph.source_graphs() {
        if source_graph.get(uid).is_some() {
            return Some(revision_uid.clone());
        }
    }
    None
}
