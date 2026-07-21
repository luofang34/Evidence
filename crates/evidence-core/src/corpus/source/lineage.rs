//! Source-revision lineage validation, effective-head derivation,
//! and immutable-superset transition comparison (LLR-130, LLR-131,
//! LLR-132).
//!
//! Revisions of one logical document group under a shared
//! `document_key`, and a newer revision owns at most one
//! [`EdgeKind::Supersedes`] edge naming the prior revision it
//! supersedes. [`validate_source_lineage`] runs inside
//! [`CorpusGraph::validate`] after every edge has resolved and
//! endpoint kinds have been checked, so a source `Supersedes` edge
//! always points at an existing source-revision node. Every check
//! iterates in uid-sorted order, so the reported violation is
//! independent of load order:
//!
//! 1. Per revision: at most one outgoing `Supersedes` edge — a
//!    revision supersedes at most one predecessor. The record
//!    loader cannot produce a multi-supersession node (a record
//!    names a single optional `supersedes`), so this invariant
//!    guards programmatically built graphs.
//! 2. Per edge: a revision may not supersede itself, and both
//!    revisions of a link must share one `document_key`.
//! 3. Forks: a revision may be superseded by at most one other
//!    revision — at most one incoming `Supersedes` edge, the dual
//!    direction of check 1.
//! 4. Cycles: walking the supersession chain must never revisit a
//!    revision.
//! 5. Roots: each `document_key` has exactly one root — a revision
//!    that supersedes nothing — so the lineage is a single chain,
//!    not several unrelated ones. The dual multiple-heads check
//!    guards the exactly-one-effective-head invariant as defense
//!    in depth (an acyclic lineage with at most one edge per
//!    direction has equally many roots and heads, so check 5
//!    reports a violating lineage first).
//!
//! [`effective_source_heads`] derives the one effective head per
//! `document_key` — the revision no other revision supersedes — as
//! a sorted map, so timestamps, file layout, and record order
//! never select a head.
//!
//! [`validate_source_transition`] and [`SourceRevisionProjection`]
//! live in the `transition` sibling: a pure comparison between a
//! prior graph and a proposed graph, scoped to the source-revision
//! subgraph, so the proposed subgraph must be a UID-preserving
//! superset whose retained revisions keep a byte-for-byte-equal
//! projection, and a new revision of an existing `document_key`
//! must extend the prior effective head. Both graphs are validated
//! inside the call, so an `Ok` transition implies two valid graphs.

use std::collections::{BTreeMap, BTreeSet};

use super::super::graph::{CorpusGraph, EdgeKind, Node, SourceRevisionNode};
use super::error::SourceError;

pub(super) mod transition;

pub use transition::{SourceRevisionProjection, validate_source_transition};

