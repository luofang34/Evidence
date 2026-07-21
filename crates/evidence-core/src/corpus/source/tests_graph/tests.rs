//! Graph-identity tests for source-revision nodes: per-kind human
//! id uniqueness, global uid uniqueness, canonical iteration,
//! layout independence, and the typed material-state distinction
//! (TEST-142).

use super::tests_support::*;
use crate::corpus::{CorpusError, CorpusGraph, CorpusIndex, EdgeKind, Node, NodeKind, SourceError};

/// Source-revision human ids are unique within the source kind
/// only: a repeat fails naming both uids, while the same id on a
/// requirement coexists; uids stay globally unique (TEST-142).
#[test]
fn human_ids_are_unique_within_the_source_kind_only() {
    let err = expect_load_err(
        load_source_content(&source_file(&[
            vendored(SRC_1, "SRC-DUP"),
            vendored(SRC_2, "SRC-DUP"),
        ])),
        "duplicate source id",
    );
    assert!(
        matches!(
            err,
            SourceError::DuplicateHumanId {
                ref id,
                kind: NodeKind::SourceRevision,
                ref first_uid,
                ref duplicate_uid,
            } if id == "SRC-DUP" && first_uid == SRC_1 && duplicate_uid == SRC_2
        ),
        "duplicate id must name kind and both uids, got: {err:?}"
    );

    let err = expect_load_err(
        load_source_content(&source_file(&[
            vendored(SRC_1, "SRC-1"),
            vendored(SRC_1, "SRC-2"),
        ])),
        "duplicate source uid",
    );
    assert!(
        matches!(err, SourceError::DuplicateUid { ref uid } if uid == SRC_1),
        "duplicate uid must fail, got: {err:?}"
    );

    // The same human id on a requirement and a source revision
    // coexists: identity namespaces are per-kind.
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("reqs/records.toml"),
        &format!(
            "schema_version = 1\n\n[[requirements]]\nuid = \"{REQ_A}\"\nid = \"SHARED-1\"\nlayer = \"hlr\"\ntitle = \"shared id requirement\"\n"
        ),
    );
    let mut shared = vendored(SRC_1, "SHARED-1");
    shared.title = "shared id source".to_string();
    write(
        &dir.path().join("sources/records.toml"),
        &source_file(&[shared]),
    );
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources/**/*.toml\"]\nrequirements = [\"reqs/**/*.toml\"]\n",
    );
    let graph = CorpusIndex::load_graph(&dir.path().join("corpus.toml")).unwrap();
    assert_eq!(graph.len(), 2, "per-kind namespaces coexist");

    // Uids stay globally unique across kinds.
    let mut graph = CorpusGraph::new();
    graph
        .insert(Node::Requirement(crate::corpus::RequirementNode::new(
            SRC_1.to_string(),
            "R-1".to_string(),
            "title".to_string(),
            crate::corpus::RequirementLayer::Sys,
            Vec::new(),
        )))
        .unwrap();
    let loaded = load_source_content(&source_file(&[vendored(SRC_1, "SRC-1")])).unwrap();
    let source_node = expect_source(&loaded, SRC_1);
    let err = graph
        .insert(Node::SourceRevision(source_node.clone()))
        .unwrap_err();
    assert!(
        matches!(err, CorpusError::DuplicateUid { ref uid } if uid == SRC_1),
        "a cross-kind uid collision must fail, got: {err:?}"
    );
}

/// Node iteration is canonical by uid regardless of record order
/// (TEST-142).
#[test]
fn source_revision_nodes_iterate_in_canonical_uid_order() {
    let graph = load_source_content(&source_file(&[
        vendored(SRC_4, "SRC-4"),
        vendored(SRC_2, "SRC-2"),
        vendored(SRC_3, "SRC-3"),
        vendored(SRC_1, "SRC-1"),
    ]))
    .expect("records load in any order");
    let uids: Vec<&str> = graph.nodes().map(Node::uid).collect();
    assert_eq!(uids, [SRC_1, SRC_2, SRC_3, SRC_4], "uid order is canonical");
}

