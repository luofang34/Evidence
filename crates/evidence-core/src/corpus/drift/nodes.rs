//! Node-plane drift: structural-key reconciliation of the
//! candidate parser graph against the committed graph, then
//! per-field comparison of every matched pair (LLR-177).
//!
//! Candidates reconcile through the `identity` module's
//! [`reconcile`] against the committed graph; the throwaway uids
//! minted for unmatched candidates never escape into findings —
//! added nodes report under their candidate uid, and a candidate
//! parent that matched nothing renders as `<uncommitted parent …>`.
//! A structural key claimed by more than one committed node is
//! formally ambiguous: the pairing cannot be shown content-coherent,
//! so every candidate consuming such a pool reports
//! [`DriftCategory::NodeUnreconciled`] instead of a field-level
//! comparison.
//!
//! # Relaxed re-pairing
//!
//! The structural key embeds the kind, the sibling ordinal, the
//! parent path, and any explicit numbering or anchor, so a node
//! whose kind, ordinal, parent, or explicit identity moved has a
//! *new* key and never primary-matches. Reporting every such move
//! as one removal plus one addition would make the per-field
//! categories unreachable, so unmatched nodes re-pair
//! deterministically: identical content digests pair first across
//! the whole unmatched set (a reparented or reordered node keeps
//! its content, so the move reports as `NodeParentChanged` or
//! `NodeOrdinalChanged`), then the remainder pairs within a
//! relaxed skeleton — the structural key with every kind erased —
//! in uid order, and leftovers report added or removed. A
//! re-paired couple compares field by field exactly like a primary
//! match, so a kind move reports `NodeKindChanged`. Root nodes
//! pair only on their exact fingerprint — a root's kind or label
//! move is indistinguishable from a removal plus an addition.
//!
//! Semantic locator fields (variant, path or canonical URL, anchor
//! or fragment, heading path) and diagnostic-only positions (byte
//! range, DOM path, page, bounding box, printed label, final URL,
//! git blob) are compared separately: semantic changes report one
//! finding per field, diagnostic movement one finding per node, and
//! diagnostic movement never counts as semantic drift.

use std::collections::{BTreeMap, BTreeSet};

use super::super::digest::StructuralContentDigest;
use super::super::source_graph::identity::{
    CandidateNode, StructuralKey, reconcile, structural_key,
};
use super::super::source_graph::{SourceGraph, SourceNode};
use super::findings::{DriftCategory, DriftDetail, DriftFinding};
use super::node_locator::{change, compare_locators, render_option};

/// Compare the candidate parser graph against the committed graph
/// of the same revision, emitting node-plane findings. Both graphs
/// are validated; the emitted order is deterministic and the
/// caller sorts.
pub(super) fn compare_nodes(committed: &SourceGraph, candidate: &SourceGraph) -> Vec<DriftFinding> {
    let mut pools: BTreeMap<StructuralKey, Vec<String>> = BTreeMap::new();
    for node in committed.nodes() {
        pools
            .entry(structural_key(node, committed))
            .or_default()
            .push(node.uid.clone());
    }
    let candidates: Vec<CandidateNode> = candidate.nodes().map(to_candidate).collect();
    let reconciled = reconcile(committed, candidates);
    // Every candidate's resolved uid: the reused committed uid on a
    // primary match, the throwaway minted uid otherwise. Relaxed
    // pairs overwrite their entry with the paired committed uid.
    let mut reconciled_uid: BTreeMap<String, String> = reconciled
        .iter()
        .map(|entry| (entry.candidate.provisional_id.clone(), entry.uid.clone()))
        .collect();
    let mut matched: BTreeSet<&str> = BTreeSet::new();
    let mut pairs: Vec<(&SourceNode, &SourceNode)> = Vec::new();
    let mut unmatched_candidates: Vec<&SourceNode> = Vec::new();
    let mut findings = Vec::new();
    for entry in &reconciled {
        let Some(candidate_node) = candidate.get(&entry.candidate.provisional_id) else {
            continue;
        };
        let Some(committed_node) = committed.get(&entry.uid) else {
            unmatched_candidates.push(candidate_node);
            continue;
        };
        matched.insert(committed_node.uid.as_str());
        let key = structural_key(committed_node, committed);
        let pool: &[String] = pools.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        if pool.len() > 1 {
            findings.push(DriftFinding {
                category: DriftCategory::NodeUnreconciled,
                structural_path: Some(structural_path(committed, &committed_node.uid)),
                node_uid: Some(candidate_node.uid.clone()),
                patch_uid: None,
                detail: DriftDetail::AmbiguousKey {
                    committed_pool: pool.to_vec(),
                },
            });
            continue;
        }
        pairs.push((committed_node, candidate_node));
    }
    let unmatched_committed: Vec<&SourceNode> = committed
        .nodes()
        .filter(|node| !matched.contains(node.uid.as_str()))
        .collect();
    let (relaxed, leftover_committed, leftover_candidates) = relaxed_pairs(
        &unmatched_committed,
        &unmatched_candidates,
        committed,
        candidate,
    );
    for (committed_uid, candidate_uid) in relaxed {
        if let (Some(committed_node), Some(candidate_node)) =
            (committed.get(&committed_uid), candidate.get(&candidate_uid))
        {
            reconciled_uid.insert(candidate_uid, committed_uid);
            pairs.push((committed_node, candidate_node));
        }
    }
    for (committed_node, candidate_node) in pairs {
        compare_matched(
            committed_node,
            candidate_node,
            committed,
            &reconciled_uid,
            &mut findings,
        );
    }
    for uid in leftover_candidates {
        findings.push(DriftFinding {
            category: DriftCategory::NodeAdded,
            structural_path: Some(structural_path(candidate, &uid)),
            node_uid: Some(uid),
            patch_uid: None,
            detail: DriftDetail::CandidateOnly,
        });
    }
    for uid in leftover_committed {
        findings.push(DriftFinding {
            category: DriftCategory::NodeRemoved,
            structural_path: Some(structural_path(committed, &uid)),
            node_uid: Some(uid),
            patch_uid: None,
            detail: DriftDetail::CommittedOnly,
        });
    }
    findings
}