/// Validate every source-revision lineage chain in `graph`
/// (LLR-130). Runs inside [`CorpusGraph::validate`] after edge
/// resolution and review validation; failures surface there as
/// [`CorpusError::Source`](super::super::error::CorpusError::Source).
///
/// # Errors
///
/// Distinct [`SourceError`] variants per violation, in check
/// order: [`SourceError::SourceDuplicateSupersedesEdge`],
/// [`SourceError::SourceSupersessionSelf`],
/// [`SourceError::SourceSupersessionDocumentKey`],
/// [`SourceError::SourceSupersessionFork`],
/// [`SourceError::SourceSupersessionCycle`],
/// [`SourceError::SourceLineageMultipleRoots`], and
/// [`SourceError::SourceLineageMultipleHeads`].
pub fn validate_source_lineage(graph: &CorpusGraph) -> Result<(), SourceError> {
    let mut superseded_by: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for node in graph.nodes() {
        let Node::SourceRevision(revision) = node else {
            continue;
        };
        let supersedes_count = revision
            .edges
            .iter()
            .filter(|(kind, _)| *kind == EdgeKind::Supersedes)
            .count();
        if supersedes_count > 1 {
            return Err(SourceError::SourceDuplicateSupersedesEdge {
                source_uid: revision.uid.clone(),
                count: supersedes_count,
            });
        }
        for (kind, target) in &revision.edges {
            if *kind != EdgeKind::Supersedes {
                continue;
            }
            if target == &revision.uid {
                return Err(SourceError::SourceSupersessionSelf {
                    uid: revision.uid.clone(),
                });
            }
            // `CorpusGraph::validate` resolves edges and checks
            // endpoint kinds before this validator runs, so the
            // target is a present source-revision node. Called
            // standalone on an unvalidated graph, a missing or
            // wrong-kind target is skipped here and reported by
            // edge validation instead.
            let Some(Node::SourceRevision(predecessor)) = graph.get(target.as_str()) else {
                continue;
            };
            if revision.document_key != predecessor.document_key {
                return Err(SourceError::SourceSupersessionDocumentKey {
                    uid: revision.uid.clone(),
                    predecessor_uid: predecessor.uid.clone(),
                });
            }
            superseded_by
                .entry(predecessor.uid.as_str())
                .or_default()
                .push(revision.uid.as_str());
        }
    }
    reject_forks(&superseded_by)?;
    reject_cycles(graph)?;
    reject_multiple_roots(graph)?;
    reject_multiple_heads(graph)
}

/// Derive the effective source head of each document key: the
/// revision of that document key no other revision supersedes
/// (LLR-131). `BTreeMap` iteration keeps reporting order
/// deterministic, and the edge set alone selects heads —
/// timestamps, file layout, and record order never do.
///
/// Read-only derived view: on a graph that passed
/// [`CorpusGraph::validate`] each document key yields exactly one
/// head. An unvalidated graph still derives deterministically
/// (uid iteration order), but lineage invariants may not hold —
/// validate first.
pub fn effective_source_heads(graph: &CorpusGraph) -> BTreeMap<String, String> {
    let superseded: BTreeSet<&str> = graph
        .nodes()
        .flat_map(|node| node.edges())
        .filter(|(kind, _)| *kind == EdgeKind::Supersedes)
        .map(|(_, target)| target.as_str())
        .collect();
    let mut heads = BTreeMap::new();
    for node in graph.nodes() {
        let Node::SourceRevision(revision) = node else {
            continue;
        };
        if !superseded.contains(revision.uid.as_str()) {
            heads.insert(revision.document_key.clone(), revision.uid.clone());
        }
    }
    heads
}

/// The single owned `Supersedes` target of a revision, if any.
/// Lineage validation guarantees at most one on a validated graph;
/// edges are canonicalized at insert, so the choice is
/// deterministic even before validation.
fn supersedes_target(revision: &SourceRevisionNode) -> Option<&str> {
    revision
        .edges
        .iter()
        .find(|(kind, _)| *kind == EdgeKind::Supersedes)
        .map(|(_, target)| target.as_str())
}

/// Group source revisions by document key. The outer `BTreeMap`
/// iterates document keys in sorted order and each group collects
/// revisions in uid order, so violation reports are load-order
/// independent.
fn revisions_by_document(graph: &CorpusGraph) -> BTreeMap<String, Vec<&SourceRevisionNode>> {
    let mut by_document: BTreeMap<String, Vec<&SourceRevisionNode>> = BTreeMap::new();
    for node in graph.nodes() {
        let Node::SourceRevision(revision) = node else {
            continue;
        };
        by_document
            .entry(revision.document_key.clone())
            .or_default()
            .push(revision);
    }
    by_document
}

/// A fork is a revision with more than one incoming supersession —
/// the dual direction of the per-revision outgoing check in
/// [`validate_source_lineage`]: a revision supersedes at most one
/// predecessor, and a predecessor is superseded by at most one
/// revision. Successors were collected in uid order, so the named
/// pair is deterministic.
fn reject_forks(superseded_by: &BTreeMap<&str, Vec<&str>>) -> Result<(), SourceError> {
    for (predecessor, successors) in superseded_by {
        if let [first, second, ..] = successors.as_slice() {
            return Err(SourceError::SourceSupersessionFork {
                uid: (*predecessor).to_string(),
                first_uid: (*first).to_string(),
                second_uid: (*second).to_string(),
            });
        }
    }
    Ok(())
}

