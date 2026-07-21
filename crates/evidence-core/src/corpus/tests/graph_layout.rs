//! File layout and edge input order are non-semantic: the same
//! records load into an identical graph from one file or many, with
//! edges listed in any order (TEST-122).

use std::path::Path;

use super::super::graph::{CorpusGraph, EdgeKind, Node, RequirementNode};
use super::super::index::CorpusIndex;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
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
