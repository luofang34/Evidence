//! Typed-error unit tests for curated-patch candidate application
//! (TEST-186): conflicts, duplicate operations, cross-document
//! edits, dangling targets, cascades, and invalid post-graphs.

use super::super::PatchOperation;
use super::super::apply::apply_patch;
use super::super::error::SourcePatchError;
use super::super::tests_support::{
    NodeRow, build_graph, finalize, input_digest, insert_op, patch_record,
};
use super::{
    CELL1, CELL2, MEDIA, NEW_NODE, PARA, PATCH_UID, ROW1, SEC, TBL, bindings, fixture_graph,
};
use crate::corpus::graph::CorpusGraph;
use crate::corpus::source_graph::SourceNodeKind;

/// Conflicts, duplicate operations, cross-document edits, dangling
/// targets, cascades, and invalid post-graphs are typed errors
/// (TEST-186).
#[test]
fn conflicts_duplicates_cross_document_dangling_cascade_and_invalid_post_graph_are_typed_errors() {
    let graph = fixture_graph();

    // Dangling target at application.
    let mut record = finalize(
        patch_record(
            PATCH_UID,
            "dangling",
            vec![PatchOperation::Reclassify {
                ordinal: 0,
                target_uid: "snode_00000000-0000-4000-8000-0000000000ff".to_string(),
                expected_kind: SourceNodeKind::Paragraph,
                new_kind: SourceNodeKind::Note,
            }],
        ),
        &graph,
    );
    record.reviewed_content_digest = super::super::digest::reviewed_content_digest(&record);
    let err = apply_patch(&graph, &record, &bindings(), MEDIA).unwrap_err();
    assert!(
        matches!(err, SourcePatchError::DanglingTarget { .. }),
        "a dangling target must fail closed, got: {err:?}"
    );

    // Dangling insert parent.
    let mut record = finalize(
        patch_record(
            PATCH_UID,
            "dangling-parent",
            vec![insert_op(
                0,
                Some("snode_00000000-0000-4000-8000-0000000000fe"),
                NEW_NODE,
                SourceNodeKind::Note,
                0,
                "orphan",
            )],
        ),
        &graph,
    );
    record.reviewed_content_digest = super::super::digest::reviewed_content_digest(&record);
    let err = apply_patch(&graph, &record, &bindings(), MEDIA).unwrap_err();
    assert!(
        matches!(err, SourcePatchError::DanglingTarget { .. }),
        "a dangling insert parent must fail closed, got: {err:?}"
    );

    // Inserted-uid collision.
    let mut record = finalize(
        patch_record(
            PATCH_UID,
            "collision",
            vec![insert_op(
                0,
                Some(SEC),
                CELL1,
                SourceNodeKind::Note,
                2,
                "collision",
            )],
        ),
        &graph,
    );
    record.reviewed_content_digest = super::super::digest::reviewed_content_digest(&record);
    let err = apply_patch(&graph, &record, &bindings(), MEDIA).unwrap_err();
    assert!(
        matches!(
            err,
            SourcePatchError::InsertedIdentityCollision { field: "uid", .. }
        ),
        "an inserted-uid collision must fail closed, got: {err:?}"
    );

    // Illegal cascade: a reparent that strands the sibling set's
    // ordinals fails the post-patch validator.
    let mut record = finalize(
        patch_record(
            PATCH_UID,
            "gapped",
            vec![PatchOperation::Reparent {
                ordinal: 0,
                target_uid: CELL2.to_string(),
                expected_parent_uid: Some(ROW1.to_string()),
                expected_ordinal: 1,
                new_parent_uid: Some(ROW1.to_string()),
                new_ordinal: 5,
            }],
        ),
        &graph,
    );
    record.reviewed_content_digest = super::super::digest::reviewed_content_digest(&record);
    let err = apply_patch(&graph, &record, &bindings(), MEDIA).unwrap_err();
    assert!(
        matches!(err, SourcePatchError::InvalidPostGraph { .. }),
        "a gapped sibling set must fail closed, got: {err:?}"
    );

    // Illegal parentage: a table cell directly under a table.
    let mut record = finalize(
        patch_record(
            PATCH_UID,
            "illegal-parent",
            vec![PatchOperation::Reparent {
                ordinal: 0,
                target_uid: CELL1.to_string(),
                expected_parent_uid: Some(ROW1.to_string()),
                expected_ordinal: 0,
                new_parent_uid: Some(TBL.to_string()),
                new_ordinal: 1,
            }],
        ),
        &graph,
    );
    record.reviewed_content_digest = super::super::digest::reviewed_content_digest(&record);
    let err = apply_patch(&graph, &record, &bindings(), MEDIA).unwrap_err();
    assert!(
        matches!(err, SourcePatchError::InvalidPostGraph { .. }),
        "an illegal parent kind must fail closed, got: {err:?}"
    );

    // Stale bindings: recipe, input, and pre-patch graph digests.
    let record = finalize(
        patch_record(
            PATCH_UID,
            "bindings",
            vec![PatchOperation::Reclassify {
                ordinal: 0,
                target_uid: PARA.to_string(),
                expected_kind: SourceNodeKind::Paragraph,
                new_kind: SourceNodeKind::Note,
            }],
        ),
        &graph,
    );
    let stale_recipe = super::PatchBindings {
        recipe_digest: input_digest(),
        input_digest: input_digest(),
    };
    let err = apply_patch(&graph, &record, &stale_recipe, MEDIA).unwrap_err();
    assert!(
        matches!(
            err,
            SourcePatchError::StaleBinding {
                field: "recipe_digest",
                ..
            }
        ),
        "a stale recipe binding must fail closed, got: {err:?}"
    );
    let other_graph = build_graph(&[NodeRow {
        uid: SEC,
        parent: None,
        kind: SourceNodeKind::Section,
        ordinal: 0,
        label: Some("PICS"),
        text: "PICS",
    }]);
    let err = apply_patch(&other_graph, &record, &bindings(), MEDIA).unwrap_err();
    assert!(
        matches!(
            err,
            SourcePatchError::StaleBinding {
                field: "pre_patch_graph_digest",
                ..
            }
        ),
        "a stale pre-patch graph binding must fail closed, got: {err:?}"
    );

    // Cross-document edit at corpus validation: the patch targets
    // a node that exists only in another revision.
    let other_revision = "src_00000000-0000-4000-8000-0000000000d9";
    let other_node = "snode_00000000-0000-4000-8000-0000000000f9";
    let mut corpus = CorpusGraph::new();
    corpus
        .insert(revision_node(
            crate::corpus::source_patch::tests_support::REV,
            "d1",
        ))
        .unwrap();
    corpus.insert(revision_node(other_revision, "d9")).unwrap();
    for node in fixture_graph().nodes() {
        corpus.insert_source_node(node.clone()).unwrap();
    }
    let mut foreign = build_graph(&[NodeRow {
        uid: other_node,
        parent: None,
        kind: SourceNodeKind::Paragraph,
        ordinal: 0,
        label: None,
        text: "other revision node",
    }])
    .remove_node(other_node)
    .unwrap();
    foreign.source_revision_uid = other_revision.to_string();
    corpus.insert_source_node(foreign).unwrap();
    let record = finalize(
        patch_record(
            PATCH_UID,
            "cross-doc",
            vec![PatchOperation::Reclassify {
                ordinal: 0,
                target_uid: other_node.to_string(),
                expected_kind: SourceNodeKind::Paragraph,
                new_kind: SourceNodeKind::Note,
            }],
        ),
        &fixture_graph(),
    );
    corpus.insert_source_patch(record).unwrap();
    let err = corpus.validate().unwrap_err();
    assert!(
        matches!(
            err,
            crate::corpus::CorpusError::SourcePatch(ref inner)
                if matches!(**inner, SourcePatchError::CrossRevisionTarget { .. })
        ),
        "a cross-document edit must fail closed, got: {err:?}"
    );
}

/// A minimal committed source-revision node.
fn revision_node(uid: &str, marker: &str) -> crate::corpus::graph::Node {
    use crate::corpus::graph::{Node, SourceMaterial, SourceRevisionNode};
    Node::SourceRevision(SourceRevisionNode {
        uid: uid.to_string(),
        id: format!("SRC-{marker}"),
        document_key: format!("DOC-{marker}"),
        title: format!("revision {marker}"),
        media_type: MEDIA.to_string(),
        canonical_location: format!("https://example.org/{marker}"),
        material: SourceMaterial::Unavailable {
            reason: format!("fixture {marker}"),
        },
        edges: Vec::new(),
    })
}
