//! Legacy `cert/trace` four-file documents load into the corpus
//! graph with identity and edge-set parity (TEST-121).

use std::path::PathBuf;

use crate::trace::{HlrEntry, TestEntry, read_all_trace_files};

use super::super::graph::{CorpusGraph, EdgeKind, Node, RequirementLayer, RequirementNode};
use super::super::legacy::graph_from_trace_files;

fn workspace_trace_root() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .join("cert/trace")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn legacy_parity_on_own_trace() {
    let files = read_all_trace_files(&workspace_trace_root()).expect("read own trace");
    let graph = graph_from_trace_files(&files).expect("adapt own trace");

    let derived_len = files.derived.as_ref().map_or(0, |d| d.requirements.len());
    let expected = files.sys.requirements.len()
        + files.hlr.requirements.len()
        + files.llr.requirements.len()
        + derived_len
        + files.tests.tests.len();
    assert_eq!(graph.len(), expected, "no entry may be dropped or added");

    assert_requirement_parity(&graph, &files.sys.requirements, RequirementLayer::Sys);
    assert_requirement_parity(&graph, &files.hlr.requirements, RequirementLayer::Hlr);
    for entry in &files.llr.requirements {
        let node = expect_requirement(&graph, entry.uid.as_deref().unwrap());
        assert_eq!(node.layer, RequirementLayer::Llr);
        assert_eq!(node.id, entry.id);
        assert_edges(&node.edges, EdgeKind::DerivesFrom, &entry.traces_to);
    }
    for entry in &files.tests.tests {
        assert_test_parity(&graph, entry);
    }

    // The tool's own trace is link-valid, so the graph view must be too.
    graph.validate().expect("own trace has no dangling edges");
}

fn assert_requirement_parity(graph: &CorpusGraph, entries: &[HlrEntry], layer: RequirementLayer) {
    for entry in entries {
        let node = expect_requirement(graph, entry.uid.as_deref().unwrap());
        assert_eq!(node.layer, layer, "{} layer mismatch", entry.id);
        assert_eq!(node.id, entry.id);
        assert_eq!(node.title, entry.title);
        assert_edges(&node.edges, EdgeKind::DerivesFrom, &entry.traces_to);
    }
}

fn assert_test_parity(graph: &CorpusGraph, entry: &TestEntry) {
    let uid = entry.uid.as_deref().unwrap();
    let Some(Node::Test(node)) = graph.get(uid) else {
        panic!("test node {uid} missing or wrong kind");
    };
    assert_eq!(node.id, entry.id);
    assert_eq!(node.selectors, entry.all_selectors());
    assert_edges(&node.edges, EdgeKind::Verifies, &entry.traces_to);
}

fn expect_requirement<'g>(graph: &'g CorpusGraph, uid: &str) -> &'g RequirementNode {
    match graph.get(uid) {
        Some(Node::Requirement(node)) => node,
        other => panic!("requirement node {uid} missing or wrong kind: {other:?}"),
    }
}

fn assert_edges(edges: &[(EdgeKind, String)], kind: EdgeKind, expected_targets: &[String]) {
    let targets: Vec<&str> = edges
        .iter()
        .map(|(k, t)| {
            assert_eq!(*k, kind);
            t.as_str()
        })
        .collect();
    let mut expected: Vec<&str> = expected_targets.iter().map(String::as_str).collect();
    expected.sort_unstable();
    assert_eq!(targets, expected);
}
