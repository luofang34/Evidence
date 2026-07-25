//! Atomic precondition-checked candidate application of a curated
//! patch (LLR-168).
//!
//! [`apply_patch`] is a pure function over the bound revision's
//! committed parser graph, the patch record, the recipe and input
//! digest bindings, and the revision's media type. It performs no
//! I/O, no fetch, no workspace mutation, and no baseline
//! replacement; the committed graph is never touched — application
//! works on a clone and returns the candidate graph as a
//! separately inspectable plane.
//!
//! The contract runs in a documented fail-fast order so the
//! reported error is deterministic:
//!
//! 1. **Parser-graph validation** — the complete standalone
//!    source-graph validator runs over the pre-patch graph; a
//!    patch never applies over an invalid parser graph.
//! 2. **Binding checks** — the presented recipe and input digests
//!    and the recomputed pre-patch canonical graph digest must
//!    match the patch record exactly; a stale binding fails
//!    closed before any operation.
//! 3. **Operations in canonical ordinal order** — each operation
//!    checks its exact preconditions against the working state
//!    (expected content digest, kind, parent, or ordinal) and
//!    fails closed with a typed stale-precondition error when the
//!    state has moved; dangling targets and inserted-identity
//!    collisions fail with their own typed errors.
//! 4. **Post-patch validation** — stored node digests are
//!    recomputed from the final kind, text, label, and ancestry,
//!    then the complete source-graph validator runs over the
//!    result; an invalid post-graph fails closed.
//!
//! Application is atomic: any failure discards the working copy,
//! and the original graph is byte-identical afterwards — the
//! function never mutates its input. The returned
//! [`PatchApplication`] records the pre-patch digest, the patch
//! reviewed-content digest, the post-patch digest, and the
//! candidate graph. It carries no review-lifecycle or approval
//! semantics; the approval-gated effective committed graph is a
//! later milestone's concern.

use std::collections::{BTreeMap, BTreeSet};

use super::super::digest::StructuralContentDigest;
use super::super::source_graph::normalization::{content_digest, fingerprint};
use super::super::source_graph::validate::validate_graph_standalone;
use super::super::source_graph::{SourceGraph, SourceNode, SourceNodeKind};
use super::digest::source_graph_digest;
use super::error::SourcePatchError;
use super::records::SourcePatchRecord;
use super::{ChildDisposition, PatchOperation};

/// The recipe and input digest bindings presented at candidate
/// application (LLR-168). Both must match the patch record
/// exactly; the pre-patch graph digest is recomputed from the
/// graph itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchBindings {
    /// The exact ingester recipe digest the patch was curated
    /// against.
    pub recipe_digest: StructuralContentDigest,
    /// The exact verified input digest the patch was curated
    /// against.
    pub input_digest: StructuralContentDigest,
}

/// The result of one candidate patch application (LLR-168): the
/// three recorded digests and the candidate graph, a separately
/// inspectable plane that is never merged into the committed
/// parser graph.
#[derive(Debug, Clone, PartialEq)]
pub struct PatchApplication {
    /// The canonical digest of the pre-patch parser graph.
    pub pre_patch_digest: StructuralContentDigest,
    /// The patch's reviewed-content digest.
    pub patch_digest: StructuralContentDigest,
    /// The canonical digest of the validated post-patch graph.
    pub post_patch_digest: StructuralContentDigest,
    /// The candidate graph after applying the patch.
    pub graph: SourceGraph,
}

