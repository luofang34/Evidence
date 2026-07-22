//! Shared fixtures for the source-verification test modules
//! (TEST-153, TEST-154, TEST-155). No `#[test]` functions live
//! here.

use std::path::Path;

use crate::corpus::{
    CorpusGraph, Node, SourceCapture, SourceContentDigest, SourceMaterial, SourceRevisionNode,
    derive_lock, render_lock_canonical,
};

pub(super) const SRC_1: &str = "src_00000000-0000-4000-8000-0000000000a1";
pub(super) const SRC_2: &str = "src_00000000-0000-4000-8000-0000000000a2";
pub(super) const SRC_3: &str = "src_00000000-0000-4000-8000-0000000000a3";
pub(super) const SRC_4: &str = "src_00000000-0000-4000-8000-0000000000a4";
pub(super) const SRC_5: &str = "src_00000000-0000-4000-8000-0000000000a5";
pub(super) const DOC_1: &str = "DOC-1";
pub(super) const DOC_2: &str = "DOC-2";
pub(super) const DOC_3: &str = "DOC-3";
pub(super) const DOC_4: &str = "DOC-4";
pub(super) const DOC_5: &str = "DOC-5";
pub(super) const VENDORED_PATH: &str = "sources/doc-1/rev-c.pdf";
pub(super) const PAYLOAD_BYTES: &[u8] = b"DOC-1 rev C payload bytes\n";
pub(super) const RETRIEVED_AT: &str = "2026-07-01T10:00:00Z";
pub(super) const UNAVAILABLE_REASON: &str = "export control blocks capture";

/// The lowercase SHA-256 of `bytes` — the digest a matching record
/// and lock must declare.
pub(super) fn digest_of(bytes: &[u8]) -> String {
    crate::hash::sha256(bytes)
}

/// A source-revision node with the given material and no edges.
/// The canonical location is an `https://` URL on purpose: the
/// fixtures double as proof that URL-located records verify
/// offline.
pub(super) fn revision_node(
    uid: &str,
    document_key: &str,
    material: SourceMaterial,
) -> SourceRevisionNode {
    SourceRevisionNode {
        uid: uid.to_string(),
        id: format!("id of {uid}"),
        document_key: document_key.to_string(),
        title: format!("title of {uid}"),
        media_type: "application/pdf".to_string(),
        canonical_location: format!("https://example.org/specs/{uid}"),
        material,
        edges: Vec::new(),
    }
}

/// Vendored available material at `wire_path`.
pub(super) fn vendored_material(digest: &str, wire_path: &str) -> SourceMaterial {
    SourceMaterial::Available {
        retrieved_at: RETRIEVED_AT.to_string(),
        sha256: SourceContentDigest::from_hex(digest).unwrap(),
        capture: SourceCapture::Vendored {
            path: wire_path.to_string(),
        },
    }
}

/// One revision with vendored available material at `wire_path`.
pub(super) fn vendored_revision(
    uid: &str,
    document_key: &str,
    digest: &str,
    wire_path: &str,
) -> Node {
    Node::SourceRevision(revision_node(
        uid,
        document_key,
        vendored_material(digest, wire_path),
    ))
}

/// One revision with hash-only available material.
pub(super) fn hash_only_revision(uid: &str, document_key: &str, digest: &str) -> Node {
    Node::SourceRevision(revision_node(
        uid,
        document_key,
        SourceMaterial::Available {
            retrieved_at: RETRIEVED_AT.to_string(),
            sha256: SourceContentDigest::from_hex(digest).unwrap(),
            capture: SourceCapture::HashOnly {},
        },
    ))
}

/// One revision with external-controlled available material.
pub(super) fn external_revision(uid: &str, document_key: &str, digest: &str) -> Node {
    Node::SourceRevision(revision_node(
        uid,
        document_key,
        SourceMaterial::Available {
            retrieved_at: RETRIEVED_AT.to_string(),
            sha256: SourceContentDigest::from_hex(digest).unwrap(),
            capture: SourceCapture::ExternalControlled {
                system: "plm-hd".to_string(),
                immutable_id: format!("{document_key}@revC"),
            },
        },
    ))
}

/// One revision with unavailable material.
pub(super) fn unavailable_revision(uid: &str, document_key: &str) -> Node {
    Node::SourceRevision(revision_node(
        uid,
        document_key,
        SourceMaterial::Unavailable {
            reason: UNAVAILABLE_REASON.to_string(),
        },
    ))
}

/// A tempdir corpus root with payloads written beneath `sources/`,
/// the graph holding `nodes`, and its canonical derived lock bytes.
/// The graph is NOT validated here — verification runs the gates —
/// so fixtures for invalid graphs build through the same path.
pub(super) struct FixtureCorpus {
    /// The tempdir backing `root()`; kept alive for the test.
    pub(super) dir: tempfile::TempDir,
    /// The corpus graph built from the fixture nodes.
    pub(super) graph: CorpusGraph,
    /// The canonical rendering of the graph's derived lock.
    pub(super) lock_bytes: Vec<u8>,
}

impl FixtureCorpus {
    /// The corpus root the payload paths resolve against.
    pub(super) fn root(&self) -> &Path {
        self.dir.path()
    }
}

/// Build a corpus: insert the nodes, write each `(wire path, bytes)`
/// payload beneath the root, and derive + render the lock.
pub(super) fn corpus_of(nodes: Vec<Node>, payloads: &[(&str, &[u8])]) -> FixtureCorpus {
    let dir = tempfile::tempdir().unwrap();
    let mut graph = CorpusGraph::new();
    for node in nodes {
        graph.insert(node).unwrap();
    }
    for (wire_path, bytes) in payloads {
        write_payload(dir.path(), wire_path, bytes);
    }
    let lock_bytes = render_lock_canonical(&derive_lock(&graph));
    FixtureCorpus {
        dir,
        graph,
        lock_bytes,
    }
}

/// Write one payload at its corpus-root-relative wire path,
/// creating parent directories.
pub(super) fn write_payload(root: &Path, wire_path: &str, bytes: &[u8]) {
    let path = root.join(wire_path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// The documented four-document fixture graph covering every
/// capture mode plus an unavailable head: DOC-1 vendored, DOC-2
/// hash-only, DOC-3 external-controlled, DOC-4 unavailable.
pub(super) fn four_document_nodes(vendored_digest: &str) -> Vec<Node> {
    vec![
        vendored_revision(SRC_1, DOC_1, vendored_digest, VENDORED_PATH),
        hash_only_revision(SRC_2, DOC_2, &"b".repeat(64)),
        external_revision(SRC_3, DOC_3, &"c".repeat(64)),
        unavailable_revision(SRC_4, DOC_4),
    ]
}
