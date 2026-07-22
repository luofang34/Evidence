//! Tests for insertion-order independence (TEST-172) and for
//! ancestry and ordinal forest invariants (TEST-173).

use super::SourceNodeKind;
use super::error::SourceGraphError;
use super::render::render_source_graph_canonical;
use super::tests_support::*;
use crate::corpus::CorpusError;
use crate::corpus::graph::CorpusGraph;

/// Extract the source-graph failure from corpus validation.
fn source_graph_err(graph: &CorpusGraph) -> SourceGraphError {
    match graph.validate() {
        Err(CorpusError::SourceGraph(err)) => err,
        other => panic!("expected a source-graph error, got: {other:?}"),
    }
}

/// Insertion order and file split carry no semantics: the same
/// nodes inserted in different orders produce equal graphs and
/// byte-identical canonical renderings (TEST-172).
#[test]
fn record_order_and_file_split_are_non_semantic() {
    let ordered = base_corpus();
    let mut reversed_nodes = three_node_set();
    reversed_nodes.reverse();
    let reversed = corpus_with(&reversed_nodes);
    reversed.validate().expect("reversed forest is valid");
    assert_eq!(ordered, reversed, "insertion order is non-semantic");
    assert_eq!(
        render_source_graph_canonical(&ordered),
        render_source_graph_canonical(&reversed),
        "canonical bytes are insertion-order independent"
    );
}

/// Dangling, cross-revision, and cyclic parent links each fail
/// with their own typed variant (TEST-173).
#[test]
fn cycles_dangling_and_cross_revision_parents_fail_closed() {
    // Dangling: the parent is absent from every revision.
    let built = vec![make_node(
        &[],
        REV_A,
        NODE_A,
        Some(NODE_E),
        SourceNodeKind::Paragraph,
        0,
        None,
        "x",
    )];
    let err = source_graph_err(&corpus_with(&built));
    assert!(
        matches!(
            err,
            SourceGraphError::DanglingParent {
                ref revision_uid,
                ref node_uid,
                ref parent_uid,
            } if revision_uid == REV_A && node_uid == NODE_A && parent_uid == NODE_E
        ),
        "expected DanglingParent, got: {err:?}"
    );

    // Cross-revision: the parent exists, but only in another
    // revision's graph.
    let parent = make_node(
        &[],
        REV_B,
        NODE_E,
        None,
        SourceNodeKind::Section,
        0,
        None,
        "",
    );
    let child = make_node(
        std::slice::from_ref(&parent),
        REV_A,
        NODE_A,
        Some(NODE_E),
        SourceNodeKind::Paragraph,
        0,
        None,
        "x",
    );
    let mut graph = corpus_with(&[child]);
    graph
        .insert(revision(REV_B))
        .expect("insert second revision");
    graph.insert_source_node(parent).expect("insert parent");
    let err = source_graph_err(&graph);
    assert!(
        matches!(
            err,
            SourceGraphError::CrossRevisionParent {
                ref revision_uid,
                ref node_uid,
                ref parent_uid,
                ref parent_revision_uid,
            } if revision_uid == REV_A
                && node_uid == NODE_A
                && parent_uid == NODE_E
                && parent_revision_uid == REV_B
        ),
        "expected CrossRevisionParent, got: {err:?}"
    );

    // Cycle: two nodes parenting each other.
    let a = make_node(
        &[],
        REV_A,
        NODE_A,
        Some(NODE_B),
        SourceNodeKind::Section,
        0,
        None,
        "",
    );
    let b = make_node(
        std::slice::from_ref(&a),
        REV_A,
        NODE_B,
        Some(NODE_A),
        SourceNodeKind::Paragraph,
        0,
        None,
        "x",
    );
    let err = source_graph_err(&corpus_with(&[a, b]));
    assert!(
        matches!(err, SourceGraphError::Cycle { ref revision_uid, .. } if revision_uid == REV_A),
        "expected Cycle, got: {err:?}"
    );

    // Self-parent is a one-node cycle.
    let selfish = make_node(
        &[],
        REV_A,
        NODE_A,
        Some(NODE_A),
        SourceNodeKind::Section,
        0,
        None,
        "",
    );
    let err = source_graph_err(&corpus_with(&[selfish]));
    assert!(
        matches!(
            err,
            SourceGraphError::Cycle {
                ref revision_uid,
                ref node_uid,
            } if revision_uid == REV_A && node_uid == NODE_A
        ),
        "expected Cycle for a self-parent, got: {err:?}"
    );
}

/// Duplicate and gapped sibling ordinals fail closed; the root
/// set is a sibling set too (TEST-173).
#[test]
fn duplicate_ordinals_and_gaps_fail_closed() {
    // Duplicate ordinal under one parent.
    let mut nodes = three_node_set();
    nodes[2].ordinal = 0;
    let err = source_graph_err(&corpus_with(&nodes));
    assert!(
        matches!(
            err,
            SourceGraphError::DuplicateOrdinal {
                ref revision_uid,
                parent_uid: Some(ref parent_uid),
                ordinal: 0,
                ..
            } if revision_uid == REV_A && parent_uid == NODE_A
        ),
        "expected DuplicateOrdinal, got: {err:?}"
    );

    // A gap: siblings at 0 and 2.
    let mut nodes = three_node_set();
    nodes[2].ordinal = 2;
    let err = source_graph_err(&corpus_with(&nodes));
    assert!(
        matches!(
            err,
            SourceGraphError::NonContiguousOrdinals {
                ref revision_uid,
                parent_uid: Some(ref parent_uid),
                expected: 1,
                found: 2,
                ..
            } if revision_uid == REV_A && parent_uid == NODE_A
        ),
        "expected NonContiguousOrdinals, got: {err:?}"
    );

    // The root set must start at 0.
    let nodes = vec![make_node(
        &[],
        REV_A,
        NODE_A,
        None,
        SourceNodeKind::Section,
        1,
        None,
        "",
    )];
    let err = source_graph_err(&corpus_with(&nodes));
    assert!(
        matches!(
            err,
            SourceGraphError::NonContiguousOrdinals {
                parent_uid: None,
                expected: 0,
                found: 1,
                ..
            }
        ),
        "expected NonContiguousOrdinals for the root set, got: {err:?}"
    );
}