/// Walk every supersession chain; a revisit along the walk is a
/// cycle. Forks are already rejected, so supersession indegree is
/// at most one and distinct walks cannot reconverge — a `path` hit
/// is always a true cycle.
fn reject_cycles(graph: &CorpusGraph) -> Result<(), SourceError> {
    let mut done: BTreeSet<&str> = BTreeSet::new();
    for node in graph.nodes() {
        let Node::SourceRevision(revision) = node else {
            continue;
        };
        if done.contains(revision.uid.as_str()) {
            continue;
        }
        let mut path: Vec<&str> = Vec::new();
        let mut frontier: Vec<&str> = vec![revision.uid.as_str()];
        while let Some(current) = frontier.pop() {
            if path.contains(&current) {
                return Err(SourceError::SourceSupersessionCycle {
                    uid: current.to_string(),
                });
            }
            if done.contains(current) {
                continue;
            }
            path.push(current);
            if let Some(Node::SourceRevision(current_revision)) = graph.get(current) {
                frontier.extend(
                    current_revision
                        .edges
                        .iter()
                        .filter(|(kind, _)| *kind == EdgeKind::Supersedes)
                        .map(|(_, target)| target.as_str()),
                );
            }
        }
        done.extend(path);
    }
    Ok(())
}

/// A root is a revision that supersedes nothing. Each document key
/// must have exactly one — the lineage is a single chain, so two
/// roots mean two unrelated chains share the key. Roots iterate in
/// uid order, so the named pair is deterministic.
fn reject_multiple_roots(graph: &CorpusGraph) -> Result<(), SourceError> {
    for (document_key, revisions) in revisions_by_document(graph) {
        let roots: Vec<&str> = revisions
            .iter()
            .filter(|revision| supersedes_target(revision).is_none())
            .map(|revision| revision.uid.as_str())
            .collect();
        if let [first, second, ..] = roots.as_slice() {
            return Err(SourceError::SourceLineageMultipleRoots {
                document_key: document_key.clone(),
                first_uid: (*first).to_string(),
                second_uid: (*second).to_string(),
            });
        }
    }
    Ok(())
}

/// A head is a revision no other revision supersedes. Each
/// document key must have exactly one. Defense in depth: an
/// acyclic lineage with at most one edge per direction has equally
/// many roots and heads, so [`reject_multiple_roots`] reports a
/// violating lineage first through the public validators and this
/// check is unreachable there — it guards the dual invariant if
/// the check order ever changes.
fn reject_multiple_heads(graph: &CorpusGraph) -> Result<(), SourceError> {
    let superseded: BTreeSet<&str> = graph
        .nodes()
        .flat_map(|node| node.edges())
        .filter(|(kind, _)| *kind == EdgeKind::Supersedes)
        .map(|(_, target)| target.as_str())
        .collect();
    for (document_key, revisions) in revisions_by_document(graph) {
        let heads: Vec<&str> = revisions
            .iter()
            .filter(|revision| !superseded.contains(revision.uid.as_str()))
            .map(|revision| revision.uid.as_str())
            .collect();
        if let [first, second, ..] = heads.as_slice() {
            return Err(SourceError::SourceLineageMultipleHeads {
                document_key: document_key.clone(),
                first_uid: (*first).to_string(),
                second_uid: (*second).to_string(),
            });
        }
    }
    Ok(())
}

// Tests live in sibling files pulled in via `#[path]`: shared
// fixtures plus one module per TEST entry.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "lineage/endpoint_tests.rs"]
mod endpoint_tests;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "lineage/fixtures.rs"]
mod fixtures;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "lineage/tests.rs"]
mod tests;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "lineage/transition_tests.rs"]
mod transition_tests;