/// Apply `patch` to `graph` as a candidate, per the contract in
/// the module docs. Pure: no I/O, and `graph` is never mutated.
///
/// # Errors
///
/// Fails closed — leaving `graph` untouched — on an invalid
/// parser graph, a stale recipe/input/pre-patch-graph binding, a
/// stale operation precondition, a dangling target, an
/// inserted-identity collision, or an invalid post-patch graph.
pub fn apply_patch(
    graph: &SourceGraph,
    patch: &SourcePatchRecord,
    bindings: &PatchBindings,
    media_type: &str,
) -> Result<PatchApplication, SourcePatchError> {
    let revision_uid = patch.source_revision_uid.as_str();
    validate_graph_standalone(revision_uid, media_type, graph).map_err(|source| {
        SourcePatchError::PreGraphInvalid {
            patch_uid: patch.uid.clone(),
            source: Box::new(source),
        }
    })?;
    check_binding(
        &patch.uid,
        "recipe_digest",
        &patch.recipe_digest,
        &bindings.recipe_digest,
    )?;
    check_binding(
        &patch.uid,
        "input_digest",
        &patch.input_digest,
        &bindings.input_digest,
    )?;
    let pre_patch_digest = source_graph_digest(graph);
    check_binding(
        &patch.uid,
        "pre_patch_graph_digest",
        &patch.pre_patch_graph_digest,
        &pre_patch_digest,
    )?;
    let mut working = graph.clone();
    for operation in &patch.operations {
        apply_operation(&mut working, patch, operation)?;
    }
    recompute_node_digests(&mut working);
    validate_graph_standalone(revision_uid, media_type, &working).map_err(|source| {
        SourcePatchError::InvalidPostGraph {
            patch_uid: patch.uid.clone(),
            source: Box::new(source),
        }
    })?;
    let post_patch_digest = source_graph_digest(&working);
    Ok(PatchApplication {
        pre_patch_digest,
        patch_digest: patch.reviewed_content_digest.clone(),
        post_patch_digest,
        graph: working,
    })
}

/// One binding digest check: the presented digest must equal the
/// recorded one exactly.
fn check_binding(
    patch_uid: &str,
    field: &'static str,
    expected: &StructuralContentDigest,
    actual: &StructuralContentDigest,
) -> Result<(), SourcePatchError> {
    if expected != actual {
        return Err(SourcePatchError::StaleBinding {
            patch_uid: patch_uid.to_string(),
            field,
            expected: expected.as_str().to_string(),
            actual: actual.as_str().to_string(),
        });
    }
    Ok(())
}

