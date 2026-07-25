//! Tests for the schema-2 typed review target: round-trip of both
//! target kinds, mixed legacy/new layouts, and fail-closed target
//! shape and kind validation (TEST-188).

use super::tests::{
    REQ_A, REV_1, REV_2, REV_3, approve, expect_load_err, expect_review, load_corpus, review_err,
    review_file, write,
};
use crate::corpus::{CorpusGraph, EdgeKind, ReviewError, ReviewTarget, ReviewTargetKind};

const PATCH_A: &str = "patch_00000000-0000-4000-8000-0000000000b1";

/// One schema-2 typed-target record's field values.
struct TypedSpec<'a> {
    uid: &'a str,
    id: &'a str,
    kind: &'a str,
    target_uid: &'a str,
    decision: &'a str,
    rationale: Option<&'a str>,
}

fn approve_typed<'a>(
    uid: &'a str,
    id: &'a str,
    kind: &'a str,
    target_uid: &'a str,
) -> TypedSpec<'a> {
    TypedSpec {
        uid,
        id,
        kind,
        target_uid,
        decision: "approve",
        rationale: None,
    }
}

/// Render a schema-2 review file: records carry the typed
/// `target = { kind, uid }` table instead of `requirement_uid`.
fn typed_review_file(specs: &[TypedSpec]) -> String {
    let mut out = "schema_version = 2\n".to_string();
    for spec in specs {
        out.push_str(&format!(
            "\n[[reviews]]\nuid = \"{}\"\nid = \"{}\"\ntarget = {{ kind = \"{}\", uid = \"{}\" }}\n\
             content_schema = 1\nreviewed_content_sha256 = \"{}\"\ndecision = \"{}\"\n\
             reviewer = \"alice@example.com\"\nreviewed_at = \"2026-07-01T10:00:00Z\"\n",
            spec.uid,
            spec.id,
            spec.kind,
            spec.target_uid,
            "a".repeat(64),
            spec.decision,
        ));
        if let Some(rationale) = spec.rationale {
            out.push_str(&format!("rationale = \"{rationale}\"\n"));
        }
    }
    out
}