/// The relaxed pairing skeleton: the structural key with every
/// kind erased. `Ord` is the derived order, used only for
/// deterministic grouping.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum RelaxedKey {
    /// Tier 1 without the kind: the explicit numbering or anchor.
    Explicit {
        /// The numbering token or document-author anchor.
        anchor: String,
    },
    /// Tier 2 without the kinds: the parent's skeleton and the
    /// sibling ordinal.
    Position {
        /// The parent's relaxed skeleton.
        parent: Box<RelaxedKey>,
        /// The node's ordinal within its sibling set.
        ordinal: u32,
    },
    /// Tier 3 unchanged: the fingerprint covers kind and label, so
    /// roots re-pair only on an exact fingerprint.
    Fingerprint(StructuralContentDigest),
}

/// Erase every kind from one structural key.
fn relax(key: &StructuralKey) -> RelaxedKey {
    match key {
        StructuralKey::Explicit { anchor, .. } => RelaxedKey::Explicit {
            anchor: anchor.clone(),
        },
        StructuralKey::Position {
            parent, ordinal, ..
        } => RelaxedKey::Position {
            parent: Box::new(relax(parent)),
            ordinal: *ordinal,
        },
        StructuralKey::Fingerprint(digest) => RelaxedKey::Fingerprint(digest.clone()),
    }
}

/// Re-pair unmatched nodes: a moved node keeps its content, so the
/// global content pass pairs identical content digests first (in
/// candidate uid order, consuming committed uids in uid order),
/// then the remainder pairs within one relaxed skeleton in uid
/// order. Returns the pairs plus the leftover committed and
/// candidate uids.
fn relaxed_pairs(
    committed: &[&SourceNode],
    candidates: &[&SourceNode],
    committed_graph: &SourceGraph,
    candidate_graph: &SourceGraph,
) -> (Vec<(String, String)>, Vec<String>, Vec<String>) {
    // Content pass: pair across the whole unmatched set, so a
    // reparented or reordered node with unchanged content pairs
    // with its committed counterpart and reports the move.
    let mut by_content: BTreeMap<&StructuralContentDigest, Vec<&SourceNode>> = BTreeMap::new();
    for node in committed {
        by_content
            .entry(&node.content_sha256)
            .or_default()
            .push(node);
    }
    let mut pairs = Vec::new();
    let mut deferred: Vec<&SourceNode> = Vec::new();
    for candidate_node in candidates {
        let paired = by_content
            .get_mut(&candidate_node.content_sha256)
            .and_then(|pool| {
                if pool.is_empty() {
                    None
                } else {
                    Some(pool.remove(0))
                }
            });
        match paired {
            Some(committed_node) => {
                pairs.push((committed_node.uid.clone(), candidate_node.uid.clone()));
            }
            None => deferred.push(candidate_node),
        }
    }
    let mut remaining: Vec<&SourceNode> = Vec::new();
    for node in committed {
        if !pairs
            .iter()
            .any(|(committed_uid, _)| committed_uid == &node.uid)
        {
            remaining.push(node);
        }
    }
    // Skeleton pass: the content-unmatched remainder pairs within
    // one kind-erased skeleton in uid order.
    let mut committed_by_key: BTreeMap<RelaxedKey, Vec<&SourceNode>> = BTreeMap::new();
    for node in remaining {
        committed_by_key
            .entry(relax(&structural_key(node, committed_graph)))
            .or_default()
            .push(node);
    }
    let mut candidate_by_key: BTreeMap<RelaxedKey, Vec<&SourceNode>> = BTreeMap::new();
    for node in deferred {
        candidate_by_key
            .entry(relax(&structural_key(node, candidate_graph)))
            .or_default()
            .push(node);
    }
    let keys: BTreeSet<&RelaxedKey> = committed_by_key
        .keys()
        .chain(candidate_by_key.keys())
        .collect();
    let mut leftover_committed = Vec::new();
    let mut leftover_candidates = Vec::new();
    for key in keys {
        let committed_pool = committed_by_key.get(key).map(Vec::as_slice).unwrap_or(&[]);
        let candidate_pool = candidate_by_key.get(key).map(Vec::as_slice).unwrap_or(&[]);
        let mut deferred_iter = candidate_pool.iter();
        for committed_node in committed_pool {
            match deferred_iter.next() {
                Some(candidate_node) => {
                    pairs.push((committed_node.uid.clone(), candidate_node.uid.clone()));
                }
                None => leftover_committed.push(committed_node.uid.clone()),
            }
        }
        leftover_candidates.extend(deferred_iter.map(|node| node.uid.clone()));
    }
    (pairs, leftover_committed, leftover_candidates)
}

