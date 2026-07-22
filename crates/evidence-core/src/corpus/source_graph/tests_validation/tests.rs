//! Tests for the kind-legality, digest, human-identity, and
//! revision-binding invariants (TEST-173).

use super::SourceNodeKind;
use super::error::SourceGraphError;
use super::normalization::{content_digest, fingerprint};
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

/// The closed parent/child kind table rejects illegal pairs and
/// accepts the legal chain (TEST-173).
#[test]
fn illegal_parent_child_kinds_fail_closed() {
    let cases: [(SourceNodeKind, SourceNodeKind); 4] = [
        (SourceNodeKind::Table, SourceNodeKind::Paragraph),
        (SourceNodeKind::TableRow, SourceNodeKind::Table),
        (SourceNodeKind::Paragraph, SourceNodeKind::Paragraph),
        (SourceNodeKind::CodeBlock, SourceNodeKind::Note),
    ];
    for (parent_kind, child_kind) in cases {
        let parent = make_node(&[], REV_A, NODE_A, None, parent_kind, 0, None, "");
        let child = make_node(
            std::slice::from_ref(&parent),
            REV_A,
            NODE_B,
            Some(NODE_A),
            child_kind,
            0,
            None,
            "x",
        );
        let err = source_graph_err(&corpus_with(&[parent, child]));
        assert!(
            matches!(
                err,
                SourceGraphError::IllegalParentKind {
                    ref revision_uid,
                    ref node_uid,
                    kind,
                    ref parent_uid,
                    parent_kind: actual_parent_kind,
                } if revision_uid == REV_A
                    && node_uid == NODE_B
                    && kind == child_kind
                    && parent_uid == NODE_A
                    && actual_parent_kind == parent_kind
            ),
            "{parent_kind:?} parenting {child_kind:?}: expected IllegalParentKind, got: {err:?}"
        );
    }

    // The legal chain validates: Section → Table → TableRow →
    // TableCell, Section → Paragraph, Section → Section.
    let a = make_node(
        &[],
        REV_A,
        NODE_A,
        None,
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
        SourceNodeKind::Table,
        0,
        None,
        "",
    );
    let c = make_node(
        &[a.clone(), b.clone()],
        REV_A,
        NODE_C,
        Some(NODE_B),
        SourceNodeKind::TableRow,
        0,
        None,
        "",
    );
    let d = make_node(
        &[a.clone(), b.clone(), c.clone()],
        REV_A,
        NODE_D,
        Some(NODE_C),
        SourceNodeKind::TableCell,
        0,
        None,
        "cell",
    );
    let e = make_node(
        std::slice::from_ref(&a),
        REV_A,
        NODE_E,
        Some(NODE_A),
        SourceNodeKind::Paragraph,
        1,
        None,
        "prose",
    );
    corpus_with(&[a, b, c, d, e])
        .validate()
        .expect("the legal chain validates");
}

/// A stored content digest or fingerprint that disagrees with the
/// recomputed value fails closed, naming the field and both
/// values (TEST-173).
#[test]
fn digest_and_fingerprint_mismatches_fail_closed() {
    // Tampered content digest.
    let mut nodes = three_node_set();
    nodes[1].content_sha256 = content_digest(SourceNodeKind::Paragraph, "different text");
    let err = source_graph_err(&corpus_with(&nodes));
    match err {
        SourceGraphError::DigestMismatch {
            revision_uid,
            node_uid,
            field,
            expected,
            actual,
        } => {
            assert_eq!(revision_uid, REV_A);
            assert_eq!(node_uid, NODE_B);
            assert_eq!(field, "content_sha256");
            assert_eq!(
                expected,
                content_digest(SourceNodeKind::Paragraph, "First prose.").as_str()
            );
            assert_eq!(
                actual,
                content_digest(SourceNodeKind::Paragraph, "different text").as_str()
            );
        }
        other => panic!("expected DigestMismatch, got: {other:?}"),
    }

    // Tampered fingerprint.
    let mut nodes = three_node_set();
    nodes[1].fingerprint = fingerprint(
        SourceNodeKind::Paragraph,
        Some("wrong label"),
        &[(SourceNodeKind::Section, Some("1 Introduction"))],
    );
    let err = source_graph_err(&corpus_with(&nodes));
    assert!(
        matches!(
            err,
            SourceGraphError::DigestMismatch {
                ref revision_uid,
                ref node_uid,
                field: "fingerprint",
                ..
            } if revision_uid == REV_A && node_uid == NODE_B
        ),
        "expected DigestMismatch for the fingerprint, got: {err:?}"
    );
}

/// Duplicate uids and duplicate labels within one kind fail at
/// insertion; the same label under different kinds or different
/// revisions is valid (TEST-173).
#[test]
fn duplicate_uids_and_human_identities_fail_closed() {
    let mut graph = CorpusGraph::new();
    graph.insert(revision(REV_A)).expect("insert revision");
    let section = make_node(
        &[],
        REV_A,
        NODE_A,
        None,
        SourceNodeKind::Section,
        0,
        Some("Introduction"),
        "",
    );
    graph
        .insert_source_node(section.clone())
        .expect("first insert");

    let err = graph
        .insert_source_node(section.clone())
        .expect_err("duplicate uid must fail");
    assert!(
        matches!(err, SourceGraphError::DuplicateUid { ref uid, .. } if uid == NODE_A),
        "expected DuplicateUid, got: {err:?}"
    );

    let twin = make_node(
        &[],
        REV_A,
        NODE_B,
        None,
        SourceNodeKind::Section,
        1,
        Some("Introduction"),
        "",
    );
    let err = graph
        .insert_source_node(twin)
        .expect_err("duplicate label within one kind must fail");
    assert!(
        matches!(
            err,
            SourceGraphError::DuplicateHumanId {
                ref revision_uid,
                kind: SourceNodeKind::Section,
                ref label,
                ref first_uid,
                ref duplicate_uid,
            } if revision_uid == REV_A
                && label == "Introduction"
                && first_uid == NODE_A
                && duplicate_uid == NODE_B
        ),
        "expected DuplicateHumanId, got: {err:?}"
    );

    // The same label under a different kind is a different human
    // identity.
    let paragraph = make_node(
        &[],
        REV_A,
        NODE_B,
        Some(NODE_A),
        SourceNodeKind::Paragraph,
        0,
        Some("Introduction"),
        "text",
    );
    graph
        .insert_source_node(paragraph)
        .expect("labels are unique within a kind, not across kinds");

    // The same label under the same kind in another revision is
    // valid — identity is per revision.
    graph
        .insert(revision(REV_B))
        .expect("insert second revision");
    let other_revision = make_node(
        &[],
        REV_B,
        NODE_C,
        None,
        SourceNodeKind::Section,
        0,
        Some("Introduction"),
        "",
    );
    graph
        .insert_source_node(other_revision)
        .expect("human identity is per revision");
}

/// A source graph whose revision uid names no committed source
/// revision fails closed (TEST-173).
#[test]
fn unbound_source_revision_fails_closed() {
    let nodes = vec![make_node(
        &[],
        REV_A,
        NODE_A,
        None,
        SourceNodeKind::Section,
        0,
        None,
        "",
    )];
    let mut graph = CorpusGraph::new();
    for node in nodes {
        graph.insert_source_node(node).expect("insert node");
    }
    let err = source_graph_err(&graph);
    assert!(
        matches!(
            err,
            SourceGraphError::UnknownSourceRevision { ref revision_uid } if revision_uid == REV_A
        ),
        "expected UnknownSourceRevision, got: {err:?}"
    );
}
