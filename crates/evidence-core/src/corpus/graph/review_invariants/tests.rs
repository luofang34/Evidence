//! Adversarial tests for the per-node review invariants: malformed
//! review nodes built programmatically (bypassing the strict record
//! loader) fail closed with distinct typed errors before supersession
//! chain validation runs (TEST-133).

use crate::corpus::{
    CorpusError, CorpusGraph, EdgeKind, Node, RequirementLayer, RequirementNode,
    ReviewContentDigest, ReviewDecision, ReviewError, ReviewNode, ReviewTarget,
};

const REQ_A: &str = "req_00000000-0000-4000-8000-00000000000a";
const REQ_B: &str = "req_00000000-0000-4000-8000-00000000000b";
const REV_1: &str = "rev_00000000-0000-4000-8000-0000000000a1";
const REV_2: &str = "rev_00000000-0000-4000-8000-0000000000a2";
const REV_3: &str = "rev_00000000-0000-4000-8000-0000000000a3";

fn requirement(uid: &str) -> Node {
    Node::Requirement(RequirementNode::new(
        uid.to_string(),
        format!("id of {uid}"),
        "title".to_string(),
        RequirementLayer::Hlr,
        Vec::new(),
    ))
}

/// A valid review node by `alice@example.com` over REQ_A: one
/// `Reviews` edge naming REQ_A, matching `requirement_uid`, and the
/// supported content schema. Tests mutate fields or edges to build
/// adversarial nodes.
fn review_node(uid: &str, id: &str) -> ReviewNode {
    ReviewNode {
        uid: uid.to_string(),
        id: id.to_string(),
        target: ReviewTarget::Requirement(REQ_A.to_string()),
        content_schema: 1,
        reviewed_content_sha256: ReviewContentDigest::from_hex(&"a".repeat(64)).unwrap(),
        decision: ReviewDecision::Approve,
        reviewer: "alice@example.com".to_string(),
        reviewed_at: "2026-07-01T10:00:00Z".to_string(),
        rationale: None,
        edges: vec![(EdgeKind::Reviews, REQ_A.to_string())],
    }
}

/// A graph with both requirements plus the given review nodes.
fn graph_with(reviews: Vec<ReviewNode>) -> CorpusGraph {
    let mut graph = CorpusGraph::new();
    graph.insert(requirement(REQ_A)).unwrap();
    graph.insert(requirement(REQ_B)).unwrap();
    for review in reviews {
        graph.insert(Node::Review(review)).unwrap();
    }
    graph
}

/// A review whose `requirement_uid` field names REQ_A but whose
/// `Reviews` edge targets REQ_B is rejected with the mismatch
/// variant (TEST-133).
#[test]
fn review_node_with_mismatched_requirement_field_and_edge_is_rejected() {
    let mut review = review_node(REV_1, "REV-001");
    review.edges = vec![(EdgeKind::Reviews, REQ_B.to_string())];
    let result = graph_with(vec![review]).validate();
    let err = result.expect_err("a field/edge disagreement must fail closed");
    assert!(
        matches!(
            err,
            CorpusError::Review(ReviewError::ReviewTargetEdgeMismatch {
                ref review_uid,
                ref field_target_uid,
                ref edge_target_uid,
            }) if review_uid == REV_1
                && field_target_uid == REQ_A
                && edge_target_uid == REQ_B
        ),
        "the error must name the review and both requirement uids, got: {err:?}"
    );
}

/// A programmatically built review with an unsupported content
/// schema bypasses the record loader but still fails validation
/// (TEST-133).
#[test]
fn programmatic_review_with_unsupported_content_schema_is_rejected() {
    let mut review = review_node(REV_1, "REV-001");
    review.content_schema = 99;
    let result = graph_with(vec![review]).validate();
    let err = result.expect_err("an unsupported content schema must fail closed");
    assert!(
        matches!(
            err,
            CorpusError::Review(ReviewError::ReviewContentSchema {
                found: 99,
                supported: 1,
                ref uid,
                ..
            }) if uid == REV_1
        ),
        "the error must name the schema found and supported, got: {err:?}"
    );
    assert!(
        err.to_string().contains("<graph>"),
        "a programmatic review has no record file: {err}"
    );
}

