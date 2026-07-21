//! Graph identity uniqueness, edge canonicalization, and
//! endpoint-kind validation tests (TEST-120).

use super::super::CorpusError;
use super::super::graph::{
    CorpusGraph, EdgeKind, Node, NodeKind, RequirementLayer, RequirementNode, TestNode,
};

fn requirement(uid: &str, id: &str, edges: &[&str]) -> Node {
    Node::Requirement(RequirementNode::new(
        uid.to_string(),
        id.to_string(),
        format!("title of {id}"),
        RequirementLayer::Sys,
        edges
            .iter()
            .map(|t| (EdgeKind::DerivesFrom, (*t).to_string()))
            .collect(),
    ))
}

fn graph_node(kind: NodeKind, uid: &str, id: &str, edges: Vec<(EdgeKind, String)>) -> Node {
    match kind {
        NodeKind::Requirement => Node::Requirement(RequirementNode::new(
            uid.to_string(),
            id.to_string(),
            format!("title of {id}"),
            RequirementLayer::Sys,
            edges,
        )),
        NodeKind::Test => Node::Test(TestNode {
            uid: uid.to_string(),
            id: id.to_string(),
            title: format!("title of {id}"),
            selectors: Vec::new(),
            edges,
        }),
        NodeKind::Review => unreachable!("review fixtures build ReviewNode directly"),
        NodeKind::SourceRevision => {
            unreachable!("source-revision fixtures build SourceRevisionNode directly")
        }
    }
}

#[test]
fn graph_rejects_duplicate_identities_and_edges() {
    let mut graph = CorpusGraph::new();
    graph.insert(requirement("req_dup", "R-1", &[])).unwrap();
    let err = graph
        .insert(requirement("req_dup", "R-2", &[]))
        .unwrap_err();
    assert!(
        matches!(err, CorpusError::DuplicateUid { ref uid } if uid == "req_dup"),
        "duplicate uid must be rejected naming the uid, got: {err:?}"
    );

    let err = graph
        .insert(requirement("req_other", "R-1", &[]))
        .unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::DuplicateHumanId {
                ref id,
                kind: NodeKind::Requirement,
                ref first_uid,
                ref duplicate_uid,
            } if id == "R-1" && first_uid == "req_dup" && duplicate_uid == "req_other"
        ),
        "duplicate human ids must be rejected within a node kind, got: {err:?}"
    );

    graph
        .insert(graph_node(NodeKind::Test, "test_one", "R-1", Vec::new()))
        .expect("the same human id is legal across different node kinds");
    let err = graph
        .insert(graph_node(NodeKind::Test, "test_two", "R-1", Vec::new()))
        .unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::DuplicateHumanId {
                kind: NodeKind::Test,
                ref first_uid,
                ref duplicate_uid,
                ..
            } if first_uid == "test_one" && duplicate_uid == "test_two"
        ),
        "duplicate human ids must also be rejected within tests, got: {err:?}"
    );

    let mut edge_graph = CorpusGraph::new();
    let err = edge_graph
        .insert(requirement(
            "req_child",
            "R-child",
            &["req_parent", "req_parent"],
        ))
        .unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::DuplicateEdge {
                ref from,
                ref to,
                kind: EdgeKind::DerivesFrom,
            } if from == "req_child" && to == "req_parent"
        ),
        "duplicate edges must be rejected with their owner and target, got: {err:?}"
    );
}

#[test]
fn graph_detects_dangling_and_invalid_edge_kinds() {
    let mut graph = CorpusGraph::new();
    graph
        .insert(requirement("req_child", "R-1", &["req_missing"]))
        .unwrap();
    let err = graph.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::DanglingEdge {
                ref from,
                ref to,
                kind: EdgeKind::DerivesFrom,
            } if from == "req_child" && to == "req_missing"
        ),
        "dangling edge must name source, target, and kind, got: {err:?}"
    );

    assert_invalid_edge_kinds(NodeKind::Requirement, EdgeKind::DerivesFrom, NodeKind::Test);
    assert_invalid_edge_kinds(NodeKind::Test, EdgeKind::DerivesFrom, NodeKind::Requirement);
    assert_invalid_edge_kinds(
        NodeKind::Requirement,
        EdgeKind::Verifies,
        NodeKind::Requirement,
    );
    assert_invalid_edge_kinds(NodeKind::Test, EdgeKind::Verifies, NodeKind::Test);
}

fn assert_invalid_edge_kinds(source_kind: NodeKind, edge_kind: EdgeKind, target_kind: NodeKind) {
    let mut graph = CorpusGraph::new();
    graph
        .insert(graph_node(target_kind, "target", "TARGET", Vec::new()))
        .unwrap();
    graph
        .insert(graph_node(
            source_kind,
            "source",
            "SOURCE",
            vec![(edge_kind, "target".to_string())],
        ))
        .unwrap();

    let err = graph.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::InvalidEdgeKinds {
                ref from,
                ref to,
                kind,
                source_kind: source,
                target_kind: target,
            } if from == "source"
                && to == "target"
                && kind == edge_kind
                && source == source_kind
                && target == target_kind
        ),
        "edge endpoint kinds must match the edge contract, got: {err:?}"
    );
}
