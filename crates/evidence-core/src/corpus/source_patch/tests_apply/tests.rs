//! Candidate-application unit tests for curated patches
//! (TEST-186): every operation succeeds under exact preconditions
//! and fails closed when stale; conflicts, cross-document edits,
//! dangling targets, cascades, and invalid post-graphs are typed
//! errors; application is atomic.

use std::path::Path;

use super::apply::{PatchBindings, apply_patch};
use super::digest::source_graph_digest;
use super::error::SourcePatchError;
use super::records::{SourcePatchRecord, parse_source_patch};
use super::tests_support::{
    NodeRow, build_graph, finalize, input_digest, insert_op, patch_record, patch_toml,
    recipe_digest,
};
use super::{ChildDisposition, PatchOperation};
use crate::corpus::source_graph::{SourceGraph, SourceNodeKind};

const PATCH_UID: &str = "patch_00000000-0000-4000-8000-0000000000f1";
const SEC: &str = "snode_00000000-0000-4000-8000-0000000000f2";
const TBL: &str = "snode_00000000-0000-4000-8000-0000000000f3";
const ROW1: &str = "snode_00000000-0000-4000-8000-0000000000f4";
const CELL1: &str = "snode_00000000-0000-4000-8000-0000000000f5";
const CELL2: &str = "snode_00000000-0000-4000-8000-0000000000f6";
const PARA: &str = "snode_00000000-0000-4000-8000-0000000000f7";
const NEW_NODE: &str = "snode_00000000-0000-4000-8000-0000000000f8";

const MEDIA: &str = "text/markdown";

/// section
/// ├── table
/// │   └── row1
/// │       ├── cell1 "Requirement"
/// │       └── cell2 "Mandatory"
/// └── paragraph "loose prose"
fn fixture_graph() -> SourceGraph {
    build_graph(&[
        NodeRow {
            uid: SEC,
            parent: None,
            kind: SourceNodeKind::Section,
            ordinal: 0,
            label: Some("PICS"),
            text: "PICS",
        },
        NodeRow {
            uid: TBL,
            parent: Some(SEC),
            kind: SourceNodeKind::Table,
            ordinal: 0,
            label: None,
            text: "capabilities",
        },
        NodeRow {
            uid: ROW1,
            parent: Some(TBL),
            kind: SourceNodeKind::TableRow,
            ordinal: 0,
            label: None,
            text: "header",
        },
        NodeRow {
            uid: CELL1,
            parent: Some(ROW1),
            kind: SourceNodeKind::TableCell,
            ordinal: 0,
            label: None,
            text: "Requirement",
        },
        NodeRow {
            uid: CELL2,
            parent: Some(ROW1),
            kind: SourceNodeKind::TableCell,
            ordinal: 1,
            label: None,
            text: "Mandatory",
        },
        NodeRow {
            uid: PARA,
            parent: Some(SEC),
            kind: SourceNodeKind::Paragraph,
            ordinal: 1,
            label: None,
            text: "loose prose",
        },
    ])
}

fn bindings() -> PatchBindings {
    PatchBindings {
        recipe_digest: recipe_digest(),
        input_digest: input_digest(),
    }
}

fn apply_ok(graph: &SourceGraph, record: &SourcePatchRecord) -> super::apply::PatchApplication {
    apply_patch(graph, record, &bindings(), MEDIA).unwrap()
}

fn content_of(graph: &SourceGraph, uid: &str) -> super::super::digest::StructuralContentDigest {
    graph.get(uid).unwrap().content_sha256.clone()
}

