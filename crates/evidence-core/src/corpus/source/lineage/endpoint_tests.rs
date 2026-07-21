//! Tests for the source supersedes edge endpoint contract
//! (TEST-145).

use super::fixtures::*;
use crate::corpus::{CorpusError, EdgeKind, NodeKind};

/// A source revision may supersede a source revision; the chain
/// validates (TEST-145).
#[test]
fn source_supersedes_source_satisfies_endpoint_contract() {
    let graph = graph_of(three_chain(DOC_1));
    assert!(
        graph.validate().is_ok(),
        "a linear source supersedes chain must validate"
    );
}

/// One endpoint-contract case: the node owning the edge under
/// test, the other graph members, and the expected kinds.
struct EndpointCase {
    case: &'static str,
    owner: crate::corpus::Node,
    targets: Vec<crate::corpus::Node>,
    edge: EdgeKind,
    source_kind: NodeKind,
    target_kind: NodeKind,
}

/// An endpoint case with a source revision owning the edge under
/// test.
fn source_case(
    case: &'static str,
    edge: EdgeKind,
    target_uid: &str,
    targets: Vec<crate::corpus::Node>,
    target_kind: NodeKind,
) -> EndpointCase {
    EndpointCase {
        case,
        owner: edge_node(SRC_A, edge, target_uid),
        targets,
        edge,
        source_kind: NodeKind::SourceRevision,
        target_kind,
    }
}

/// Every other edge combination touching a source revision fails
/// with `InvalidEdgeKinds` naming both endpoints and their kinds
/// (TEST-145).
#[test]
fn non_source_supersedes_endpoints_fail_closed() {
    let cases = vec![
        source_case(
            "source supersedes requirement",
            EdgeKind::Supersedes,
            REQ_A,
            vec![requirement(REQ_A)],
            NodeKind::Requirement,
        ),
        source_case(
            "source supersedes review",
            EdgeKind::Supersedes,
            REV_1,
            vec![requirement(REQ_A), review(REV_1, None)],
            NodeKind::Review,
        ),
        source_case(
            "source supersedes test",
            EdgeKind::Supersedes,
            TEST_A,
            vec![test_node(TEST_A)],
            NodeKind::Test,
        ),
        EndpointCase {
            case: "review supersedes source",
            owner: review(REV_1, Some(SRC_A)),
            targets: vec![requirement(REQ_A), revision(SRC_A, DOC_1, None)],
            edge: EdgeKind::Supersedes,
            source_kind: NodeKind::Review,
            target_kind: NodeKind::SourceRevision,
        },
        EndpointCase {
            case: "requirement supersedes source",
            owner: requirement_with_edges(REQ_A, vec![(EdgeKind::Supersedes, SRC_A.to_string())]),
            targets: vec![revision(SRC_A, DOC_1, None)],
            edge: EdgeKind::Supersedes,
            source_kind: NodeKind::Requirement,
            target_kind: NodeKind::SourceRevision,
        },
        source_case(
            "source reviews requirement",
            EdgeKind::Reviews,
            REQ_A,
            vec![requirement(REQ_A)],
            NodeKind::Requirement,
        ),
        source_case(
            "source verifies requirement",
            EdgeKind::Verifies,
            REQ_A,
            vec![requirement(REQ_A)],
            NodeKind::Requirement,
        ),
        source_case(
            "source derives from source",
            EdgeKind::DerivesFrom,
            SRC_B,
            vec![revision(SRC_B, DOC_1, None)],
            NodeKind::SourceRevision,
        ),
    ];
    for case in cases {
        let mut nodes = case.targets;
        nodes.push(case.owner);
        let err = graph_of(nodes).validate().unwrap_err();
        assert!(
            matches!(
                err,
                CorpusError::InvalidEdgeKinds {
                    kind,
                    source_kind: found_source,
                    target_kind: found_target,
                    ..
                } if kind == case.edge
                    && found_source == case.source_kind
                    && found_target == case.target_kind
            ),
            "{} must fail with InvalidEdgeKinds naming kinds, got: {err:?}",
            case.case,
        );
    }
}

/// Build a source-revision node owning one edge of `edge` kind.
fn edge_node(uid: &str, edge: EdgeKind, target: &str) -> crate::corpus::Node {
    let mut node = revision(uid, DOC_1, None);
    let crate::corpus::Node::SourceRevision(revision) = &mut node else {
        unreachable!("revision() builds a source revision")
    };
    revision.edges = vec![(edge, target.to_string())];
    node
}

/// A supersedes edge naming an absent uid fails as `DanglingEdge`
/// before domain validation runs (TEST-145).
#[test]
fn dangling_source_supersedes_fails_closed() {
    let graph = graph_of(vec![
        revision(SRC_A, DOC_1, None),
        revision(
            SRC_B,
            DOC_1,
            Some("src_00000000-0000-4000-8000-000000000099"),
        ),
    ]);
    let err = graph.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::DanglingEdge {
                ref from,
                ref to,
                kind: EdgeKind::Supersedes,
            } if from == SRC_B && to == "src_00000000-0000-4000-8000-000000000099"
        ),
        "a dangling source supersedes must fail with DanglingEdge, got: {err:?}"
    );
}