/// Apply one operation to the working graph after checking its
/// exact preconditions.
fn apply_operation(
    graph: &mut SourceGraph,
    patch: &SourcePatchRecord,
    operation: &PatchOperation,
) -> Result<(), SourcePatchError> {
    match operation {
        PatchOperation::ReplaceContent {
            ordinal,
            target_uid,
            expected_content_sha256,
            new_canonical_text,
            new_label,
        } => {
            let node = target_node(graph, &patch.uid, target_uid)?;
            check_content_digest(&patch.uid, *ordinal, node, expected_content_sha256)?;
            let node = graph
                .node_mut(target_uid)
                .ok_or_else(|| dangling(&patch.uid, target_uid))?;
            if let Some(text) = new_canonical_text {
                node.canonical_text = text.clone();
            }
            if let Some(label) = new_label {
                node.label = Some(label.clone());
            }
        }
        PatchOperation::Reclassify {
            ordinal,
            target_uid,
            expected_kind,
            new_kind,
        } => {
            let node = target_node(graph, &patch.uid, target_uid)?;
            if node.kind != *expected_kind {
                return Err(SourcePatchError::StalePrecondition {
                    patch_uid: patch.uid.clone(),
                    ordinal: *ordinal,
                    target_uid: target_uid.clone(),
                    field: "kind",
                    expected: expected_kind.as_str().to_string(),
                    actual: node.kind.as_str().to_string(),
                });
            }
            let node = graph
                .node_mut(target_uid)
                .ok_or_else(|| dangling(&patch.uid, target_uid))?;
            node.kind = *new_kind;
        }
        PatchOperation::Reparent {
            ordinal,
            target_uid,
            expected_parent_uid,
            expected_ordinal,
            new_parent_uid,
            new_ordinal,
        } => {
            let node = target_node(graph, &patch.uid, target_uid)?;
            if node.parent_uid.as_deref() != expected_parent_uid.as_deref() {
                return Err(stale(
                    &patch.uid,
                    *ordinal,
                    target_uid,
                    "parent_uid",
                    expected_parent_uid.as_deref().unwrap_or("<root>"),
                    node.parent_uid.as_deref().unwrap_or("<root>"),
                ));
            }
            if node.ordinal != *expected_ordinal {
                return Err(stale(
                    &patch.uid,
                    *ordinal,
                    target_uid,
                    "ordinal",
                    &expected_ordinal.to_string(),
                    &node.ordinal.to_string(),
                ));
            }
            if let Some(parent) = new_parent_uid {
                if graph.get(parent).is_none() {
                    return Err(dangling(&patch.uid, parent));
                }
            }
            let node = graph
                .node_mut(target_uid)
                .ok_or_else(|| dangling(&patch.uid, target_uid))?;
            node.parent_uid = new_parent_uid.clone();
            node.ordinal = *new_ordinal;
        }
        PatchOperation::Insert {
            expected_parent_uid,
            node,
            ..
        } => {
            if let Some(parent) = expected_parent_uid {
                if graph.get(parent).is_none() {
                    return Err(dangling(&patch.uid, parent));
                }
            }
            let pairs = ancestry_pairs(graph, expected_parent_uid.as_deref());
            let pair_refs: Vec<(SourceNodeKind, Option<&str>)> = pairs
                .iter()
                .map(|(kind, label)| (*kind, label.as_deref()))
                .collect();
            let inserted = SourceNode {
                uid: node.uid.clone(),
                source_revision_uid: patch.source_revision_uid.clone(),
                parent_uid: expected_parent_uid.clone(),
                kind: node.kind,
                ordinal: node.ordinal,
                label: node.label.clone(),
                canonical_text: node.canonical_text.clone(),
                content_sha256: content_digest(node.kind, &node.canonical_text),
                fingerprint: fingerprint(node.kind, node.label.as_deref(), &pair_refs),
                locator: node.locator.clone(),
            };
            graph.insert(inserted).map_err(|err| match err {
                super::super::source_graph::error::SourceGraphError::DuplicateUid {
                    uid, ..
                } => SourcePatchError::InsertedIdentityCollision {
                    patch_uid: patch.uid.clone(),
                    uid,
                    field: "uid",
                    existing_uid: node.uid.clone(),
                },
                super::super::source_graph::error::SourceGraphError::DuplicateHumanId {
                    first_uid,
                    duplicate_uid,
                    ..
                } => SourcePatchError::InsertedIdentityCollision {
                    patch_uid: patch.uid.clone(),
                    uid: duplicate_uid,
                    field: "label",
                    existing_uid: first_uid,
                },
                other => SourcePatchError::InvalidPostGraph {
                    patch_uid: patch.uid.clone(),
                    source: Box::new(other),
                },
            })?;
        }
        PatchOperation::Remove {
            ordinal,
            target_uid,
            expected_digest,
            child_disposition,
        } => {
            let node = target_node(graph, &patch.uid, target_uid)?;
            check_content_digest(&patch.uid, *ordinal, node, expected_digest)?;
            match child_disposition {
                ChildDisposition::ReparentChildren => reparent_children(graph, target_uid),
                ChildDisposition::RemoveSubtree => remove_subtree(graph, target_uid),
            }
        }
    }
    Ok(())
}

/// The target node, or a dangling-target error naming the patch.
fn target_node<'g>(
    graph: &'g SourceGraph,
    patch_uid: &str,
    target_uid: &str,
) -> Result<&'g SourceNode, SourcePatchError> {
    graph
        .get(target_uid)
        .ok_or_else(|| dangling(patch_uid, target_uid))
}

fn dangling(patch_uid: &str, target_uid: &str) -> SourcePatchError {
    SourcePatchError::DanglingTarget {
        patch_uid: patch_uid.to_string(),
        target_uid: target_uid.to_string(),
    }
}

fn stale(
    patch_uid: &str,
    ordinal: u32,
    target_uid: &str,
    field: &'static str,
    expected: &str,
    actual: &str,
) -> SourcePatchError {
    SourcePatchError::StalePrecondition {
        patch_uid: patch_uid.to_string(),
        ordinal,
        target_uid: target_uid.to_string(),
        field,
        expected: expected.to_string(),
        actual: actual.to_string(),
    }
}

/// The expected-content-digest precondition shared by
/// `replace_content` and `remove`.
fn check_content_digest(
    patch_uid: &str,
    ordinal: u32,
    node: &SourceNode,
    expected: &StructuralContentDigest,
) -> Result<(), SourcePatchError> {
    if node.content_sha256 != *expected {
        return Err(stale(
            patch_uid,
            ordinal,
            &node.uid,
            "content_sha256",
            expected.as_str(),
            node.content_sha256.as_str(),
        ));
    }
    Ok(())
}