/// Every operation applies under exact preconditions and fails
/// closed with a typed stale-precondition error when the state has
/// moved (TEST-186).
#[test]
fn each_operation_applies_under_exact_preconditions_and_fails_closed_when_stale() {
    let graph = fixture_graph();

    // replace_content: text and label.
    let record = finalize(
        patch_record(
            PATCH_UID,
            "replace",
            vec![PatchOperation::ReplaceContent {
                ordinal: 0,
                target_uid: PARA.to_string(),
                expected_content_sha256: content_of(&graph, PARA),
                new_canonical_text: Some("curated prose".to_string()),
                new_label: Some("curated label".to_string()),
            }],
        ),
        &graph,
    );
    let applied = apply_ok(&graph, &record);
    let node = applied.graph.get(PARA).unwrap();
    assert_eq!(node.canonical_text, "curated prose");
    assert_eq!(node.label.as_deref(), Some("curated label"));

    let mut stale = record.clone();
    stale.operations[0] = PatchOperation::ReplaceContent {
        ordinal: 0,
        target_uid: PARA.to_string(),
        expected_content_sha256: content_of(&graph, CELL1),
        new_canonical_text: Some("curated prose".to_string()),
        new_label: None,
    };
    stale.reviewed_content_digest = super::digest::reviewed_content_digest(&stale);
    let err = apply_patch(&graph, &stale, &bindings(), MEDIA).unwrap_err();
    assert!(
        matches!(
            err,
            SourcePatchError::StalePrecondition {
                field: "content_sha256",
                ordinal: 0,
                ..
            }
        ),
        "a stale content digest must fail closed, got: {err:?}"
    );

    // reclassify.
    let record = finalize(
        patch_record(
            PATCH_UID,
            "reclassify",
            vec![PatchOperation::Reclassify {
                ordinal: 0,
                target_uid: PARA.to_string(),
                expected_kind: SourceNodeKind::Paragraph,
                new_kind: SourceNodeKind::Note,
            }],
        ),
        &graph,
    );
    let applied = apply_ok(&graph, &record);
    assert_eq!(applied.graph.get(PARA).unwrap().kind, SourceNodeKind::Note);

    let mut stale = record.clone();
    stale.operations[0] = PatchOperation::Reclassify {
        ordinal: 0,
        target_uid: PARA.to_string(),
        expected_kind: SourceNodeKind::Note,
        new_kind: SourceNodeKind::Paragraph,
    };
    stale.reviewed_content_digest = super::digest::reviewed_content_digest(&stale);
    let err = apply_patch(&graph, &stale, &bindings(), MEDIA).unwrap_err();
    assert!(
        matches!(
            err,
            SourcePatchError::StalePrecondition { field: "kind", .. }
        ),
        "a stale expected kind must fail closed, got: {err:?}"
    );

    // reparent.
    let record = finalize(
        patch_record(
            PATCH_UID,
            "reparent",
            vec![PatchOperation::Reparent {
                ordinal: 0,
                target_uid: CELL2.to_string(),
                expected_parent_uid: Some(ROW1.to_string()),
                expected_ordinal: 1,
                new_parent_uid: Some(SEC.to_string()),
                new_ordinal: 2,
            }],
        ),
        &graph,
    );
    let applied = apply_ok(&graph, &record);
    let node = applied.graph.get(CELL2).unwrap();
    assert_eq!(node.parent_uid.as_deref(), Some(SEC));
    assert_eq!(node.ordinal, 2);

    for (expected_parent, expected_ord, field) in [
        (Some(TBL.to_string()), 1, "parent_uid"),
        (Some(ROW1.to_string()), 0, "ordinal"),
    ] {
        let mut stale = record.clone();
        stale.operations[0] = PatchOperation::Reparent {
            ordinal: 0,
            target_uid: CELL2.to_string(),
            expected_parent_uid: expected_parent,
            expected_ordinal: expected_ord,
            new_parent_uid: Some(SEC.to_string()),
            new_ordinal: 2,
        };
        stale.reviewed_content_digest = super::digest::reviewed_content_digest(&stale);
        let err = apply_patch(&graph, &stale, &bindings(), MEDIA).unwrap_err();
        assert!(
            matches!(err, SourcePatchError::StalePrecondition { field: f, .. } if f == field),
            "a stale {field} must fail closed, got: {err:?}"
        );
    }

    // insert.
    let record = finalize(
        patch_record(
            PATCH_UID,
            "insert",
            vec![insert_op(
                0,
                Some(ROW1),
                NEW_NODE,
                SourceNodeKind::TableCell,
                2,
                "Support",
            )],
        ),
        &graph,
    );
    let applied = apply_ok(&graph, &record);
    let node = applied.graph.get(NEW_NODE).unwrap();
    assert_eq!(node.kind, SourceNodeKind::TableCell);
    assert_eq!(node.parent_uid.as_deref(), Some(ROW1));

    // remove with each explicit child disposition. Removing the
    // table promotes its row to the section (Section parents any
    // kind), taking the table's position.
    let record = finalize(
        patch_record(
            PATCH_UID,
            "remove-promote",
            vec![PatchOperation::Remove {
                ordinal: 0,
                target_uid: TBL.to_string(),
                expected_digest: content_of(&graph, TBL),
                child_disposition: ChildDisposition::ReparentChildren,
            }],
        ),
        &graph,
    );
    let applied = apply_ok(&graph, &record);
    assert!(applied.graph.get(TBL).is_none(), "the node is removed");
    let promoted = applied.graph.get(ROW1).unwrap();
    assert_eq!(promoted.parent_uid.as_deref(), Some(SEC));
    assert_eq!(promoted.ordinal, 0, "the promoted child takes the position");
    assert_eq!(
        applied.graph.get(PARA).unwrap().ordinal,
        1,
        "later siblings renumber contiguously"
    );

    let record = finalize(
        patch_record(
            PATCH_UID,
            "remove-subtree",
            vec![PatchOperation::Remove {
                ordinal: 0,
                target_uid: ROW1.to_string(),
                expected_digest: content_of(&graph, ROW1),
                child_disposition: ChildDisposition::RemoveSubtree,
            }],
        ),
        &graph,
    );
    let applied = apply_ok(&graph, &record);
    for uid in [ROW1, CELL1, CELL2] {
        assert!(
            applied.graph.get(uid).is_none(),
            "{uid} is removed with the subtree"
        );
    }
}

