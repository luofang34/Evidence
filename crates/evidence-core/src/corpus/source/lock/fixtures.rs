//! Shared fixtures for the sources-lock test modules (TEST-149,
//! TEST-150, TEST-151). No `#[test]` functions live here.

use crate::corpus::{
    CorpusGraph, EdgeKind, Node, SourceCapture, SourceContentDigest, SourceMaterial,
    SourceRevisionNode,
};

pub(super) const SRC_1: &str = "src_00000000-0000-4000-8000-0000000000a1";
pub(super) const SRC_2: &str = "src_00000000-0000-4000-8000-0000000000a2";
pub(super) const SRC_3: &str = "src_00000000-0000-4000-8000-0000000000a3";
pub(super) const SRC_4: &str = "src_00000000-0000-4000-8000-0000000000a4";
pub(super) const SRC_1H: &str = "src_00000000-0000-4000-8000-0000000000b1";
pub(super) const DOC_1: &str = "DOC-1";
pub(super) const DOC_2: &str = "DOC-2";
pub(super) const DOC_3: &str = "DOC-3";
pub(super) const DOC_4: &str = "DOC-4";
pub(super) const DIGEST_A: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const DIGEST_B: &str =
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub(super) const DIGEST_C: &str =
    "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
pub(super) const VENDORED_PATH: &str = "sources/doc-1/rev-c.pdf";
pub(super) const RETRIEVED_AT: &str = "2026-07-01T10:00:00Z";
pub(super) const UNAVAILABLE_REASON: &str = "export control blocks capture";

/// One revision of `document_key` with vendored available material.
pub(super) fn vendored_revision(uid: &str, document_key: &str, digest: &str) -> Node {
    revision_with_material(
        uid,
        document_key,
        SourceMaterial::Available {
            retrieved_at: RETRIEVED_AT.to_string(),
            sha256: SourceContentDigest::from_hex(digest).unwrap(),
            capture: SourceCapture::Vendored {
                path: VENDORED_PATH.to_string(),
            },
        },
    )
}

/// One revision of `document_key` with hash-only available material.
pub(super) fn hash_only_revision(uid: &str, document_key: &str, digest: &str) -> Node {
    revision_with_material(
        uid,
        document_key,
        SourceMaterial::Available {
            retrieved_at: RETRIEVED_AT.to_string(),
            sha256: SourceContentDigest::from_hex(digest).unwrap(),
            capture: SourceCapture::HashOnly {},
        },
    )
}

/// One revision of `document_key` with external-controlled available
/// material.
pub(super) fn external_revision(
    uid: &str,
    document_key: &str,
    digest: &str,
    system: &str,
    immutable_id: &str,
) -> Node {
    revision_with_material(
        uid,
        document_key,
        SourceMaterial::Available {
            retrieved_at: RETRIEVED_AT.to_string(),
            sha256: SourceContentDigest::from_hex(digest).unwrap(),
            capture: SourceCapture::ExternalControlled {
                system: system.to_string(),
                immutable_id: immutable_id.to_string(),
            },
        },
    )
}

/// One revision of `document_key` with unavailable material.
pub(super) fn unavailable_revision(uid: &str, document_key: &str) -> Node {
    revision_with_material(
        uid,
        document_key,
        SourceMaterial::Unavailable {
            reason: UNAVAILABLE_REASON.to_string(),
        },
    )
}

/// A revision node with the given material and no edges.
fn revision_with_material(uid: &str, document_key: &str, material: SourceMaterial) -> Node {
    Node::SourceRevision(SourceRevisionNode {
        uid: uid.to_string(),
        id: format!("id of {uid}"),
        document_key: document_key.to_string(),
        title: format!("title of {uid}"),
        media_type: "application/pdf".to_string(),
        canonical_location: format!("https://example.org/specs/{uid}"),
        material,
        edges: Vec::new(),
    })
}

/// Mutable access to the source revision inside a fixture node.
pub(super) fn source_node_mut(node: &mut Node) -> &mut SourceRevisionNode {
    match node {
        Node::SourceRevision(revision) => revision,
        other => unreachable!("fixture nodes here are source revisions: {other:?}"),
    }
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

/// The documented four-document fixture graph covering every capture
/// mode plus an unavailable head, with one historical superseded
/// revision under DOC-1 (SRC_1H supersedes SRC_1, so SRC_1 is never
/// an active lock entry):
///
/// - DOC-1: vendored, chain SRC_1 → SRC_1H (head SRC_1H)
/// - DOC-2: hash-only, single revision SRC_2
/// - DOC-3: external-controlled, single revision SRC_3
/// - DOC-4: unavailable, single revision SRC_4
pub(super) fn four_document_nodes() -> Vec<Node> {
    let mut historical = vendored_revision(SRC_1, DOC_1, DIGEST_A);
    let mut head = vendored_revision(SRC_1H, DOC_1, DIGEST_A);
    source_node_mut(&mut head)
        .edges
        .push((EdgeKind::Supersedes, SRC_1.to_string()));
    // The historical revision keeps a different title so a layout
    // slip into the lock would be visible.
    source_node_mut(&mut historical).title = "historical DOC-1 rev B".to_string();
    vec![
        historical,
        head,
        hash_only_revision(SRC_2, DOC_2, DIGEST_B),
        external_revision(SRC_3, DOC_3, DIGEST_C, "plm-hd", "DOC-3@revC"),
        unavailable_revision(SRC_4, DOC_4),
    ]
}

/// The four-document fixture graph in canonical node order.
pub(super) fn four_document_graph() -> CorpusGraph {
    graph_of(four_document_nodes())
}
