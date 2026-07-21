//! Tests for single-chain lineage validation (TEST-146) and the
//! effective source heads derived view (TEST-147).

use super::fixtures::*;
use super::{effective_source_heads, reject_multiple_heads};
use crate::corpus::{CorpusError, CorpusIndex, EdgeKind, SourceError};

/// One-, two-, and three-revision linear histories all validate
/// (TEST-146).
#[test]
fn one_two_and_three_revision_chains_validate() {
    let one = graph_of(vec![revision(SRC_A, DOC_1, None)]);
    assert!(one.validate().is_ok(), "one revision validates");

    let two = graph_of(vec![
        revision(SRC_A, DOC_1, None),
        revision(SRC_B, DOC_1, Some(SRC_A)),
    ]);
    assert!(two.validate().is_ok(), "a two-revision chain validates");

    let three = graph_of(three_chain(DOC_1));
    assert!(three.validate().is_ok(), "a three-revision chain validates");
}

/// A revision that supersedes itself fails with a distinct typed
/// error (TEST-146).
#[test]
fn self_link_fails_closed() {
    let graph = graph_of(vec![revision(SRC_A, DOC_1, Some(SRC_A))]);
    let err = graph.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::Source(SourceError::SourceSupersessionSelf { ref uid }) if uid == SRC_A
        ),
        "a self-link must fail with SourceSupersessionSelf, got: {err:?}"
    );
}

/// A supersedes link across document keys fails with a distinct
/// typed error naming both revisions (TEST-146).
#[test]
fn cross_document_link_fails_closed() {
    let graph = graph_of(vec![
        revision(SRC_A, DOC_1, None),
        revision(SRC_B, DOC_2, Some(SRC_A)),
    ]);
    let err = graph.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::Source(SourceError::SourceSupersessionDocumentKey {
                ref uid,
                ref predecessor_uid,
            }) if uid == SRC_B && predecessor_uid == SRC_A
        ),
        "a cross-document link must fail with SourceSupersessionDocumentKey, got: {err:?}"
    );
}

/// A revision owning two distinct outgoing supersedes edges fails
/// with a distinct typed error; the record loader cannot produce
/// this shape, so programmatic graphs are guarded (TEST-146).
#[test]
fn duplicate_outgoing_edges_fail_closed() {
    let mut newer = revision(SRC_B, DOC_1, Some(SRC_A));
    let crate::corpus::Node::SourceRevision(newer_revision) = &mut newer else {
        unreachable!("revision() builds a source revision")
    };
    newer_revision
        .edges
        .push((EdgeKind::Supersedes, SRC_C.to_string()));
    let graph = graph_of(vec![
        revision(SRC_A, DOC_1, None),
        newer,
        revision(SRC_C, DOC_1, None),
    ]);
    let err = graph.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::Source(SourceError::SourceDuplicateSupersedesEdge {
                ref source_uid,
                count,
            }) if source_uid == SRC_B && count == 2
        ),
        "duplicate outgoing edges must fail with SourceDuplicateSupersedesEdge, got: {err:?}"
    );
}

/// Two revisions both superseding one predecessor — a fork — fails
/// with a distinct typed error naming the forked pair in uid order
/// (TEST-146).
#[test]
fn fork_fails_closed() {
    let graph = graph_of(vec![
        revision(SRC_A, DOC_1, None),
        revision(SRC_B, DOC_1, Some(SRC_A)),
        revision(SRC_C, DOC_1, Some(SRC_A)),
    ]);
    let err = graph.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::Source(SourceError::SourceSupersessionFork {
                ref uid,
                ref first_uid,
                ref second_uid,
            }) if uid == SRC_A && first_uid == SRC_B && second_uid == SRC_C
        ),
        "a fork must fail with SourceSupersessionFork, got: {err:?}"
    );
}

/// A two-node cycle fails with a distinct typed error (TEST-146).
#[test]
fn cycle_fails_closed() {
    let graph = graph_of(vec![
        revision(SRC_A, DOC_1, Some(SRC_B)),
        revision(SRC_B, DOC_1, Some(SRC_A)),
    ]);
    let err = graph.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::Source(SourceError::SourceSupersessionCycle { .. })
        ),
        "a cycle must fail with SourceSupersessionCycle, got: {err:?}"
    );
}

