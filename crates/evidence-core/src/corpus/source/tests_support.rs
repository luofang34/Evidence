//! Shared fixtures for the source-revision test modules (TEST-141,
//! TEST-142). No `#[test]` functions live here.

use std::path::Path;

use super::records::load_sources_into;
use crate::corpus::{CorpusGraph, Node, NodeKind, SourceError, SourceRevisionNode};

pub(super) const SRC_1: &str = "src_00000000-0000-4000-8000-0000000000a1";
pub(super) const SRC_2: &str = "src_00000000-0000-4000-8000-0000000000a2";
pub(super) const SRC_3: &str = "src_00000000-0000-4000-8000-0000000000a3";
pub(super) const SRC_4: &str = "src_00000000-0000-4000-8000-0000000000a4";
pub(super) const REQ_A: &str = "req_00000000-0000-4000-8000-00000000000a";
pub(super) const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// One source record's field values, rendered by [`record_toml`].
/// Only representable shapes are buildable here; invalid material
/// combinations are written as raw TOML in the tests that need
/// them.
#[derive(Clone)]
pub(super) struct RecordSpec {
    pub(super) uid: String,
    pub(super) id: String,
    pub(super) document_key: String,
    pub(super) title: String,
    pub(super) media_type: String,
    pub(super) canonical_location: String,
    pub(super) material_toml: String,
}

/// A valid vendored record; mutate fields or `material_toml` for
/// other shapes.
pub(super) fn vendored(uid: &str, id: &str) -> RecordSpec {
    RecordSpec {
        uid: uid.to_string(),
        id: id.to_string(),
        document_key: "DOC-1".to_string(),
        title: "spec rev C".to_string(),
        media_type: "application/pdf".to_string(),
        canonical_location: "https://example.org/specs/DOC-1/rev-c".to_string(),
        material_toml: format!(
            "state = \"available\"\nretrieved_at = \"2026-07-01T10:00:00Z\"\nsha256 = \"{DIGEST}\"\n\n[sources.material.capture]\nmode = \"vendored\"\npath = \"sources/doc-1/rev-c.pdf\"\n"
        ),
    }
}

pub(super) fn record_toml(spec: &RecordSpec) -> String {
    format!(
        "\n[[sources]]\nuid = \"{}\"\nid = \"{}\"\ndocument_key = \"{}\"\ntitle = \"{}\"\nmedia_type = \"{}\"\ncanonical_location = \"{}\"\n\n[sources.material]\n{}",
        spec.uid,
        spec.id,
        spec.document_key,
        spec.title,
        spec.media_type,
        spec.canonical_location,
        spec.material_toml,
    )
}

pub(super) fn source_file(specs: &[RecordSpec]) -> String {
    let mut out = "schema_version = 1\n".to_string();
    for spec in specs {
        out.push_str(&record_toml(spec));
    }
    out
}

pub(super) fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

/// Load one source file's content into a fresh graph.
pub(super) fn load_source_content(content: &str) -> Result<CorpusGraph, SourceError> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sources.toml");
    write(&path, content);
    let mut graph = CorpusGraph::new();
    load_sources_into(&path, &mut graph)?;
    Ok(graph)
}

pub(super) fn expect_load_err(result: Result<CorpusGraph, SourceError>, case: &str) -> SourceError {
    result.expect_err(&format!("{case} must fail closed"))
}

/// Unwrap the source-revision node `uid` must name, exercising the
/// shared `Node` accessors for the new kind.
pub(super) fn expect_source<'g>(graph: &'g CorpusGraph, uid: &str) -> &'g SourceRevisionNode {
    let node = graph.get(uid).expect("node must be present");
    assert_eq!(node.kind(), NodeKind::SourceRevision);
    assert_eq!(node.uid(), uid);
    match node {
        Node::SourceRevision(node) => node,
        other => unreachable!("kind was asserted above: {other:?}"),
    }
}
