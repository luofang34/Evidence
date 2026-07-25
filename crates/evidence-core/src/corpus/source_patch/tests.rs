//! Record schema, identity, binding, and digest unit tests for
//! curated patches (TEST-184, TEST-185).

use std::path::Path;

use super::super::digest::StructuralContentDigest;
use super::super::graph::CorpusGraph;
use super::super::source_graph::SourceNodeKind;
use super::PatchOperation;
use super::digest::{reviewed_content_bytes, reviewed_content_digest};
use super::error::SourcePatchError;
use super::records::{SUPPORTED_SOURCE_PATCH_SCHEMA, parse_source_patch};
use super::tests_support::{
    INPUT_HEX, NodeRow, RECIPE_HEX, build_graph, finalize, insert_op, patch_record, patch_toml,
};

const PATCH_UID: &str = "patch_00000000-0000-4000-8000-0000000000e1";
const SEC: &str = "snode_00000000-0000-4000-8000-0000000000e2";
const PARA: &str = "snode_00000000-0000-4000-8000-0000000000e3";

fn fixture_graph() -> super::super::source_graph::SourceGraph {
    build_graph(&[
        NodeRow {
            uid: SEC,
            parent: None,
            kind: SourceNodeKind::Section,
            ordinal: 0,
            label: Some("1 Intro"),
            text: "1 Intro",
        },
        NodeRow {
            uid: PARA,
            parent: Some(SEC),
            kind: SourceNodeKind::Paragraph,
            ordinal: 0,
            label: None,
            text: "the parser text",
        },
    ])
}

fn fixture_record() -> super::records::SourcePatchRecord {
    let graph = fixture_graph();
    let para_digest = graph.get(PARA).unwrap().content_sha256.clone();
    finalize(
        patch_record(
            PATCH_UID,
            "fix-para",
            vec![
                PatchOperation::ReplaceContent {
                    ordinal: 0,
                    target_uid: PARA.to_string(),
                    expected_content_sha256: para_digest,
                    new_canonical_text: Some("the curated text".to_string()),
                    new_label: None,
                },
                insert_op(
                    1,
                    Some(SEC),
                    "snode_00000000-0000-4000-8000-0000000000e4",
                    SourceNodeKind::Note,
                    1,
                    "a curated note",
                ),
            ],
        ),
        &graph,
    )
}

fn parse_ok(toml_text: &str) -> super::records::SourcePatchRecord {
    parse_source_patch(Path::new("patch.toml"), toml_text).unwrap()
}

/// A valid record round-trips through the strict parser; unknown
/// fields, an unknown operation tag, and a newer schema fail
/// closed (TEST-184).
#[test]
fn patch_record_round_trips_and_rejects_unknown_fields_and_newer_schema() {
    let record = fixture_record();
    let parsed = parse_ok(&patch_toml(&record));
    assert_eq!(parsed, record, "a valid record round-trips");

    let unknown_field = patch_toml(&record).replace("[patch]\n", "[patch]\nsurprise = 1\n");
    let err = parse_source_patch(Path::new("patch.toml"), &unknown_field).unwrap_err();
    assert!(
        matches!(err, SourcePatchError::RecordParse { .. }),
        "an unknown field must fail closed, got: {err:?}"
    );

    let unknown_op = patch_toml(&record).replace("op = \"reparent\"", "op = \"json_patch\"");
    let unknown_op = unknown_op.replacen("op = \"replace_content\"", "op = \"move\"", 1);
    let err = parse_source_patch(Path::new("patch.toml"), &unknown_op).unwrap_err();
    assert!(
        matches!(err, SourcePatchError::RecordParse { .. }),
        "an unknown operation tag must be unrepresentable, got: {err:?}"
    );

    let newer = patch_toml(&record).replace(
        "schema_version = 1",
        &format!("schema_version = {}", SUPPORTED_SOURCE_PATCH_SCHEMA + 1),
    );
    let err = parse_source_patch(Path::new("patch.toml"), &newer).unwrap_err();
    assert!(
        matches!(err, SourcePatchError::RecordSchemaTooNew { .. }),
        "a newer schema must fail closed, got: {err:?}"
    );
}

/// The `patch_` UUIDv4 uid contract and per-kind human-id
/// uniqueness are enforced (TEST-184).
#[test]
fn patch_uid_and_human_id_contract_enforced() {
    let record = fixture_record();
    for bad_uid in [
        "prop_00000000-0000-4000-8000-0000000000e1",
        "patch_not-a-uuid",
    ] {
        let mut bad = record.clone();
        bad.uid = bad_uid.to_string();
        let err = parse_source_patch(Path::new("patch.toml"), &patch_toml(&bad)).unwrap_err();
        assert!(
            matches!(
                err,
                SourcePatchError::NativeUidPrefix { .. } | SourcePatchError::NativeUidUuidV4 { .. }
            ),
            "uid {bad_uid:?} must fail closed, got: {err:?}"
        );
    }

    let mut graph = CorpusGraph::new();
    graph.insert_source_patch(record.clone()).unwrap();
    let err = graph.insert_source_patch(record.clone()).unwrap_err();
    assert!(
        matches!(err, SourcePatchError::DuplicateUid { .. }),
        "a duplicate patch uid must fail closed, got: {err:?}"
    );
    let mut same_human_id = record.clone();
    same_human_id.uid = "patch_00000000-0000-4000-8000-0000000000e9".to_string();
    let err = graph.insert_source_patch(same_human_id).unwrap_err();
    assert!(
        matches!(err, SourcePatchError::DuplicateHumanId { .. }),
        "a duplicate human id must fail closed, got: {err:?}"
    );
}