/// Project one committed-shaped candidate node into the identity
/// module's candidate record: the provisional identity is the
/// candidate uid, unique within the validated candidate graph.
fn to_candidate(node: &SourceNode) -> CandidateNode {
    CandidateNode {
        provisional_id: node.uid.clone(),
        parent_id: node.parent_uid.clone(),
        kind: node.kind,
        ordinal: node.ordinal,
        label: node.label.clone(),
        canonical_text: node.canonical_text.clone(),
        locator: node.locator.clone(),
    }
}

/// Compare one matched pair field by field; every changed field is
/// its own finding, so one changed node never hides another field.
fn compare_matched(
    committed: &SourceNode,
    candidate: &SourceNode,
    committed_graph: &SourceGraph,
    reconciled_uid: &BTreeMap<String, String>,
    findings: &mut Vec<DriftFinding>,
) {
    let mut push = |category, detail| {
        findings.push(DriftFinding {
            category,
            structural_path: Some(structural_path(committed_graph, &committed.uid)),
            node_uid: Some(committed.uid.clone()),
            patch_uid: None,
            detail,
        });
    };
    if committed.kind != candidate.kind {
        push(
            DriftCategory::NodeKindChanged,
            change("kind", committed.kind.as_str(), candidate.kind.as_str()),
        );
    }
    let (committed_parent, candidate_parent) =
        rendered_parents(committed, candidate, committed_graph, reconciled_uid);
    if committed_parent != candidate_parent {
        push(
            DriftCategory::NodeParentChanged,
            change("parent_uid", &committed_parent, &candidate_parent),
        );
    }
    if committed.ordinal != candidate.ordinal {
        push(
            DriftCategory::NodeOrdinalChanged,
            change(
                "ordinal",
                &committed.ordinal.to_string(),
                &candidate.ordinal.to_string(),
            ),
        );
    }
    if committed.label != candidate.label {
        push(
            DriftCategory::NodeLabelChanged,
            change(
                "label",
                &render_option(committed.label.as_deref()),
                &render_option(candidate.label.as_deref()),
            ),
        );
    }
    if committed.canonical_text != candidate.canonical_text {
        push(
            DriftCategory::NodeCanonicalTextChanged,
            change(
                "canonical_text",
                &committed.canonical_text,
                &candidate.canonical_text,
            ),
        );
    }
    if committed.content_sha256 != candidate.content_sha256 {
        push(
            DriftCategory::NodeContentDigestChanged,
            change(
                "content_sha256",
                committed.content_sha256.as_str(),
                candidate.content_sha256.as_str(),
            ),
        );
    }
    if committed.fingerprint != candidate.fingerprint {
        push(
            DriftCategory::NodeStructuralFingerprintChanged,
            change(
                "fingerprint",
                committed.fingerprint.as_str(),
                candidate.fingerprint.as_str(),
            ),
        );
    }
    compare_locators(committed, candidate, &mut push);
}

/// Render both sides' parent linkage deterministically. A
/// candidate parent that matched no committed node renders as
/// `<uncommitted parent …>` naming the candidate uid — never the
/// throwaway minted uid.
fn rendered_parents(
    committed: &SourceNode,
    candidate: &SourceNode,
    committed_graph: &SourceGraph,
    reconciled_uid: &BTreeMap<String, String>,
) -> (String, String) {
    let committed_parent = committed
        .parent_uid
        .clone()
        .unwrap_or_else(|| "<root>".to_string());
    let candidate_parent = match &candidate.parent_uid {
        None => "<root>".to_string(),
        Some(parent) => match reconciled_uid.get(parent.as_str()) {
            // A matched parent reuses a committed uid; an unmatched
            // parent's minted uid is random and never renders.
            Some(reconciled) if committed_graph.get(reconciled).is_some() => reconciled.to_string(),
            _ => format!("<uncommitted parent {parent}>"),
        },
    };
    (committed_parent, candidate_parent)
}

fn structural_path(graph: &SourceGraph, uid: &str) -> String {
    let mut segments = Vec::new();
    let mut visited = BTreeSet::new();
    let mut current = Some(uid);
    while let Some(node_uid) = current {
        let Some(node) = graph.get(node_uid) else {
            break;
        };
        if !visited.insert(node_uid) {
            break;
        }
        segments.push(match &node.label {
            Some(label) => label.clone(),
            None => format!("{}[{}]", node.kind.as_str(), node.ordinal),
        });
        current = node.parent_uid.as_deref();
    }
    segments.reverse();
    segments.join("/")
}