/// Application is atomic: a later operation's failure leaves the
/// original graph byte-identical (TEST-186).
#[test]
fn application_is_atomic_and_leaves_the_original_graph_untouched() {
    let graph = fixture_graph();
    let before = graph.clone();
    let before_digest = source_graph_digest(&graph);

    let mut record = finalize(
        patch_record(
            PATCH_UID,
            "atomic",
            vec![
                PatchOperation::Reclassify {
                    ordinal: 0,
                    target_uid: PARA.to_string(),
                    expected_kind: SourceNodeKind::Paragraph,
                    new_kind: SourceNodeKind::Note,
                },
                PatchOperation::Reclassify {
                    ordinal: 1,
                    target_uid: CELL1.to_string(),
                    expected_kind: SourceNodeKind::Paragraph,
                    new_kind: SourceNodeKind::Note,
                },
            ],
        ),
        &graph,
    );
    record.reviewed_content_digest = super::digest::reviewed_content_digest(&record);
    let err = apply_patch(&graph, &record, &bindings(), MEDIA).unwrap_err();
    assert!(
        matches!(err, SourcePatchError::StalePrecondition { ordinal: 1, .. }),
        "the second operation must fail closed, got: {err:?}"
    );
    assert_eq!(graph, before, "the original graph is untouched");
    assert_eq!(
        source_graph_digest(&graph),
        before_digest,
        "the original graph digests identically"
    );

    // The recorded digests of a successful application.
    let record = finalize(
        patch_record(
            PATCH_UID,
            "atomic-ok",
            vec![PatchOperation::Reclassify {
                ordinal: 0,
                target_uid: PARA.to_string(),
                expected_kind: SourceNodeKind::Paragraph,
                new_kind: SourceNodeKind::Note,
            }],
        ),
        &graph,
    );
    let applied = apply_ok(&graph, &record);
    assert_eq!(applied.pre_patch_digest, before_digest);
    assert_eq!(applied.patch_digest, record.reviewed_content_digest);
    assert_ne!(
        applied.post_patch_digest, applied.pre_patch_digest,
        "the correction moves the graph digest"
    );
    assert_eq!(
        source_graph_digest(&graph),
        before_digest,
        "the committed graph is never mutated by candidate application"
    );
}

/// A reordered record file produces the identical patch and
/// application result (TEST-185 ordering contract, exercised end
/// to end here).
#[test]
fn reordered_record_files_produce_identical_patch_sets_and_results() {
    let graph = fixture_graph();
    let record = finalize(
        patch_record(
            PATCH_UID,
            "reorder",
            vec![
                PatchOperation::Reclassify {
                    ordinal: 0,
                    target_uid: PARA.to_string(),
                    expected_kind: SourceNodeKind::Paragraph,
                    new_kind: SourceNodeKind::Note,
                },
                PatchOperation::ReplaceContent {
                    ordinal: 1,
                    target_uid: CELL1.to_string(),
                    expected_content_sha256: content_of(&graph, CELL1),
                    new_canonical_text: Some("Curated requirement".to_string()),
                    new_label: None,
                },
            ],
        ),
        &graph,
    );
    let mut reordered = record.clone();
    reordered.operations.reverse();
    let canonical = parse_source_patch(Path::new("a.toml"), &patch_toml(&record)).unwrap();
    let reparsed = parse_source_patch(Path::new("b.toml"), &patch_toml(&reordered)).unwrap();
    assert_eq!(canonical, reparsed, "reordered files parse identically");
    let first = apply_ok(&graph, &canonical);
    let second = apply_ok(&graph, &reparsed);
    assert_eq!(first, second, "reordered files apply identically");
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "tests_errors.rs"]
mod tests_errors;