/// Metadata, binding, and operation field validation: blank
/// metadata, a malformed timestamp, an empty operation list,
/// duplicate ordinals, conflicting operations, an incomplete
/// replace, and a reviewed-content digest that does not recompute
/// all fail closed (TEST-184).
#[test]
fn patch_bindings_metadata_and_operation_fields_validated() {
    let record = fixture_record();
    let path = Path::new("patch.toml");

    let mut blank_author = record.clone();
    blank_author.author = "   ".to_string();
    let err = parse_source_patch(path, &patch_toml(&blank_author)).unwrap_err();
    assert!(
        matches!(
            err,
            SourcePatchError::BlankField {
                field: "author",
                ..
            }
        ),
        "a blank author must fail closed, got: {err:?}"
    );

    let mut bad_time = record.clone();
    bad_time.created_at = "not-a-time".to_string();
    let err = parse_source_patch(path, &patch_toml(&bad_time)).unwrap_err();
    assert!(
        matches!(err, SourcePatchError::PatchTimestamp { .. }),
        "a malformed created_at must fail closed, got: {err:?}"
    );

    let mut no_ops = record.clone();
    no_ops.operations.clear();
    let err = parse_source_patch(path, &patch_toml(&no_ops)).unwrap_err();
    assert!(
        matches!(err, SourcePatchError::EmptyOperations { .. }),
        "an empty operation list must fail closed, got: {err:?}"
    );

    let mut dup_ordinal = record.clone();
    let mut second = dup_ordinal.operations[1].clone();
    if let PatchOperation::Insert { ordinal, .. } = &mut second {
        *ordinal = 0;
    }
    dup_ordinal.operations.push(second);
    let err = parse_source_patch(path, &patch_toml(&dup_ordinal)).unwrap_err();
    assert!(
        matches!(
            err,
            SourcePatchError::DuplicateOperationOrdinal { ordinal: 0, .. }
        ),
        "a duplicate ordinal must fail closed, got: {err:?}"
    );

    let mut conflicting = record.clone();
    conflicting
        .operations
        .push(conflicting.operations[0].clone());
    if let PatchOperation::ReplaceContent { ordinal, .. } = &mut conflicting.operations[2] {
        *ordinal = 7;
    }
    let err = parse_source_patch(path, &patch_toml(&conflicting)).unwrap_err();
    assert!(
        matches!(
            err,
            SourcePatchError::ConflictingOperation {
                op: "replace_content",
                ..
            }
        ),
        "a duplicate operation on one target must fail closed, got: {err:?}"
    );

    let mut incomplete = record.clone();
    incomplete.operations[0] = PatchOperation::ReplaceContent {
        ordinal: 0,
        target_uid: PARA.to_string(),
        expected_content_sha256: StructuralContentDigest::from_hex(RECIPE_HEX).unwrap(),
        new_canonical_text: None,
        new_label: None,
    };
    let err = parse_source_patch(path, &patch_toml(&incomplete)).unwrap_err();
    assert!(
        matches!(err, SourcePatchError::IncompleteReplaceContent { .. }),
        "a replace with no replacement must fail closed, got: {err:?}"
    );

    let mut stale_digest = record.clone();
    stale_digest.reviewed_content_digest = StructuralContentDigest::from_hex(INPUT_HEX).unwrap();
    let err = parse_source_patch(path, &patch_toml(&stale_digest)).unwrap_err();
    assert!(
        matches!(err, SourcePatchError::ReviewedContentDigestMismatch { .. }),
        "a stale reviewed-content digest must fail closed, got: {err:?}"
    );
}

/// Reordered operation blocks parse to the identical record and
/// digest, and author, rationale, creation metadata, uid, and
/// human id stay outside semantic identity while every operation
/// precondition stays inside it (TEST-185).
#[test]
fn reordered_operations_and_metadata_stay_outside_semantic_identity() {
    let record = fixture_record();
    let mut reordered = record.clone();
    reordered.operations.reverse();
    let canonical = parse_ok(&patch_toml(&record));
    let reparsed = parse_ok(&patch_toml(&reordered));
    assert_eq!(
        canonical, reparsed,
        "reordered operation blocks must produce the identical patch"
    );
    assert_eq!(
        reviewed_content_bytes(&canonical),
        reviewed_content_bytes(&reparsed),
        "reordered operation blocks must digest identically"
    );

    let baseline = reviewed_content_digest(&record);
    for tweaked in [
        {
            let mut r = record.clone();
            r.author = "someone-else@example.com".to_string();
            r
        },
        {
            let mut r = record.clone();
            r.rationale = "a different rationale".to_string();
            r
        },
        {
            let mut r = record.clone();
            r.created_at = "2026-07-26T00:00:00Z".to_string();
            r
        },
        {
            let mut r = record.clone();
            r.human_id = "renamed".to_string();
            r
        },
        {
            let mut r = record.clone();
            r.uid = "patch_00000000-0000-4000-8000-0000000000e8".to_string();
            r
        },
    ] {
        assert_eq!(
            reviewed_content_digest(&tweaked),
            baseline,
            "metadata and identity fields must stay outside semantic identity"
        );
    }

    let mut moved = record.clone();
    if let PatchOperation::Reclassify { .. } = &moved.operations[0] {
        panic!("fixture op 0 must be replace_content");
    }
    moved.operations[0] = PatchOperation::Reclassify {
        ordinal: 0,
        target_uid: PARA.to_string(),
        expected_kind: SourceNodeKind::Paragraph,
        new_kind: SourceNodeKind::Note,
    };
    assert_ne!(
        reviewed_content_digest(&moved),
        baseline,
        "a changed operation must move the reviewed-content digest"
    );
}