/// The same records load into equal graphs from one file or many,
/// in any record order (TEST-142).
#[test]
fn layout_split_and_record_order_produce_identical_graphs() {
    let mut spec_a = vendored(SRC_1, "SRC-1");
    spec_a.document_key = "DOC-A".to_string();
    let mut spec_b = vendored(SRC_2, "SRC-2");
    spec_b.document_key = "DOC-B".to_string();
    let mut spec_c = vendored(SRC_3, "SRC-3");
    spec_c.document_key = "DOC-C".to_string();
    let record_a = record_toml(&spec_a);
    let record_b = record_toml(&spec_b);
    let record_c = record_toml(&spec_c);

    let one = tempfile::tempdir().unwrap();
    write(
        &one.path().join("all/entries.toml"),
        &format!("schema_version = 1\n{record_a}{record_b}{record_c}"),
    );
    write(
        &one.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"all/**/*.toml\"]\n",
    );
    let g1 = CorpusIndex::load_graph(&one.path().join("corpus.toml")).unwrap();

    let split = tempfile::tempdir().unwrap();
    write(
        &split.path().join("x/one.toml"),
        &format!("schema_version = 1\n{record_c}"),
    );
    write(
        &split.path().join("x/two.toml"),
        &format!("schema_version = 1\n{record_a}"),
    );
    write(
        &split.path().join("y/rest.toml"),
        &format!("schema_version = 1\n{record_b}"),
    );
    write(
        &split.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"x/**/*.toml\", \"y/rest.toml\"]\n",
    );
    let g2 = CorpusIndex::load_graph(&split.path().join("corpus.toml")).unwrap();

    assert_eq!(
        g1, g2,
        "file split and record order must not affect the loaded graph"
    );
    assert_eq!(g1.len(), 3);
}

/// An unavailable revision is valid graph state, distinguishable
/// from available material by its typed state, and carries no
/// digest to report as byte-verified; a source revision carrying a
/// non-supersedes edge fails graph validation (TEST-142).
#[test]
fn unavailable_material_is_valid_distinguishable_graph_state() {
    let mut unavailable = vendored(SRC_1, "SRC-1");
    unavailable.material_toml =
        "state = \"unavailable\"\nreason = \"restricted distribution\"".to_string();
    let mut available_spec = vendored(SRC_2, "SRC-2");
    available_spec.document_key = "DOC-2".to_string();
    let graph = load_source_content(&source_file(&[unavailable, available_spec]))
        .expect("unavailable material is valid graph state");
    graph.validate().expect("the graph validates");

    let node = expect_source(&graph, SRC_1);
    let crate::corpus::SourceMaterial::Unavailable { reason } = &node.material else {
        panic!("expected unavailable material, got: {:?}", node.material);
    };
    assert_eq!(reason, "restricted distribution");

    let available = expect_source(&graph, SRC_2);
    assert!(
        matches!(
            available.material,
            crate::corpus::SourceMaterial::Available { .. }
        ),
        "the available sibling stays distinguishable"
    );

    // A source revision carrying a non-supersedes edge fails
    // endpoint validation: only `Supersedes` accepts a
    // source-revision endpoint (LLR-129).
    let mut graph = CorpusGraph::new();
    graph
        .insert(Node::Requirement(crate::corpus::RequirementNode::new(
            REQ_A.to_string(),
            "R-A".to_string(),
            "title".to_string(),
            crate::corpus::RequirementLayer::Sys,
            Vec::new(),
        )))
        .unwrap();
    let loaded = load_source_content(&source_file(&[vendored(SRC_1, "SRC-1")])).unwrap();
    let mut edged = expect_source(&loaded, SRC_1).clone();
    edged.edges = vec![(EdgeKind::DerivesFrom, REQ_A.to_string())];
    graph.insert(Node::SourceRevision(edged)).unwrap();
    let err = graph.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::InvalidEdgeKinds {
                source_kind: NodeKind::SourceRevision,
                ..
            }
        ),
        "a source revision with an edge must fail validation, got: {err:?}"
    );
}