/// Remove a node and promote its children to its parent, taking
/// the removed node's position in their original order; the
/// sibling set renumbers deterministically.
fn reparent_children(graph: &mut SourceGraph, target_uid: &str) {
    let Some(removed) = graph.remove_node(target_uid) else {
        return;
    };
    let parent = removed.parent_uid.clone();
    // Deterministic promoted order: ordinal, then uid.
    let mut ordered: Vec<(u32, String)> = graph
        .nodes()
        .filter(|node| node.parent_uid.as_deref() == Some(target_uid))
        .map(|node| (node.ordinal, node.uid.clone()))
        .collect();
    ordered.sort();
    let promoted: Vec<String> = ordered.into_iter().map(|(_, uid)| uid).collect();
    let mut earlier: Vec<(u32, String)> = graph
        .nodes()
        .filter(|node| node.parent_uid == parent && node.ordinal < removed.ordinal)
        .map(|node| (node.ordinal, node.uid.clone()))
        .collect();
    earlier.sort();
    let mut later: Vec<(u32, String)> = graph
        .nodes()
        .filter(|node| node.parent_uid == parent && node.ordinal > removed.ordinal)
        .map(|node| (node.ordinal, node.uid.clone()))
        .collect();
    later.sort();
    let sequence: Vec<String> = earlier
        .into_iter()
        .map(|(_, uid)| uid)
        .chain(promoted.iter().cloned())
        .chain(later.into_iter().map(|(_, uid)| uid))
        .collect();
    for (index, uid) in sequence.iter().enumerate() {
        if let Some(node) = graph.node_mut(uid) {
            node.ordinal = index as u32;
            if promoted.contains(uid) {
                node.parent_uid = parent.clone();
            }
        }
    }
}

/// Remove a node and its entire subtree — the explicit cascade.
fn remove_subtree(graph: &mut SourceGraph, target_uid: &str) {
    let mut pending = vec![target_uid.to_string()];
    let mut members: BTreeSet<String> = BTreeSet::new();
    while let Some(uid) = pending.pop() {
        if members.insert(uid.clone()) {
            pending.extend(
                graph
                    .nodes()
                    .filter(|node| node.parent_uid.as_deref() == Some(uid.as_str()))
                    .map(|node| node.uid.clone()),
            );
        }
    }
    for uid in members {
        graph.remove_node(&uid);
    }
}

/// The `(kind, label)` ancestry pairs of `parent_uid`'s chain,
/// ordered root to parent, with a visited guard so a mid-edit
/// break stops deterministically instead of looping.
fn ancestry_pairs(
    graph: &SourceGraph,
    parent_uid: Option<&str>,
) -> Vec<(SourceNodeKind, Option<String>)> {
    let mut chain = Vec::new();
    let mut visited = BTreeSet::new();
    let mut current = parent_uid;
    while let Some(uid) = current {
        let Some(parent) = graph.get(uid) else {
            break;
        };
        if !visited.insert(parent.uid.as_str()) {
            break;
        }
        chain.push((parent.kind, parent.label.clone()));
        current = parent.parent_uid.as_deref();
    }
    chain.reverse();
    chain
}

/// Recompute every node's stored content digest and fingerprint
/// from the final kind, text, label, and ancestry, so the stored
/// digests the canonical rendering and validator check reflect the
/// post-patch graph. Untouched nodes recompute to their existing
/// values — a valid pre-patch graph stores recomputed digests by
/// construction.
fn recompute_node_digests(graph: &mut SourceGraph) {
    let mut updates: BTreeMap<String, (StructuralContentDigest, StructuralContentDigest)> =
        BTreeMap::new();
    for node in graph.nodes() {
        let content = content_digest(node.kind, &node.canonical_text);
        let pairs = ancestry_pairs(graph, node.parent_uid.as_deref());
        let pair_refs: Vec<(SourceNodeKind, Option<&str>)> = pairs
            .iter()
            .map(|(kind, label)| (*kind, label.as_deref()))
            .collect();
        let finger = fingerprint(node.kind, node.label.as_deref(), &pair_refs);
        updates.insert(node.uid.clone(), (content, finger));
    }
    for (uid, (content, finger)) in updates {
        if let Some(node) = graph.node_mut(&uid) {
            node.content_sha256 = content;
            node.fingerprint = finger;
        }
    }
}