/// Two unrelated chains sharing one document key — two roots —
/// fail with a distinct typed error naming the key and the uid
/// pair (TEST-146).
#[test]
fn multiple_roots_fail_closed() {
    let graph = graph_of(vec![
        revision(SRC_A, DOC_1, None),
        revision(SRC_B, DOC_1, Some(SRC_A)),
        revision(SRC_C, DOC_1, None),
    ]);
    let err = graph.validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::Source(SourceError::SourceLineageMultipleRoots {
                ref document_key,
                ref first_uid,
                ref second_uid,
            }) if document_key == DOC_1 && first_uid == SRC_A && second_uid == SRC_C
        ),
        "multiple roots must fail with SourceLineageMultipleRoots, got: {err:?}"
    );
}

/// The dual multiple-heads guard fires with a distinct typed error;
/// through the public validator the same shape reports the roots
/// dual first (TEST-146).
#[test]
fn multiple_heads_fail_closed_as_dual_guard() {
    let graph = graph_of(vec![
        revision(SRC_A, DOC_1, None),
        revision(SRC_B, DOC_1, Some(SRC_A)),
        revision(SRC_C, DOC_1, None),
    ]);
    let err = reject_multiple_heads(&graph).unwrap_err();
    assert!(
        matches!(
            err,
            SourceError::SourceLineageMultipleHeads {
                ref document_key,
                ref first_uid,
                ref second_uid,
            } if document_key == DOC_1 && first_uid == SRC_B && second_uid == SRC_C
        ),
        "the dual guard must fail with SourceLineageMultipleHeads, got: {err:?}"
    );
    let public_err = graph.validate().unwrap_err();
    assert!(
        matches!(
            public_err,
            CorpusError::Source(SourceError::SourceLineageMultipleRoots { .. })
        ),
        "the public validator reports the roots dual first, got: {public_err:?}"
    );
}

/// Review supersession chains and source lineage chains validate
/// side by side, and each domain's failures still surface through
/// its own wrapper (TEST-146; review behavior unchanged).
#[test]
fn review_and_source_chains_validate_in_one_graph() {
    let mut nodes = vec![
        requirement(REQ_A),
        review(REV_1, None),
        review(REV_2, Some(REV_1)),
    ];
    nodes.extend(three_chain(DOC_1));
    let graph = graph_of(nodes.clone());
    assert!(
        graph.validate().is_ok(),
        "review and source chains validate side by side"
    );

    let mut source_fork = nodes.clone();
    source_fork.push(revision(SRC_D, DOC_1, Some(SRC_A)));
    let err = graph_of(source_fork).validate().unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::Source(SourceError::SourceSupersessionFork { .. })
        ),
        "a source fork surfaces through CorpusError::Source, got: {err:?}"
    );

    let mut review_fork = nodes;
    review_fork.push(review(REV_3, Some(REV_1)));
    let err = graph_of(review_fork).validate().unwrap_err();
    assert!(
        matches!(err, CorpusError::Review(_)),
        "a review fork still surfaces through CorpusError::Review, got: {err:?}"
    );
}

/// One-, two-, and three-revision histories each derive exactly
/// one deterministic head — the newest revision, selected by the
/// edge set alone (TEST-147).
#[test]
fn linear_chains_derive_one_deterministic_head() {
    let one = graph_of(vec![revision(SRC_A, DOC_1, None)]);
    assert_eq!(
        effective_source_heads(&one),
        [(DOC_1.to_string(), SRC_A.to_string())]
            .into_iter()
            .collect(),
        "one revision is its own head"
    );

    let two = graph_of(vec![
        revision(SRC_A, DOC_1, None),
        revision(SRC_B, DOC_1, Some(SRC_A)),
    ]);
    assert_eq!(
        effective_source_heads(&two),
        [(DOC_1.to_string(), SRC_B.to_string())]
            .into_iter()
            .collect(),
        "the superseding revision is the head"
    );

    let three = graph_of(three_chain(DOC_1));
    assert_eq!(
        effective_source_heads(&three),
        [(DOC_1.to_string(), SRC_C.to_string())]
            .into_iter()
            .collect(),
        "the end of the chain is the head"
    );

    let mut two_docs = three_chain(DOC_1);
    two_docs.extend([
        revision(SRC_D, DOC_2, None),
        revision(SRC_E, DOC_2, Some(SRC_D)),
    ]);
    let two_docs = graph_of(two_docs);
    assert_eq!(
        effective_source_heads(&two_docs),
        [
            (DOC_1.to_string(), SRC_C.to_string()),
            (DOC_2.to_string(), SRC_E.to_string()),
        ]
        .into_iter()
        .collect(),
        "each document key derives its own head in sorted order"
    );
}

