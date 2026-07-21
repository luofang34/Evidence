//! Sources lock: golden byte-lock of the canonical v1 rendering,
//! layout-independent byte identity, and full three-gate validation
//! of the committed bytes (TEST-152).
//!
//! The committed `sources_lock_v1.golden` byte-locks the complete
//! canonical rendering of the documented four-document fixture
//! graph — one vendored head (with a superseded historical revision
//! that never enters the lock), one hash-only head, one
//! external-controlled head, and one unavailable head — so entry
//! order, field order, quoting, whitespace, blank-line placement,
//! and the trailing newline are all pinned. Regenerate with
//! `EVIDENCE_UPDATE_FIXTURES=1`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::{Path, PathBuf};

use evidence_core::corpus::{
    CorpusGraph, EdgeKind, Node, SourceCapture, SourceContentDigest, SourceMaterial,
    SourceRevisionNode, derive_lock, render_lock_canonical, validate_committed_lock,
};

const SRC_1: &str = "src_00000000-0000-4000-8000-0000000000a1";
const SRC_1H: &str = "src_00000000-0000-4000-8000-0000000000b1";
const SRC_2: &str = "src_00000000-0000-4000-8000-0000000000a2";
const SRC_3: &str = "src_00000000-0000-4000-8000-0000000000a3";
const SRC_4: &str = "src_00000000-0000-4000-8000-0000000000a4";
const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus/sources_lock_v1.golden")
}

fn revision(uid: &str, document_key: &str, material: SourceMaterial) -> Node {
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

fn available(digest: &str, capture: SourceCapture) -> SourceMaterial {
    SourceMaterial::Available {
        retrieved_at: "2026-07-01T10:00:00Z".to_string(),
        sha256: SourceContentDigest::from_hex(digest).expect("fixture digest"),
        capture,
    }
}

/// The documented fixture nodes, in canonical insertion order:
///
/// - DOC-1: vendored, chain SRC_1 → SRC_1H (head SRC_1H; the
///   superseded SRC_1 stays in the registry but never in the lock)
/// - DOC-2: hash-only, head SRC_2
/// - DOC-3: external-controlled, head SRC_3
/// - DOC-4: unavailable, head SRC_4
fn fixture_nodes() -> Vec<Node> {
    let historical = revision(
        SRC_1,
        "DOC-1",
        available(
            DIGEST_A,
            SourceCapture::Vendored {
                path: "sources/doc-1/rev-b.pdf".to_string(),
            },
        ),
    );
    let head = Node::SourceRevision(SourceRevisionNode {
        uid: SRC_1H.to_string(),
        id: format!("id of {SRC_1H}"),
        document_key: "DOC-1".to_string(),
        title: format!("title of {SRC_1H}"),
        media_type: "application/pdf".to_string(),
        canonical_location: format!("https://example.org/specs/{SRC_1H}"),
        material: available(
            DIGEST_A,
            SourceCapture::Vendored {
                path: "sources/doc-1/rev-c.pdf".to_string(),
            },
        ),
        edges: vec![(EdgeKind::Supersedes, SRC_1.to_string())],
    });
    vec![
        historical,
        head,
        revision(
            SRC_2,
            "DOC-2",
            available(DIGEST_B, SourceCapture::HashOnly {}),
        ),
        revision(
            SRC_3,
            "DOC-3",
            available(
                DIGEST_C,
                SourceCapture::ExternalControlled {
                    system: "plm-hd".to_string(),
                    immutable_id: "DOC-3@revC".to_string(),
                },
            ),
        ),
        revision(
            SRC_4,
            "DOC-4",
            SourceMaterial::Unavailable {
                reason: "export control blocks capture".to_string(),
            },
        ),
    ]
}

fn fixture_graph() -> CorpusGraph {
    let mut graph = CorpusGraph::new();
    for node in fixture_nodes() {
        graph.insert(node).expect("insert fixture node");
    }
    graph.validate().expect("fixture graph validates");
    graph
}

/// The committed golden byte-locks the canonical rendering of the
/// fixture graph's derived lock (TEST-152).
#[test]
fn golden_fixture_byte_locks_canonical_lock_bytes() {
    let graph = fixture_graph();
    let rendered = render_lock_canonical(&derive_lock(&graph));
    let path = golden_path();

    if std::env::var_os("EVIDENCE_UPDATE_FIXTURES").is_some() {
        fs::write(&path, &rendered).expect("write fixture");
        return;
    }

    let committed = fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "missing or unreadable fixture {}: {e}\n\
             hint: run with EVIDENCE_UPDATE_FIXTURES=1 to write it",
            path.display()
        )
    });
    assert_eq!(
        committed, rendered,
        "canonical sources.lock rendering drifted — the byte contract is locked; \
         an intentional change requires a new lock schema version"
    );
}

/// Equivalent graphs — same nodes in a different insertion order —
/// render byte-identical canonical locks (TEST-152).
#[test]
fn equivalent_graphs_render_byte_identical_locks() {
    let forward = fixture_graph();
    let mut reversed_nodes = fixture_nodes();
    reversed_nodes.reverse();
    let mut reversed = CorpusGraph::new();
    for node in reversed_nodes {
        reversed.insert(node).expect("insert fixture node");
    }
    reversed.validate().expect("reversed graph validates");
    assert_eq!(
        render_lock_canonical(&derive_lock(&reversed)),
        render_lock_canonical(&derive_lock(&forward)),
        "insertion order must never reach the canonical bytes"
    );
}

/// The committed golden validates against its graph through the
/// full three-gate check (TEST-152).
#[test]
fn golden_lock_validates_against_its_graph() {
    let graph = fixture_graph();
    let path = golden_path();
    if std::env::var_os("EVIDENCE_UPDATE_FIXTURES").is_some() {
        let rendered = render_lock_canonical(&derive_lock(&graph));
        fs::write(&path, &rendered).expect("write fixture");
        return;
    }
    let committed = fs::read(&path).expect("golden fixture is committed");
    validate_committed_lock(&committed, &graph).expect("golden lock validates");
}
