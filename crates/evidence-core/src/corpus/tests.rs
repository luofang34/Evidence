//! Unit tests for the corpus index, graph invariants, layout
//! agnosticism, and legacy-trace parity (TEST-119..122).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::path::{Path, PathBuf};

use crate::trace::{HlrEntry, TestEntry, read_all_trace_files};

use super::CorpusError;
use super::graph::{
    CorpusGraph, EdgeKind, Node, NodeKind, RequirementLayer, RequirementNode, TestNode,
};
use super::index::CorpusIndex;
use super::legacy::graph_from_trace_files;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

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

#[test]
fn index_parses_minimal() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("reqs/base.toml"),
        r#"
schema_version = 1

[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000a"
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
            .get("req_00000000-0000-4000-8000-00000000000a")
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
fn index_unsupported_kind_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources/x.toml\"]\n",
    );
    let err = CorpusIndex::load(&dir.path().join("corpus.toml")).unwrap_err();
    assert!(
        matches!(err, CorpusError::UnsupportedKind { kind: "sources" }),
        "an indexed-but-unloadable kind must refuse, got: {err:?}"
    );
}

#[test]
fn records_reject_invalid_native_uid() {
    let err = native_requirement_error("00000000-0000-4000-8000-00000000000a");
    assert!(
        matches!(err, CorpusError::NativeUidPrefix { .. }),
        "native record uids must carry the req_ prefix, got: {err:?}"
    );

    let err = native_requirement_error("req_00000000-0000-1000-8000-00000000000a");
    assert!(
        matches!(err, CorpusError::NativeUidUuidV4 { .. }),
        "native record uid suffixes must be UUIDv4, got: {err:?}"
    );
}

fn native_requirement_error(uid: &str) -> CorpusError {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("reqs/bad.toml"),
        &format!(
            r#"
schema_version = 1

[[requirements]]
uid = "{uid}"
id = "R-A"
layer = "sys"
title = "a"
"#
        ),
    );
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"reqs/**/*.toml\"]\n",
    );
    CorpusIndex::load_graph(&dir.path().join("corpus.toml")).unwrap_err()
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
    }
}

const RECORD_A: &str = r#"
[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000a"
id = "R-A"
layer = "sys"
title = "root"
"#;
const RECORD_B: &str = r#"
[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000b"
id = "R-B"
layer = "hlr"
title = "first parent"
derives_from = ["req_00000000-0000-4000-8000-00000000000a"]
"#;
const RECORD_D: &str = r#"
[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000d"
id = "R-D"
layer = "hlr"
title = "second parent"
derives_from = ["req_00000000-0000-4000-8000-00000000000a"]
"#;
const RECORD_C_FORWARD: &str = r#"
[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000c"
id = "R-C"
layer = "llr"
title = "leaf"
derives_from = [
    "req_00000000-0000-4000-8000-00000000000b",
    "req_00000000-0000-4000-8000-00000000000d",
]
"#;
const RECORD_C_REVERSED: &str = r#"
[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000c"
id = "R-C"
layer = "llr"
title = "leaf"
derives_from = [
    "req_00000000-0000-4000-8000-00000000000d",
    "req_00000000-0000-4000-8000-00000000000b",
]
"#;

#[test]
fn layout_and_edge_order_produce_identical_graph() {
    let g1 = load_single_file_layout();
    let g2 = load_split_layout();
    assert_eq!(
        g1, g2,
        "file layout and input edge order must not affect the loaded graph"
    );
    assert_eq!(g1.len(), 4);
    let leaf = expect_requirement(&g1, "req_00000000-0000-4000-8000-00000000000c");
    assert_edges(
        &leaf.edges,
        EdgeKind::DerivesFrom,
        &[
            "req_00000000-0000-4000-8000-00000000000b".to_string(),
            "req_00000000-0000-4000-8000-00000000000d".to_string(),
        ],
    );
}

fn load_single_file_layout() -> CorpusGraph {
    let one = tempfile::tempdir().unwrap();
    write(
        &one.path().join("all/entries.toml"),
        &format!("schema_version = 1\n{RECORD_A}{RECORD_B}{RECORD_D}{RECORD_C_FORWARD}"),
    );
    write(
        &one.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"all/**/*.toml\"]\n",
    );
    CorpusIndex::load_graph(&one.path().join("corpus.toml")).unwrap()
}

fn load_split_layout() -> CorpusGraph {
    let split = tempfile::tempdir().unwrap();
    write(
        &split.path().join("x/one.toml"),
        &format!("schema_version = 1\n{RECORD_A}"),
    );
    write(
        &split.path().join("x/two.toml"),
        &format!("schema_version = 1\n{RECORD_B}"),
    );
    write(
        &split.path().join("y/rest.toml"),
        &format!("schema_version = 1\n{RECORD_D}{RECORD_C_REVERSED}"),
    );
    write(
        &split.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"x/**/*.toml\", \"y/rest.toml\"]\n",
    );

    CorpusIndex::load_graph(&split.path().join("corpus.toml")).unwrap()
}

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