/// Equivalent file layouts and record insertion orders load into
/// identical graphs and derive identical lineage and head views
/// (TEST-147).
#[test]
fn layout_and_insertion_order_produce_identical_lineage_and_heads() {
    use super::super::tests_support::{load_source_content, source_file, vendored, write};

    let prior = vendored(SRC_A, "SRC-A");
    let mut middle = vendored(SRC_B, "SRC-B");
    middle.supersedes = Some(SRC_A.to_string());
    let mut newest = vendored(SRC_C, "SRC-C");
    newest.supersedes = Some(SRC_B.to_string());

    let single = load_source_content(&source_file(&[
        prior.clone(),
        middle.clone(),
        newest.clone(),
    ]))
    .expect("single-file layout loads");
    let shuffled = load_source_content(&source_file(&[
        newest.clone(),
        middle.clone(),
        prior.clone(),
    ]))
    .expect("shuffled insertion order loads");

    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("split/first.toml");
    let second = dir.path().join("split/second.toml");
    write(&first, &source_file(&[middle.clone()]));
    write(&second, &source_file(&[prior.clone(), newest.clone()]));
    let mut split = crate::corpus::CorpusGraph::new();
    super::super::records::load_sources_into(&first, &mut split).unwrap();
    super::super::records::load_sources_into(&second, &mut split).unwrap();

    assert_eq!(single, shuffled, "insertion order is non-semantic");
    assert_eq!(single, split, "file layout is non-semantic");
    let expected_heads = [("DOC-1".to_string(), SRC_C.to_string())]
        .into_iter()
        .collect();
    assert_eq!(effective_source_heads(&single), expected_heads);
    assert_eq!(effective_source_heads(&shuffled), expected_heads);
    assert_eq!(effective_source_heads(&split), expected_heads);

    // End to end through the corpus index: two layouts validate
    // into equal graphs with equal heads.
    let index_a = tempfile::tempdir().unwrap();
    write(
        &index_a.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources/**/*.toml\"]\n",
    );
    write(
        &index_a.path().join("sources/records.toml"),
        &source_file(&[prior.clone(), middle.clone(), newest.clone()]),
    );
    let index_b = tempfile::tempdir().unwrap();
    write(
        &index_b.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources/**/*.toml\"]\n",
    );
    write(
        &index_b.path().join("sources/a.toml"),
        &source_file(&[newest, middle]),
    );
    write(
        &index_b.path().join("sources/b.toml"),
        &source_file(&[prior]),
    );
    let graph_a = CorpusIndex::load_graph(&index_a.path().join("corpus.toml")).unwrap();
    let graph_b = CorpusIndex::load_graph(&index_b.path().join("corpus.toml")).unwrap();
    assert_eq!(graph_a, graph_b, "index layouts load into equal graphs");
    assert_eq!(
        effective_source_heads(&graph_a),
        effective_source_heads(&graph_b),
        "index layouts derive identical heads"
    );
}

/// Retrieval timestamps never select a head: the edge set alone
/// decides, so the newest revision stays head even when it carries
/// the oldest timestamp (TEST-147).
#[test]
fn timestamps_never_select_a_head() {
    let mut oldest_chain_tip = revision(SRC_A, DOC_1, None);
    source_node_mut(&mut oldest_chain_tip).material = material_at(DIGEST_A, "2026-07-03T10:00:00Z");
    let mut middle = revision(SRC_B, DOC_1, Some(SRC_A));
    source_node_mut(&mut middle).material = material_at(DIGEST_A, "2026-07-02T10:00:00Z");
    let mut newest = revision(SRC_C, DOC_1, Some(SRC_B));
    source_node_mut(&mut newest).material = material_at(DIGEST_A, "2026-07-01T10:00:00Z");
    let graph = graph_of(vec![oldest_chain_tip, middle, newest]);
    assert!(graph.validate().is_ok());
    assert_eq!(
        effective_source_heads(&graph),
        [(DOC_1.to_string(), SRC_C.to_string())]
            .into_iter()
            .collect(),
        "the edge-selected head wins over any timestamp order"
    );
}
