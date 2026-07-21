//! Tests for review edge endpoint contracts and deterministic
//! supersession chain validation (TEST-133).

use crate::corpus::{
    CorpusError, CorpusGraph, EdgeKind, Node, NodeKind, RequirementLayer, RequirementNode,
    ReviewContentDigest, ReviewDecision, ReviewError, ReviewNode, TestNode,
};

const REQ_A: &str = "req_00000000-0000-4000-8000-00000000000a";
const REQ_B: &str = "req_00000000-0000-4000-8000-00000000000b";
const REV_1: &str = "rev_00000000-0000-4000-8000-0000000000a1";
const REV_2: &str = "rev_00000000-0000-4000-8000-0000000000a2";
const REV_3: &str = "rev_00000000-0000-4000-8000-0000000000a3";
const REV_4: &str = "rev_00000000-0000-4000-8000-0000000000a4";

/// A review node by `alice@example.com` over REQ_A's `a*64` digest,
/// with an optional supersession pointer.
fn review_node(uid: &str, id: &str, supersedes: Option<&str>) -> Node {
    review_node_with(
        uid,
        id,
        "alice@example.com",
        REQ_A,
        &"a".repeat(64),
        supersedes,
    )
}

fn review_node_with(
    uid: &str,
    id: &str,
    reviewer: &str,
    requirement_uid: &str,
    digest: &str,
    supersedes: Option<&str>,
) -> Node {
    let mut edges = vec![(EdgeKind::Reviews, requirement_uid.to_string())];
    if let Some(target) = supersedes {
        edges.push((EdgeKind::Supersedes, target.to_string()));
    }
    Node::Review(ReviewNode {
        uid: uid.to_string(),
        id: id.to_string(),
        requirement_uid: requirement_uid.to_string(),
        content_schema: 1,
        reviewed_content_sha256: ReviewContentDigest::from_hex(digest).unwrap(),
        decision: ReviewDecision::Approve,
        reviewer: reviewer.to_string(),
        reviewed_at: "2026-07-01T10:00:00Z".to_string(),
        rationale: None,
        edges,
    })
}

fn graph_node(kind: NodeKind, uid: &str, edges: Vec<(EdgeKind, String)>) -> Node {
    match kind {
        NodeKind::Requirement => Node::Requirement(RequirementNode::new(
            uid.to_string(),
            format!("id of {uid}"),
            "title".to_string(),
            RequirementLayer::Sys,
            edges,
        )),
        NodeKind::Test => Node::Test(TestNode {
            uid: uid.to_string(),
            id: format!("id of {uid}"),
            title: "title".to_string(),
            selectors: Vec::new(),
            edges,
        }),
        NodeKind::Review => Node::Review(ReviewNode {
            uid: uid.to_string(),
            id: format!("id of {uid}"),
            // Field/edge agreement: the requirement field names the
            // `Reviews` edge target when the fixture carries one.
            requirement_uid: edges
                .iter()
                .find(|(kind, _)| *kind == EdgeKind::Reviews)
                .map_or_else(|| REQ_A.to_string(), |(_, target)| target.clone()),
            content_schema: 1,
            reviewed_content_sha256: ReviewContentDigest::from_hex(&"a".repeat(64)).unwrap(),
            decision: ReviewDecision::Approve,
            reviewer: "alice@example.com".to_string(),
            reviewed_at: "2026-07-01T10:00:00Z".to_string(),
            rationale: None,
            edges,
        }),
    }
}

/// Build a graph with one edge under test and report whether
/// `validate` accepts it. Review nodes also carry the `Reviews`
/// edge their per-node invariants require (the edge under test
/// doubles as it when it is itself a `Reviews` edge), and both
/// reviews share reviewer, requirement, and digest, so only the
/// endpoint contract of the edge under test decides the outcome.
fn edge_contract_holds(source: NodeKind, edge: EdgeKind, target: NodeKind) -> bool {
    let fixture_edges = |kind: NodeKind| match kind {
        NodeKind::Review => vec![(EdgeKind::Reviews, REQ_A.to_string())],
        _ => Vec::new(),
    };
    let mut graph = CorpusGraph::new();
    graph
        .insert(graph_node(NodeKind::Requirement, REQ_A, Vec::new()))
        .unwrap();
    graph
        .insert(graph_node(target, "target", fixture_edges(target)))
        .unwrap();
    let mut source_edges = match (source, edge) {
        (NodeKind::Review, EdgeKind::Reviews) => Vec::new(),
        _ => fixture_edges(source),
    };
    source_edges.push((edge, "target".to_string()));
    graph
        .insert(graph_node(source, "source", source_edges))
        .unwrap();
    graph.validate().is_ok()
}

