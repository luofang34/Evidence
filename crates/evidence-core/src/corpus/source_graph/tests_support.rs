//! Shared fixtures for the structural source-graph test modules
//! (TEST-172, TEST-173). No `#[test]` functions live here.

use super::normalization::{content_digest, fingerprint};
use super::{SourceNode, SourceNodeKind};
use crate::corpus::graph::CorpusGraph;
use crate::corpus::{Node, SourceLocator, SourceMaterial, SourceRevisionNode};

pub(super) const REV_A: &str = "src_00000000-0000-4000-8000-0000000000a1";
pub(super) const REV_B: &str = "src_00000000-0000-4000-8000-0000000000a2";
pub(super) const NODE_A: &str = "snode_00000000-0000-4000-8000-0000000000b1";
pub(super) const NODE_B: &str = "snode_00000000-0000-4000-8000-0000000000b2";
pub(super) const NODE_C: &str = "snode_00000000-0000-4000-8000-0000000000b3";
pub(super) const NODE_D: &str = "snode_00000000-0000-4000-8000-0000000000b4";
pub(super) const NODE_E: &str = "snode_00000000-0000-4000-8000-0000000000b5";

/// One committed source revision with per-uid unique human id and
/// document key, so multi-revision fixtures pass lineage rules.
pub(super) fn revision(uid: &str) -> Node {
    Node::SourceRevision(SourceRevisionNode {
        uid: uid.to_string(),
        id: format!("REV-{uid}"),
        document_key: format!("DOC-{uid}"),
        title: "spec".to_string(),
        media_type: "text/markdown".to_string(),
        canonical_location: "https://example.org/spec".to_string(),
        material: SourceMaterial::Unavailable {
            reason: "test fixture".to_string(),
        },
        edges: Vec::new(),
    })
}

pub(super) fn md_locator() -> SourceLocator {
    SourceLocator::Markdown {
        path: crate::corpus::SafeRelPath::new("docs/spec.md").expect("safe path"),
        git_blob: None,
        anchor: None,
        heading_path: Vec::new(),
        byte_range: (0, 10),
    }
}

/// Build one node with digests computed against the already-built
/// ancestors in `built` (parents must come first there). A parent
/// absent from `built` yields an empty ancestry, matching the
/// recomputation order validation applies: ancestry errors fire
/// before digest checks.
#[allow(
    clippy::too_many_arguments,
    reason = "the builder mirrors the node schema field for field"
)]
pub(super) fn make_node(
    built: &[SourceNode],
    revision_uid: &str,
    uid: &str,
    parent: Option<&str>,
    kind: SourceNodeKind,
    ordinal: u32,
    label: Option<&str>,
    text: &str,
) -> SourceNode {
    let mut ancestry = Vec::new();
    let mut current = parent;
    while let Some(uid) = current {
        let Some(node) = built.iter().find(|node| node.uid == uid) else {
            break;
        };
        ancestry.push((node.kind, node.label.clone()));
        current = node.parent_uid.as_deref();
    }
    ancestry.reverse();
    let ancestry_refs: Vec<(SourceNodeKind, Option<&str>)> = ancestry
        .iter()
        .map(|(kind, label)| (*kind, label.as_deref()))
        .collect();
    SourceNode {
        uid: uid.to_string(),
        source_revision_uid: revision_uid.to_string(),
        parent_uid: parent.map(str::to_string),
        kind,
        ordinal,
        label: label.map(str::to_string),
        canonical_text: text.to_string(),
        content_sha256: content_digest(kind, text),
        fingerprint: fingerprint(kind, label, &ancestry_refs),
        locator: md_locator(),
    }
}

/// The base valid forest: a section root with a paragraph and a
/// code block child.
pub(super) fn three_node_set() -> Vec<SourceNode> {
    let mut built = Vec::new();
    let section = make_node(
        &built,
        REV_A,
        NODE_A,
        None,
        SourceNodeKind::Section,
        0,
        Some("1 Introduction"),
        "",
    );
    built.push(section);
    let paragraph = make_node(
        &built,
        REV_A,
        NODE_B,
        Some(NODE_A),
        SourceNodeKind::Paragraph,
        0,
        None,
        "First prose.",
    );
    built.push(paragraph);
    let code = make_node(
        &built,
        REV_A,
        NODE_C,
        Some(NODE_A),
        SourceNodeKind::CodeBlock,
        1,
        None,
        "fn main() {}\n",
    );
    built.push(code);
    built
}

/// A corpus carrying `REV_A` and the given nodes of its source
/// graph.
pub(super) fn corpus_with(nodes: &[SourceNode]) -> CorpusGraph {
    let mut graph = CorpusGraph::new();
    graph.insert(revision(REV_A)).expect("insert revision");
    for node in nodes {
        graph.insert_source_node(node.clone()).expect("insert node");
    }
    graph
}

/// The base forest as a validated corpus.
pub(super) fn base_corpus() -> CorpusGraph {
    let graph = corpus_with(&three_node_set());
    graph.validate().expect("base forest is valid");
    graph
}