/// Schema-2 typed targets round-trip for both kinds, a schema-1
/// legacy record loads as the identical requirement target, and
/// mixed schema-1/schema-2 layouts load deterministically
/// (TEST-188).
#[test]
fn typed_target_records_round_trip_and_mixed_layouts_load() {
    let legacy = review_file(&[approve(REV_1, "REV-001")]);
    let typed = typed_review_file(&[approve_typed(REV_2, "REV-002", "requirement", REQ_A)]);
    let graph = load_corpus(&[("legacy", legacy.as_str()), ("typed", typed.as_str())])
        .expect("mixed schema layouts load");

    let legacy_review = expect_review(&graph, REV_1);
    let typed_review = expect_review(&graph, REV_2);
    assert_eq!(
        legacy_review.target,
        ReviewTarget::Requirement(REQ_A.to_string()),
        "a schema-1 record loads as a requirement target"
    );
    assert_eq!(
        typed_review.target,
        ReviewTarget::Requirement(REQ_A.to_string()),
        "a typed requirement target resolves to the same value"
    );
    for field in [legacy_review.content_schema, typed_review.content_schema] {
        assert_eq!(field, 1);
    }
    assert_eq!(
        legacy_review.reviewed_content_sha256, typed_review.reviewed_content_sha256,
        "legacy and typed records over the same content digest identically"
    );
    assert_eq!(legacy_review.decision, typed_review.decision);
    assert_eq!(legacy_review.reviewer, typed_review.reviewer);
    assert_eq!(legacy_review.reviewed_at, typed_review.reviewed_at);
    assert_eq!(
        legacy_review.edges,
        vec![(EdgeKind::Reviews, REQ_A.to_string())]
    );
    assert_eq!(legacy_review.edges, typed_review.edges);

    // Layout: the same records split differently across files load
    // to the identical graph.
    let split = load_corpus(&[("x/legacy", legacy.as_str()), ("y/typed", typed.as_str())])
        .expect("split layout loads");
    assert_eq!(graph, split, "file layout must not affect the loaded graph");

    // A typed curated-patch target round-trips through the record
    // loader itself; the graph-level endpoint contract (the patch
    // plane must carry the target) is enforced at `validate` —
    // covered by the review-invariant and integration tests.
    let patch_file =
        typed_review_file(&[approve_typed(REV_3, "REV-003", "curated_patch", PATCH_A)]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reviews.toml");
    write(&path, &patch_file);
    let mut patch_graph = CorpusGraph::new();
    super::load_reviews_into(&path, &mut patch_graph).expect("patch review record loads");
    let patch_review = expect_review(&patch_graph, REV_3);
    assert_eq!(
        patch_review.target,
        ReviewTarget::CuratedPatch(PATCH_A.to_string())
    );
    assert_eq!(patch_review.target.kind(), ReviewTargetKind::CuratedPatch);
    assert_eq!(
        patch_review.edges,
        vec![(EdgeKind::Reviews, PATCH_A.to_string())]
    );
}

/// Generic target kinds, cross-kind uids, mixed or missing target
/// shapes, and unknown target-table fields fail closed (TEST-188).
#[test]
fn target_shape_and_kind_violations_fail_closed() {
    let valid_typed = typed_review_file(&[approve_typed(REV_1, "REV-001", "requirement", REQ_A)]);
    let generic_kind = valid_typed.replace("kind = \"requirement\"", "kind = \"other\"");
    let unknown_target_field = valid_typed.replace("uid = \"req_", "extra = true, uid = \"req_");
    let cross_kind_req = valid_typed.replace(REQ_A, PATCH_A);
    let cross_kind_patch =
        typed_review_file(&[approve_typed(REV_1, "REV-001", "curated_patch", REQ_A)]);
    let non_uuid_target = valid_typed.replace(REQ_A, "req_not-a-uuid");
    let legacy_with_target = review_file(&[approve(REV_1, "REV-001")]).replace(
        "requirement_uid",
        "target = { kind = \"requirement\", uid = \"req_00000000-0000-4000-8000-00000000000a\" }\nrequirement_uid",
    );
    let legacy_missing_target = review_file(&[approve(REV_1, "REV-001")]).replace(
        "requirement_uid = \"req_00000000-0000-4000-8000-00000000000a\"\n",
        "",
    );
    let typed_with_legacy_field = valid_typed.replace(
        "content_schema",
        "requirement_uid = \"req_00000000-0000-4000-8000-00000000000a\"\ncontent_schema",
    );
    let typed_missing_target = valid_typed.replace(
        "target = { kind = \"requirement\", uid = \"req_00000000-0000-4000-8000-00000000000a\" }\n",
        "",
    );

    type Case = (&'static str, String, fn(&ReviewError) -> bool);
    let cases: [Case; 9] = [
        ("generic target kind", generic_kind, |err| {
            matches!(err, ReviewError::RecordParse { .. })
        }),
        ("unknown target-table field", unknown_target_field, |err| {
            matches!(err, ReviewError::RecordParse { .. })
        }),
        ("requirement kind with patch uid", cross_kind_req, |err| {
            matches!(
                err,
                ReviewError::NativeUidPrefix {
                    expected: "req_",
                    ..
                }
            )
        }),
        (
            "curated-patch kind with requirement uid",
            cross_kind_patch,
            |err| {
                matches!(
                    err,
                    ReviewError::NativeUidPrefix {
                        expected: "patch_",
                        ..
                    }
                )
            },
        ),
        ("non-uuid target uid", non_uuid_target, |err| {
            matches!(err, ReviewError::NativeUidUuidV4 { .. })
        }),
        (
            "typed target in a schema-1 record",
            legacy_with_target,
            |err| {
                matches!(
                    err,
                    ReviewError::ReviewTargetShape {
                        schema_version: 1,
                        ..
                    }
                )
            },
        ),
        (
            "schema-1 record without requirement_uid",
            legacy_missing_target,
            |err| {
                matches!(
                    err,
                    ReviewError::ReviewTargetShape {
                        schema_version: 1,
                        ..
                    }
                )
            },
        ),
        (
            "legacy field in a schema-2 record",
            typed_with_legacy_field,
            |err| {
                matches!(
                    err,
                    ReviewError::ReviewTargetShape {
                        schema_version: 2,
                        ..
                    }
                )
            },
        ),
        (
            "schema-2 record without target",
            typed_missing_target,
            |err| {
                matches!(
                    err,
                    ReviewError::ReviewTargetShape {
                        schema_version: 2,
                        ..
                    }
                )
            },
        ),
    ];
    for (name, content, matches_expectation) in cases {
        let err = expect_load_err(load_corpus(&[("records", content.as_str())]), name);
        assert!(
            matches_expectation(review_err(&err)),
            "{name} produced the wrong error: {err:?}"
        );
    }
}