/// Only Review→Requirement `Reviews` and Review→Review `Supersedes`
/// satisfy the endpoint contract (TEST-133).
#[test]
fn review_edges_enforce_endpoint_kinds() {
    let valid: [(NodeKind, EdgeKind, NodeKind); 2] = [
        (NodeKind::Review, EdgeKind::Reviews, NodeKind::Requirement),
        (NodeKind::Review, EdgeKind::Supersedes, NodeKind::Review),
    ];
    for (source_kind, edge_kind, target_kind) in valid {
        assert!(
            edge_contract_holds(source_kind, edge_kind, target_kind),
            "{source_kind:?}-{edge_kind:?}->{target_kind:?} must be accepted"
        );
    }

    let invalid: [(NodeKind, EdgeKind, NodeKind); 9] = [
        (NodeKind::Review, EdgeKind::Reviews, NodeKind::Review),
        (NodeKind::Review, EdgeKind::Reviews, NodeKind::Test),
        (
            NodeKind::Review,
            EdgeKind::Supersedes,
            NodeKind::Requirement,
        ),
        (NodeKind::Review, EdgeKind::Supersedes, NodeKind::Test),
        (
            NodeKind::Review,
            EdgeKind::DerivesFrom,
            NodeKind::Requirement,
        ),
        (NodeKind::Review, EdgeKind::Verifies, NodeKind::Requirement),
        (NodeKind::Requirement, EdgeKind::Reviews, NodeKind::Review),
        (
            NodeKind::Requirement,
            EdgeKind::DerivesFrom,
            NodeKind::Review,
        ),
        (NodeKind::Test, EdgeKind::Verifies, NodeKind::Review),
    ];
    for (source_kind, edge_kind, target_kind) in invalid {
        assert!(
            !edge_contract_holds(source_kind, edge_kind, target_kind),
            "{source_kind:?}-{edge_kind:?}->{target_kind:?} must be rejected"
        );
    }

    let mut graph = CorpusGraph::new();
    graph
        .insert(graph_node(
            NodeKind::Review,
            "source",
            vec![(EdgeKind::Reviews, "target".to_string())],
        ))
        .unwrap();
    graph
        .insert(graph_node(NodeKind::Review, "target", Vec::new()))
        .unwrap();
    let err = graph.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::InvalidEdgeKinds {
                ref from,
                ref to,
                kind: EdgeKind::Reviews,
                source_kind: NodeKind::Review,
                target_kind: NodeKind::Review,
            } if from == "source" && to == "target"
        ),
        "the error must name endpoints and kinds, got: {err:?}"
    );
}

/// A graph with both requirements plus the given review nodes.
fn chain_graph(reviews: Vec<Node>) -> CorpusGraph {
    let mut graph = CorpusGraph::new();
    graph
        .insert(graph_node(NodeKind::Requirement, REQ_A, Vec::new()))
        .unwrap();
    graph
        .insert(graph_node(NodeKind::Requirement, REQ_B, Vec::new()))
        .unwrap();
    for review in reviews {
        graph.insert(review).unwrap();
    }
    graph
}

