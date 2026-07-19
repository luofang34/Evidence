//! Unit tests for the corpus index, graph invariants, layout
//! agnosticism, and legacy-trace parity (TEST-118..121).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::path::{Path, PathBuf};

use crate::trace::{HlrEntry, TestEntry, read_all_trace_files};

use super::CorpusError;
use super::graph::{CorpusGraph, EdgeKind, Node, RequirementLayer, RequirementNode};
use super::index::CorpusIndex;
use super::legacy::graph_from_trace_files;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn requirement(uid: &str, id: &str, edges: &[&str]) -> Node {
    Node::Requirement(RequirementNode {
        uid: uid.to_string(),
        id: id.to_string(),
        title: format!("title of {id}"),
        layer: RequirementLayer::Sys,
        edges: edges
            .iter()
            .map(|t| (EdgeKind::DerivesFrom, (*t).to_string()))
            .collect(),
    })
}

// ---------------------------------------------------------------- index

#[test]
fn index_parses_minimal() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("reqs/base.toml"),
        r#"
schema_version = 1

[[requirements]]
uid = "req_00000000-0000-0000-0000-00000000000a"
id = "R-A"
layer = "sys"
title = "a"
"#,
    );
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"reqs/**/*.toml\"]\n",
    );

    let index = CorpusIndex::load(&dir.path().join("corpus.toml")).unwrap();
    assert_eq!(index.requirement_files.len(), 1);

    let graph = CorpusIndex::load_graph(&dir.path().join("corpus.toml")).unwrap();
    assert_eq!(graph.len(), 1);
    assert!(
        graph
            .get("req_00000000-0000-0000-0000-00000000000a")
            .is_some()
    );
}

#[test]
fn index_rejects_unknown_field() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nfrobnicate = true\n",
    );
    let err = CorpusIndex::load(&dir.path().join("corpus.toml")).unwrap_err();
    assert!(
        matches!(err, CorpusError::IndexParse { .. }),
        "unknown index field must be a parse error, got: {err:?}"
    );
}

#[test]
fn index_refuses_newer_schema() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("corpus.toml"), "schema_version = 999\n");
    let err = CorpusIndex::load(&dir.path().join("corpus.toml")).unwrap_err();
    assert!(
        matches!(err, CorpusError::IndexSchemaTooNew { found: 999, .. }),
        "newer schema must refuse to load, got: {err:?}"
    );
}

#[test]
fn index_empty_resolution_is_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("empty")).unwrap();
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"empty/**/*.toml\"]\n",
    );
    let err = CorpusIndex::load(&dir.path().join("corpus.toml")).unwrap_err();
    assert!(
        matches!(err, CorpusError::EmptyIndexEntry { .. }),
        "an entry resolving to nothing must fail closed, got: {err:?}"
    );
}

#[test]
fn index_unimplemented_kind_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources/x.toml\"]\n",
    );
    let err = CorpusIndex::load(&dir.path().join("corpus.toml")).unwrap_err();
    assert!(
        matches!(err, CorpusError::UnimplementedKind { kind: "sources" }),
        "an indexed-but-unloadable kind must refuse, got: {err:?}"
    );
}

#[test]
fn records_reject_unprefixed_uid() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("reqs/bad.toml"),
        r#"
schema_version = 1

[[requirements]]
uid = "00000000-0000-0000-0000-00000000000a"
id = "R-A"
layer = "sys"
title = "a"
"#,
    );
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"reqs/**/*.toml\"]\n",
    );
    let err = CorpusIndex::load_graph(&dir.path().join("corpus.toml")).unwrap_err();
    assert!(
        matches!(err, CorpusError::NativeUidPrefix { .. }),
        "native record uids must carry the req_ prefix, got: {err:?}"
    );
}

// ---------------------------------------------------------------- graph

#[test]
fn graph_rejects_duplicate_uid() {
    let mut graph = CorpusGraph::new();
    graph.insert(requirement("req_dup", "R-1", &[])).unwrap();
    let err = graph
        .insert(requirement("req_dup", "R-2", &[]))
        .unwrap_err();
    assert!(
        matches!(err, CorpusError::DuplicateUid { ref uid } if uid == "req_dup"),
        "duplicate uid must be rejected naming the uid, got: {err:?}"
    );
}

#[test]
fn graph_detects_dangling_edge() {
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
}

// --------------------------------------------------------------- layout

#[test]
fn layout_split_produces_identical_graph() {
    const A: &str = r#"
[[requirements]]
uid = "req_00000000-0000-0000-0000-00000000000a"
id = "R-A"
layer = "sys"
title = "root"
"#;
    const B: &str = r#"
[[requirements]]
uid = "req_00000000-0000-0000-0000-00000000000b"
id = "R-B"
layer = "hlr"
title = "middle"
derives_from = ["req_00000000-0000-0000-0000-00000000000a"]
"#;
    const C: &str = r#"
[[requirements]]
uid = "req_00000000-0000-0000-0000-00000000000c"
id = "R-C"
layer = "llr"
title = "leaf"
derives_from = ["req_00000000-0000-0000-0000-00000000000b"]
"#;

    // Layout 1: everything in one indexed file.
    let one = tempfile::tempdir().unwrap();
    write(
        &one.path().join("all/entries.toml"),
        &format!("schema_version = 1\n{A}{B}{C}"),
    );
    write(
        &one.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"all/**/*.toml\"]\n",
    );

    // Layout 2: three files across two directories, mixing a literal
    // path with a pattern, under unrelated filenames.
    let split = tempfile::tempdir().unwrap();
    write(
        &split.path().join("x/one.toml"),
        &format!("schema_version = 1\n{A}"),
    );
    write(
        &split.path().join("x/two.toml"),
        &format!("schema_version = 1\n{B}"),
    );
    write(
        &split.path().join("y/rest.toml"),
        &format!("schema_version = 1\n{C}"),
    );
    write(
        &split.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"x/**/*.toml\", \"y/rest.toml\"]\n",
    );

    let g1 = CorpusIndex::load_graph(&one.path().join("corpus.toml")).unwrap();
    let g2 = CorpusIndex::load_graph(&split.path().join("corpus.toml")).unwrap();
    assert_eq!(g1, g2, "file layout must not affect the loaded graph");
    assert_eq!(g1.len(), 3);
}

// --------------------------------------------------------------- legacy

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
    let expected: Vec<&str> = expected_targets.iter().map(String::as_str).collect();
    assert_eq!(targets, expected);
}
