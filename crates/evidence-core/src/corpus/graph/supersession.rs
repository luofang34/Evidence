//! Review supersession chain validation (LLR-115).
//!
//! Runs inside [`CorpusGraph::validate`] after every edge has
//! resolved and endpoint kinds have been checked, so a `Supersedes`
//! edge always points at an existing review node. Every check
//! iterates in uid-sorted order, so the reported violation is
//! independent of load order:
//!
//! 1. Per edge: a review may not supersede itself, and a
//!    superseding review must name the same reviewer, requirement
//!    uid, and reviewed content digest as its predecessor — each
//!    mismatch is a distinct error naming both uids.
//! 2. Forks: a review may be superseded by at most one other
//!    review.
//! 3. Cycles: walking the supersession chain must never revisit a
//!    review.

use std::collections::{BTreeMap, BTreeSet};

use super::super::review_records::error::ReviewError;
use super::{CorpusGraph, EdgeKind, Node};

/// Validate every review supersession chain in `graph`.
pub(super) fn validate_review_supersession(graph: &CorpusGraph) -> Result<(), ReviewError> {
    let mut superseded_by: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for node in graph.nodes.values() {
        let Node::Review(review) = node else {
            continue;
        };
        for (kind, target) in &review.edges {
            if *kind != EdgeKind::Supersedes {
                continue;
            }
            if target == &review.uid {
                return Err(ReviewError::ReviewSupersessionSelf {
                    uid: review.uid.clone(),
                });
            }
            // Edge resolution and endpoint kinds ran first in
            // `validate`: the target is a present review node.
            let Some(Node::Review(predecessor)) = graph.nodes.get(target.as_str()) else {
                continue;
            };
            if review.reviewer != predecessor.reviewer {
                return Err(ReviewError::ReviewSupersessionReviewer {
                    uid: review.uid.clone(),
                    predecessor_uid: predecessor.uid.clone(),
                });
            }
            if review.requirement_uid != predecessor.requirement_uid {
                return Err(ReviewError::ReviewSupersessionRequirement {
                    uid: review.uid.clone(),
                    predecessor_uid: predecessor.uid.clone(),
                });
            }
            if review.reviewed_content_sha256 != predecessor.reviewed_content_sha256 {
                return Err(ReviewError::ReviewSupersessionDigest {
                    uid: review.uid.clone(),
                    predecessor_uid: predecessor.uid.clone(),
                });
            }
            superseded_by
                .entry(predecessor.uid.as_str())
                .or_default()
                .push(review.uid.as_str());
        }
    }
    reject_forks(&superseded_by)?;
    reject_cycles(graph)
}

/// A fork is a review with more than one incoming supersession.
/// Successors were collected in uid order, so the named pair is
/// deterministic.
fn reject_forks(superseded_by: &BTreeMap<&str, Vec<&str>>) -> Result<(), ReviewError> {
    for (predecessor, successors) in superseded_by {
        if let [first, second, ..] = successors.as_slice() {
            return Err(ReviewError::ReviewSupersessionFork {
                uid: (*predecessor).to_string(),
                first_uid: (*first).to_string(),
                second_uid: (*second).to_string(),
            });
        }
    }
    Ok(())
}

/// Walk every supersession chain; a revisit along the walk is a
/// cycle. Forks are already rejected, so supersession indegree is at
/// most one and distinct walks cannot reconverge — a `path` hit is
/// always a true cycle.
fn reject_cycles(graph: &CorpusGraph) -> Result<(), ReviewError> {
    let mut done: BTreeSet<&str> = BTreeSet::new();
    for node in graph.nodes.values() {
        let Node::Review(review) = node else {
            continue;
        };
        if done.contains(review.uid.as_str()) {
            continue;
        }
        let mut path: Vec<&str> = Vec::new();
        let mut frontier: Vec<&str> = vec![review.uid.as_str()];
        while let Some(current) = frontier.pop() {
            if path.contains(&current) {
                return Err(ReviewError::ReviewSupersessionCycle {
                    uid: current.to_string(),
                });
            }
            if done.contains(current) {
                continue;
            }
            path.push(current);
            if let Some(Node::Review(current_review)) = graph.nodes.get(current) {
                frontier.extend(
                    current_review
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "supersession/tests.rs"]
mod tests;
