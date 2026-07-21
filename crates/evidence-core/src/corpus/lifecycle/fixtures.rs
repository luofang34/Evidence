//! Shared fixtures for the lifecycle test modules (TEST-135,
//! TEST-136): programmatically built graphs — exactly the loader
//! bypass the malformed-graph cases rely on.

use super::{LifecycleEvaluation, evaluate_lifecycle};
use crate::corpus::{
    CorpusGraph, EdgeKind, Node, RequirementLayer, RequirementNode, RequirementReviewContentV1,
    ReviewContentDigest, ReviewDecision, ReviewNode, review_content_digest_v1,
};

pub(super) const REQ: &str = "req_a";
pub(super) const REQ_B: &str = "req_b";
pub(super) const REV_1: &str = "rev_1";
pub(super) const REV_2: &str = "rev_2";

/// A requirement whose `description` populates the review-content
/// projection, so editing it moves the digest.
pub(super) fn requirement(uid: &str, description: &str) -> RequirementNode {
    let mut node = RequirementNode::new(
        uid.to_string(),
        uid.to_uppercase().replace('_', "-"),
        format!("title of {uid}"),
        RequirementLayer::Hlr,
        Vec::new(),
    );
    node.description = Some(description.to_string());
    node
}

/// The digest a review of `node`'s current content binds.
pub(super) fn digest_of(node: &RequirementNode) -> ReviewContentDigest {
    review_content_digest_v1(&RequirementReviewContentV1::from_node(node))
}

pub(super) fn review(
    uid: &str,
    requirement_uid: &str,
    digest: &ReviewContentDigest,
    decision: ReviewDecision,
) -> ReviewNode {
    ReviewNode {
        uid: uid.to_string(),
        id: uid.to_string(),
        requirement_uid: requirement_uid.to_string(),
        content_schema: 1,
        reviewed_content_sha256: digest.clone(),
        decision,
        reviewer: format!("{uid}@example.com"),
        reviewed_at: "2026-07-01T10:00:00Z".to_string(),
        rationale: match decision {
            ReviewDecision::Approve => None,
            ReviewDecision::Reject => Some("reviewed and found wanting".to_string()),
        },
        edges: vec![(EdgeKind::Reviews, requirement_uid.to_string())],
    }
}

pub(super) fn approve(
    uid: &str,
    requirement_uid: &str,
    digest: &ReviewContentDigest,
) -> ReviewNode {
    review(uid, requirement_uid, digest, ReviewDecision::Approve)
}

pub(super) fn reject(uid: &str, requirement_uid: &str, digest: &ReviewContentDigest) -> ReviewNode {
    review(uid, requirement_uid, digest, ReviewDecision::Reject)
}

/// Override the reviewer — a supersession chain names one reviewer.
pub(super) fn by(mut node: ReviewNode, reviewer: &str) -> ReviewNode {
    node.reviewer = reviewer.to_string();
    node
}

pub(super) fn supersedes(node: &mut ReviewNode, predecessor: &str) {
    node.edges
        .push((EdgeKind::Supersedes, predecessor.to_string()));
}

pub(super) fn graph_with(req: RequirementNode, reviews: Vec<ReviewNode>) -> CorpusGraph {
    let mut graph = CorpusGraph::new();
    graph
        .insert(Node::Requirement(req))
        .expect("insert requirement");
    for review in reviews {
        graph.insert(Node::Review(review)).expect("insert review");
    }
    graph
}

pub(super) fn evaluate(graph: &CorpusGraph, uid: &str) -> LifecycleEvaluation {
    evaluate_lifecycle(graph, uid).expect("evaluation succeeds")
}