/// Dangling, self-referential, cyclic, forked, and cross-binding
/// supersession chains are rejected deterministically (TEST-133).
#[test]
fn supersession_validation_rejects_invalid_chains() {
    let dangling = chain_graph(vec![
        review_node(REV_1, "REV-001", None),
        review_node(REV_2, "REV-002", Some(REV_4)),
    ]);
    let err = dangling.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::DanglingEdge {
                kind: EdgeKind::Supersedes,
                ref to,
                ..
            } if to == REV_4
        ),
        "dangling supersession target, got: {err:?}"
    );

    let self_loop = chain_graph(vec![review_node(REV_1, "REV-001", Some(REV_1))]);
    let err = self_loop.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::Review(ReviewError::ReviewSupersessionSelf { ref uid }) if uid == REV_1
        ),
        "self-supersession, got: {err:?}"
    );

    let cycle = chain_graph(vec![
        review_node(REV_1, "REV-001", Some(REV_2)),
        review_node(REV_2, "REV-002", Some(REV_1)),
    ]);
    let err = cycle.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::Review(ReviewError::ReviewSupersessionCycle { ref uid }) if uid == REV_1
        ),
        "two-node cycle, got: {err:?}"
    );

    let fork = chain_graph(vec![
        review_node(REV_1, "REV-001", None),
        review_node(REV_2, "REV-002", Some(REV_1)),
        review_node(REV_3, "REV-003", Some(REV_1)),
    ]);
    let err = fork.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::Review(ReviewError::ReviewSupersessionFork {
                ref uid,
                ref first_uid,
                ref second_uid,
            }) if uid == REV_1 && first_uid == REV_2 && second_uid == REV_3
        ),
        "fork names the superseded review and both successors, got: {err:?}"
    );

    let cross_reviewer = chain_graph(vec![
        review_node(REV_1, "REV-001", None),
        review_node_with(
            REV_2,
            "REV-002",
            "bob@example.com",
            REQ_A,
            &"a".repeat(64),
            Some(REV_1),
        ),
    ]);
    let err = cross_reviewer.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::Review(ReviewError::ReviewSupersessionReviewer {
                ref uid,
                ref predecessor_uid,
            }) if uid == REV_2 && predecessor_uid == REV_1
        ),
        "cross-reviewer supersession, got: {err:?}"
    );

    let cross_requirement = chain_graph(vec![
        review_node(REV_1, "REV-001", None),
        review_node_with(
            REV_2,
            "REV-002",
            "alice@example.com",
            REQ_B,
            &"a".repeat(64),
            Some(REV_1),
        ),
    ]);
    let err = cross_requirement.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::Review(ReviewError::ReviewSupersessionRequirement {
                ref uid,
                ref predecessor_uid,
            }) if uid == REV_2 && predecessor_uid == REV_1
        ),
        "cross-requirement supersession, got: {err:?}"
    );

    let cross_digest = chain_graph(vec![
        review_node(REV_1, "REV-001", None),
        review_node_with(
            REV_2,
            "REV-002",
            "alice@example.com",
            REQ_A,
            &"b".repeat(64),
            Some(REV_1),
        ),
    ]);
    let err = cross_digest.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::Review(ReviewError::ReviewSupersessionDigest {
                ref uid,
                ref predecessor_uid,
            }) if uid == REV_2 && predecessor_uid == REV_1
        ),
        "cross-digest supersession, got: {err:?}"
    );

    let valid_chain = chain_graph(vec![
        review_node(REV_1, "REV-001", None),
        review_node(REV_2, "REV-002", Some(REV_1)),
        review_node(REV_3, "REV-003", Some(REV_2)),
    ]);
    valid_chain
        .validate()
        .expect("a linear same-reviewer correction chain validates");
}

/// A review carrying two outgoing `Supersedes` edges is rejected
/// even when both predecessors satisfy every per-edge invariant;
/// the single-supersession chain still validates (TEST-133). The
/// record loader cannot produce this shape — a record names a
/// single optional `supersedes_review_uid` — so the invariant
/// guards programmatic construction.
#[test]
fn review_with_two_outgoing_supersedes_edges_is_rejected() {
    let mut double = review_node(REV_3, "REV-003", Some(REV_1));
    let Node::Review(review) = &mut double else {
        unreachable!("review_node builds a review node")
    };
    review.edges.push((EdgeKind::Supersedes, REV_2.to_string()));
    let multi = chain_graph(vec![
        review_node(REV_1, "REV-001", None),
        review_node(REV_2, "REV-002", None),
        double,
    ]);
    let err = multi.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::Review(ReviewError::ReviewDuplicateSupersedesEdge {
                ref review_uid,
                count: 2,
            }) if review_uid == REV_3
        ),
        "two outgoing supersession edges name the review and the count, got: {err:?}"
    );

    let valid_chain = chain_graph(vec![
        review_node(REV_1, "REV-001", None),
        review_node(REV_2, "REV-002", Some(REV_1)),
    ]);
    valid_chain
        .validate()
        .expect("a single supersession still validates");
}