/// A review with no `Reviews` edge at all — bare, or carrying only
/// a `Supersedes` pointer — is rejected with the missing-edge
/// variant (TEST-133).
#[test]
fn review_node_without_reviews_edge_is_rejected() {
    let mut bare = review_node(REV_1, "REV-001");
    bare.edges = Vec::new();
    let result = graph_with(vec![bare]).validate();
    let err = result.expect_err("a review with no edges must fail closed");
    assert!(
        matches!(
            err,
            CorpusError::Review(ReviewError::ReviewMissingReviewsEdge { ref review_uid })
                if review_uid == REV_1
        ),
        "a bare review node, got: {err:?}"
    );

    let mut orphan_correction = review_node(REV_2, "REV-002");
    orphan_correction.edges = vec![(EdgeKind::Supersedes, REV_1.to_string())];
    let result = graph_with(vec![review_node(REV_1, "REV-001"), orphan_correction]).validate();
    let err = result.expect_err("a supersession-only review must fail closed");
    assert!(
        matches!(
            err,
            CorpusError::Review(ReviewError::ReviewMissingReviewsEdge { ref review_uid })
                if review_uid == REV_2
        ),
        "a review with only a Supersedes edge, got: {err:?}"
    );
}

/// A review declaring two `Reviews` edges is rejected with the
/// duplicate-edge variant naming the count (TEST-133).
#[test]
fn review_node_with_two_reviews_edges_is_rejected() {
    let mut review = review_node(REV_1, "REV-001");
    review.edges = vec![
        (EdgeKind::Reviews, REQ_A.to_string()),
        (EdgeKind::Reviews, REQ_B.to_string()),
    ];
    let result = graph_with(vec![review]).validate();
    let err = result.expect_err("two Reviews edges must fail closed");
    assert!(
        matches!(
            err,
            CorpusError::Review(ReviewError::ReviewDuplicateReviewsEdge {
                ref review_uid,
                count: 2,
            }) if review_uid == REV_1
        ),
        "the error must name the review and the edge count, got: {err:?}"
    );
}

/// A supersession cycle of otherwise well-formed review nodes is
/// still rejected once the per-node invariants pass (TEST-133).
#[test]
fn supersession_cycle_remains_rejected_after_invariants() {
    let mut first = review_node(REV_1, "REV-001");
    first.edges.push((EdgeKind::Supersedes, REV_2.to_string()));
    let mut second = review_node(REV_2, "REV-002");
    second.edges.push((EdgeKind::Supersedes, REV_1.to_string()));
    let result = graph_with(vec![first, second]).validate();
    let err = result.expect_err("a supersession cycle must fail closed");
    assert!(
        matches!(
            err,
            CorpusError::Review(ReviewError::ReviewSupersessionCycle { ref uid }) if uid == REV_1
        ),
        "two-node cycle, got: {err:?}"
    );
}

/// A malformed review node fails before chain validation: a cyclic
/// pair whose second node also carries an unsupported content schema
/// reports the schema invariant, not the cycle (TEST-133).
#[test]
fn malformed_review_node_fails_before_chain_validation() {
    let mut first = review_node(REV_1, "REV-001");
    first.edges.push((EdgeKind::Supersedes, REV_2.to_string()));
    let mut second = review_node(REV_2, "REV-002");
    second.edges.push((EdgeKind::Supersedes, REV_1.to_string()));
    second.content_schema = 99;
    let result = graph_with(vec![first, second]).validate();
    let err = result.expect_err("a malformed cyclic pair must fail closed");
    assert!(
        matches!(
            err,
            CorpusError::Review(ReviewError::ReviewContentSchema {
                found: 99,
                supported: 1,
                ref uid,
                ..
            }) if uid == REV_2
        ),
        "node invariants run before chain validation, got: {err:?}"
    );
}

/// A programmatically built graph of valid review nodes — including
/// a same-reviewer correction chain — validates, so the invariants
/// do not over-fire (TEST-133).
#[test]
fn programmatically_built_valid_graph_validates() {
    let mut correction = review_node(REV_2, "REV-002");
    correction.decision = ReviewDecision::Reject;
    correction.rationale = Some("correcting my earlier approval".to_string());
    correction
        .edges
        .push((EdgeKind::Supersedes, REV_1.to_string()));
    let mut second_correction = review_node(REV_3, "REV-003");
    second_correction
        .edges
        .push((EdgeKind::Supersedes, REV_2.to_string()));
    graph_with(vec![
        review_node(REV_1, "REV-001"),
        correction,
        second_correction,
    ])
    .validate()
    .expect("a programmatically built valid review graph validates");
}
