//! Shared fixtures for the approval-boundary test modules
//! (TEST-137): programmatically built graphs. Assertion helpers
//! with `panic!` paths stay in `tests.rs` — the library-panics
//! ceiling walk exempts only files named `tests.rs`.

use crate::corpus::graph::{RequirementMetadata, TraceMetadata};
use crate::corpus::{
    CorpusGraph, EdgeKind, Node, RequirementLayer, RequirementNode, RequirementReviewContentV1,
    ReviewContentDigest, ReviewDecision, ReviewNode, ReviewTarget, TestNode,
    review_content_digest_v1,
};

pub(super) const REQ: &str = "req_a";
pub(super) const REQ_B: &str = "req_b";
pub(super) const REQ_C: &str = "req_c";
pub(super) const REV_1: &str = "rev_1";
pub(super) const TEST_A: &str = "test_a";
pub(super) const TEST_B: &str = "test_b";

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

/// Insert a requirement carrying legacy-style trace metadata claims
/// (`modules` / `emits`), as the legacy adapter populates them.
pub(super) fn insert_with_claims(
    graph: &mut CorpusGraph,
    node: RequirementNode,
    modules: &[&str],
    emits: &[&str],
) {
    graph
        .insert_with_trace_metadata(
            Node::Requirement(node),
            TraceMetadata::Requirement(RequirementMetadata {
                modules: modules.iter().map(|m| (*m).to_string()).collect(),
                emits: emits.iter().map(|c| (*c).to_string()).collect(),
                ..RequirementMetadata::default()
            }),
        )
        .expect("insert requirement with claims");
}

pub(super) fn test_verifies(uid: &str, target: &str) -> TestNode {
    TestNode {
        uid: uid.to_string(),
        id: uid.to_uppercase().replace('_', "-"),
        title: format!("title of {uid}"),
        selectors: Vec::new(),
        edges: vec![(EdgeKind::Verifies, target.to_string())],
    }
}

fn review(
    uid: &str,
    requirement_uid: &str,
    digest: &ReviewContentDigest,
    decision: ReviewDecision,
) -> ReviewNode {
    ReviewNode {
        uid: uid.to_string(),
        id: uid.to_string(),
        target: ReviewTarget::Requirement(requirement_uid.to_string()),
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
