//! Shared fixtures for the lineage and transition test modules
//! (TEST-145, TEST-146, TEST-147, TEST-148). No `#[test]`
//! functions live here.

use crate::corpus::{
    CorpusGraph, EdgeKind, Node, RequirementLayer, RequirementNode, ReviewContentDigest,
    ReviewDecision, ReviewNode, ReviewTarget, SourceCapture, SourceContentDigest, SourceMaterial,
    SourceRevisionNode, TestNode,
};

pub(super) const SRC_A: &str = "src_00000000-0000-4000-8000-00000000000a";
pub(super) const SRC_B: &str = "src_00000000-0000-4000-8000-00000000000b";
pub(super) const SRC_C: &str = "src_00000000-0000-4000-8000-00000000000c";
pub(super) const SRC_D: &str = "src_00000000-0000-4000-8000-00000000000d";
pub(super) const SRC_E: &str = "src_00000000-0000-4000-8000-00000000000e";
pub(super) const REQ_A: &str = "req_00000000-0000-4000-8000-00000000000a";
pub(super) const REQ_B: &str = "req_00000000-0000-4000-8000-00000000000b";
pub(super) const REV_1: &str = "rev_00000000-0000-4000-8000-0000000000a1";
pub(super) const REV_2: &str = "rev_00000000-0000-4000-8000-0000000000a2";
pub(super) const REV_3: &str = "rev_00000000-0000-4000-8000-0000000000a3";
pub(super) const TEST_A: &str = "test_00000000-0000-4000-8000-00000000000a";
pub(super) const DOC_1: &str = "DOC-1";
pub(super) const DOC_2: &str = "DOC-2";
pub(super) const DIGEST_A: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const DIGEST_B: &str =
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// A source-revision node with vendored available material and an
/// optional supersedes pointer. Every descriptive field derives
/// from the uid so two nodes built with the same uid are
/// projection-identical unless a test mutates one.
pub(super) fn revision(uid: &str, document_key: &str, supersedes: Option<&str>) -> Node {
    let mut edges = Vec::new();
    if let Some(target) = supersedes {
        edges.push((EdgeKind::Supersedes, target.to_string()));
    }
    Node::SourceRevision(SourceRevisionNode {
        uid: uid.to_string(),
        id: format!("id of {uid}"),
        document_key: document_key.to_string(),
        title: format!("title of {uid}"),
        media_type: "application/pdf".to_string(),
        canonical_location: format!("https://example.org/specs/{uid}"),
        material: available_material(DIGEST_A),
        edges,
    })
}

/// Available vendored material with a fixed retrieval timestamp —
/// audit metadata, never ordering authority.
pub(super) fn available_material(digest: &str) -> SourceMaterial {
    material_at(digest, "2026-07-01T10:00:00Z")
}

/// Available vendored material with an explicit retrieval
/// timestamp, for the timestamp-independence tests.
pub(super) fn material_at(digest: &str, retrieved_at: &str) -> SourceMaterial {
    SourceMaterial::Available {
        retrieved_at: retrieved_at.to_string(),
        sha256: SourceContentDigest::from_hex(digest).unwrap(),
        capture: SourceCapture::Vendored {
            path: "sources/doc/rev.pdf".to_string(),
        },
    }
}

/// Available hash-only material with a fixed retrieval timestamp,
/// for capture-mutation fixtures.
pub(super) fn hash_only_material(digest: &str) -> SourceMaterial {
    SourceMaterial::Available {
        retrieved_at: "2026-07-01T10:00:00Z".to_string(),
        sha256: SourceContentDigest::from_hex(digest).unwrap(),
        capture: SourceCapture::HashOnly {},
    }
}

/// Unavailable material with a reason, for material-state
/// fixtures.
pub(super) fn unavailable_material(reason: &str) -> SourceMaterial {
    SourceMaterial::Unavailable {
        reason: reason.to_string(),
    }
}

/// Mutable access to the source revision inside a fixture node.
pub(super) fn source_node_mut(node: &mut Node) -> &mut SourceRevisionNode {
    match node {
        Node::SourceRevision(revision) => revision,
        other => unreachable!("fixture nodes here are source revisions: {other:?}"),
    }
}

/// A bare requirement node, for endpoint and non-source-difference
/// fixtures.
pub(super) fn requirement(uid: &str) -> Node {
    requirement_with_edges(uid, Vec::new())
}

/// A requirement node carrying explicit edges, for endpoint
/// fixtures.
pub(super) fn requirement_with_edges(uid: &str, edges: Vec<(EdgeKind, String)>) -> Node {
    Node::Requirement(RequirementNode::new(
        uid.to_string(),
        format!("id of {uid}"),
        "title".to_string(),
        RequirementLayer::Sys,
        edges,
    ))
}

/// A bare test-case node, for endpoint fixtures.
pub(super) fn test_node(uid: &str) -> Node {
    Node::Test(TestNode {
        uid: uid.to_string(),
        id: format!("id of {uid}"),
        title: "title".to_string(),
        selectors: Vec::new(),
        edges: Vec::new(),
    })
}

/// A review node over REQ_A with an optional supersession pointer;
/// reviewer, requirement, and digest agree across the fixture so
/// only the edge under test decides validation.
pub(super) fn review(uid: &str, supersedes: Option<&str>) -> Node {
    let mut edges = vec![(EdgeKind::Reviews, REQ_A.to_string())];
    if let Some(target) = supersedes {
        edges.push((EdgeKind::Supersedes, target.to_string()));
    }
    Node::Review(ReviewNode {
        uid: uid.to_string(),
        id: format!("id of {uid}"),
        target: ReviewTarget::Requirement(REQ_A.to_string()),
        content_schema: 1,
        reviewed_content_sha256: ReviewContentDigest::from_hex(DIGEST_A).unwrap(),
        decision: ReviewDecision::Approve,
        reviewer: "alice@example.com".to_string(),
        reviewed_at: "2026-07-01T10:00:00Z".to_string(),
        rationale: None,
        edges,
    })
}

/// Insert every node into a fresh graph; fixtures are valid by
/// construction at insert time (validation is the behavior under
/// test).
pub(super) fn graph_of(nodes: Vec<Node>) -> CorpusGraph {
    let mut graph = CorpusGraph::new();
    for node in nodes {
        graph.insert(node).unwrap();
    }
    graph
}

/// The linear chain A <- B (B supersedes A) under `document_key`.
pub(super) fn two_chain(document_key: &str) -> Vec<Node> {
    vec![
        revision(SRC_A, document_key, None),
        revision(SRC_B, document_key, Some(SRC_A)),
    ]
}

/// The linear chain A <- B <- C (B supersedes A, C supersedes B)
/// under `document_key`.
pub(super) fn three_chain(document_key: &str) -> Vec<Node> {
    vec![
        revision(SRC_A, document_key, None),
        revision(SRC_B, document_key, Some(SRC_A)),
        revision(SRC_C, document_key, Some(SRC_B)),
    ]
}
