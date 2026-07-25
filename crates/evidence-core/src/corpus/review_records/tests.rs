//! Tests for strict review record loading: round-trip, fail-closed
//! schema and field validation, and layout independence
//! (TEST-133, TEST-134).

use std::path::Path;

use crate::corpus::{
    CorpusError, CorpusGraph, CorpusIndex, EdgeKind, Node, NodeKind, ReviewDecision, ReviewError,
    ReviewNode, ReviewTarget,
};

pub(super) const REQ_A: &str = "req_00000000-0000-4000-8000-00000000000a";
pub(super) const REV_1: &str = "rev_00000000-0000-4000-8000-0000000000a1";
pub(super) const REV_2: &str = "rev_00000000-0000-4000-8000-0000000000a2";
pub(super) const REV_3: &str = "rev_00000000-0000-4000-8000-0000000000a3";
pub(super) const REV_4: &str = "rev_00000000-0000-4000-8000-0000000000a4";

const REQ_RECORDS: &str = r#"
[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000a"
id = "R-A"
layer = "hlr"
title = "reviewed requirement"

[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000b"
id = "R-B"
layer = "llr"
title = "another requirement"
derives_from = ["req_00000000-0000-4000-8000-00000000000a"]
"#;

/// One review record's field values, rendered by [`record_toml`].
#[derive(Clone)]
pub(super) struct RecordSpec {
    uid: String,
    id: String,
    requirement_uid: String,
    content_schema: u32,
    digest: String,
    decision: String,
    reviewer: String,
    reviewed_at: String,
    rationale: Option<String>,
    supersedes: Option<String>,
}

pub(super) fn approve(uid: &str, id: &str) -> RecordSpec {
    RecordSpec {
        uid: uid.to_string(),
        id: id.to_string(),
        requirement_uid: REQ_A.to_string(),
        content_schema: 1,
        digest: "a".repeat(64),
        decision: "approve".to_string(),
        reviewer: "alice@example.com".to_string(),
        reviewed_at: "2026-07-01T10:00:00Z".to_string(),
        rationale: None,
        supersedes: None,
    }
}

pub(super) fn record_toml(spec: &RecordSpec) -> String {
    let mut out = format!(
        "\n[[reviews]]\nuid = \"{}\"\nid = \"{}\"\nrequirement_uid = \"{}\"\ncontent_schema = {}\n\
         reviewed_content_sha256 = \"{}\"\ndecision = \"{}\"\nreviewer = \"{}\"\nreviewed_at = \"{}\"\n",
        spec.uid,
        spec.id,
        spec.requirement_uid,
        spec.content_schema,
        spec.digest,
        spec.decision,
        spec.reviewer,
        spec.reviewed_at,
    );
    if let Some(rationale) = &spec.rationale {
        out.push_str(&format!("rationale = \"{rationale}\"\n"));
    }
    if let Some(supersedes) = &spec.supersedes {
        out.push_str(&format!("supersedes_review_uid = \"{supersedes}\"\n"));
    }
    out
}

pub(super) fn review_file(specs: &[RecordSpec]) -> String {
    let mut out = "schema_version = 1\n".to_string();
    for spec in specs {
        out.push_str(&record_toml(spec));
    }
    out
}

pub(super) fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

/// Load a corpus with the standard requirements plus review files
/// given as `(path under reviews/, content)` pairs.
pub(super) fn load_corpus(review_files: &[(&str, &str)]) -> Result<CorpusGraph, CorpusError> {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("reqs/records.toml"),
        &format!("schema_version = 1\n{REQ_RECORDS}"),
    );
    for (name, content) in review_files {
        write(&dir.path().join(format!("reviews/{name}.toml")), content);
    }
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"reqs/**/*.toml\"]\nreviews = [\"reviews/**/*.toml\"]\n",
    );
    CorpusIndex::load_graph(&dir.path().join("corpus.toml"))
}

pub(super) fn load_with_reviews(specs: &[RecordSpec]) -> Result<CorpusGraph, CorpusError> {
    let file = review_file(specs);
    load_corpus(&[("records", file.as_str())])
}

pub(super) fn expect_load_err(result: Result<CorpusGraph, CorpusError>, case: &str) -> CorpusError {
    match result {
        Ok(_) => panic!("{case} must fail closed"),
        Err(err) => err,
    }
}

pub(super) fn expect_review<'g>(graph: &'g CorpusGraph, uid: &str) -> &'g ReviewNode {
    match graph.get(uid) {
        Some(Node::Review(node)) => node,
        other => panic!("review node {uid} missing or wrong kind: {other:?}"),
    }
}

/// Unwrap the review error a failed review load must surface.
pub(super) fn review_err(err: &CorpusError) -> &ReviewError {
    match err {
        CorpusError::Review(review_err) => review_err,
        other => panic!("expected a review error, got: {other:?}"),
    }
}

/// Valid approve and reject records become typed nodes and edges; a
/// superseding correction validates (TEST-133).
#[test]
fn approve_and_reject_records_round_trip() {
    let mut rejection = approve(REV_2, "REV-002");
    rejection.decision = "reject".to_string();
    rejection.rationale = Some("the rationale field is not covered".to_string());
    let mut correction = approve(REV_3, "REV-003");
    correction.decision = "reject".to_string();
    correction.rationale = Some("correcting my earlier approval".to_string());
    correction.reviewed_at = "2026-07-02T11:30:00+02:00".to_string();
    correction.supersedes = Some(REV_1.to_string());

    let graph = load_with_reviews(&[approve(REV_1, "REV-001"), rejection, correction])
        .expect("valid review records load");
    assert_eq!(graph.len(), 5, "2 requirements + 3 reviews");

    let approval = expect_review(&graph, REV_1);
    assert_eq!(approval.id, "REV-001");
    assert_eq!(
        approval.target,
        ReviewTarget::Requirement(REQ_A.to_string())
    );
    assert_eq!(approval.content_schema, 1);
    assert_eq!(approval.reviewed_content_sha256.as_str(), "a".repeat(64));
    assert_eq!(approval.decision, ReviewDecision::Approve);
    assert_eq!(approval.reviewer, "alice@example.com");
    assert_eq!(approval.reviewed_at, "2026-07-01T10:00:00Z");
    assert_eq!(approval.rationale, None);
    assert_eq!(approval.edges, vec![(EdgeKind::Reviews, REQ_A.to_string())]);

    let rejected = expect_review(&graph, REV_2);
    assert_eq!(rejected.decision, ReviewDecision::Reject);
    assert_eq!(
        rejected.rationale.as_deref(),
        Some("the rationale field is not covered")
    );

    let correction = expect_review(&graph, REV_3);
    assert_eq!(
        correction.edges,
        vec![
            (EdgeKind::Reviews, REQ_A.to_string()),
            (EdgeKind::Supersedes, REV_1.to_string()),
        ],
        "edges canonicalize to sorted order"
    );
    assert_eq!(correction.reviewed_at, "2026-07-02T11:30:00+02:00");
    graph
        .validate()
        .expect("a valid supersession chain validates");

    let duplicate_id = expect_load_err(
        load_with_reviews(&[approve(REV_1, "REV-001"), approve(REV_4, "REV-001")]),
        "duplicate review human id",
    );
    assert!(
        matches!(
            review_err(&duplicate_id),
            ReviewError::DuplicateHumanId {
                kind: NodeKind::Review,
                ..
            }
        ),
        "duplicate review human ids must collide within the review kind: {duplicate_id:?}"
    );
}

/// Unknown fields and newer file/content schemas refuse to load;
/// errors name the file path and record identity (TEST-133).
#[test]
fn strict_schema_violations_fail_closed() {
    let unknown_field = review_file(&[approve(REV_1, "REV-001")])
        .replace("reviewed_at", "surprise_field = true\nreviewed_at");
    let unknown_top_level = "schema_version = 1\nsurprise = true\n".to_string();
    let newer_file_schema = "schema_version = 3\n".to_string();
    let mut newer_content = approve(REV_1, "REV-001");
    newer_content.content_schema = 2;
    let mut zero_content = approve(REV_1, "REV-001");
    zero_content.content_schema = 0;

    type SchemaCase = (&'static str, String, fn(&ReviewError) -> bool);
    let cases: [SchemaCase; 5] = [
        ("unknown record field", unknown_field, |err| {
            matches!(err, ReviewError::RecordParse { .. })
        }),
        ("unknown top-level field", unknown_top_level, |err| {
            matches!(err, ReviewError::RecordParse { .. })
        }),
        ("newer file schema", newer_file_schema, |err| {
            matches!(
                err,
                ReviewError::RecordSchemaTooNew {
                    found: 3,
                    supported: 2,
                    ..
                }
            )
        }),
        (
            "newer content schema",
            review_file(&[newer_content]),
            |err| {
                matches!(
                    err,
                    ReviewError::ReviewContentSchema {
                        found: 2,
                        supported: 1,
                        ..
                    }
                )
            },
        ),
        ("zero content schema", review_file(&[zero_content]), |err| {
            matches!(
                err,
                ReviewError::ReviewContentSchema {
                    found: 0,
                    supported: 1,
                    ..
                }
            )
        }),
    ];
    for (name, content, matches_expectation) in cases {
        let err = expect_load_err(load_corpus(&[("records", content.as_str())]), name);
        assert!(
            matches_expectation(review_err(&err)),
            "{name} produced the wrong error: {err:?}"
        );
    }

    let mut bad_schema = approve(REV_1, "REV-001");
    bad_schema.content_schema = 2;
    let err = expect_load_err(load_with_reviews(&[bad_schema]), "content schema context");
    let message = err.to_string();
    assert!(
        message.contains("reviews") && message.contains(REV_1) && message.contains("REV-001"),
        "review errors must name the file path and the record identity: {message}"
    );
}

/// Malformed uids, digests, timestamps, reviewer identities, human
/// ids, rationales, and supersession pointers fail closed (TEST-133).
#[test]
fn malformed_record_fields_fail_closed() {
    type Case = (&'static str, fn(&mut RecordSpec), fn(&ReviewError) -> bool);
    let cases: [Case; 14] = [
        (
            "review uid without prefix",
            |spec| {
                spec.uid = "00000000-0000-4000-8000-0000000000a1".to_string();
            },
            |err| {
                matches!(
                    err,
                    ReviewError::NativeUidPrefix {
                        expected: "rev_",
                        ..
                    }
                )
            },
        ),
        (
            "review uid not v4",
            |spec| {
                spec.uid = "rev_00000000-0000-1000-8000-0000000000a1".to_string();
            },
            |err| matches!(err, ReviewError::NativeUidUuidV4 { .. }),
        ),
        (
            "review uid not RFC 4122",
            |spec| {
                spec.uid = "rev_00000000-0000-4000-c000-0000000000a1".to_string();
            },
            |err| matches!(err, ReviewError::NativeUidUuidV4 { .. }),
        ),
        (
            "requirement uid without prefix",
            |spec| {
                spec.requirement_uid = REV_1.to_string();
            },
            |err| {
                matches!(
                    err,
                    ReviewError::NativeUidPrefix {
                        expected: "req_",
                        ..
                    }
                )
            },
        ),
        (
            "uppercase digest",
            |spec| spec.digest = "A".repeat(64),
            |err| matches!(err, ReviewError::RecordParse { .. }),
        ),
        (
            "short digest",
            |spec| spec.digest = "a".repeat(63),
            |err| matches!(err, ReviewError::RecordParse { .. }),
        ),
        (
            "garbage timestamp",
            |spec| {
                spec.reviewed_at = "yesterday".to_string();
            },
            |err| matches!(err, ReviewError::ReviewTimestamp { value, .. } if value == "yesterday"),
        ),
        (
            "date-only timestamp",
            |spec| {
                spec.reviewed_at = "2026-07-01".to_string();
            },
            |err| matches!(err, ReviewError::ReviewTimestamp { .. }),
        ),
        (
            "whitespace reviewer",
            |spec| {
                spec.reviewer = "   ".to_string();
            },
            |err| matches!(err, ReviewError::ReviewReviewer { .. }),
        ),
        (
            "empty human id",
            |spec| spec.id = String::new(),
            |err| matches!(err, ReviewError::ReviewHumanId { .. }),
        ),
        (
            "reject without rationale",
            |spec| {
                spec.decision = "reject".to_string();
            },
            |err| matches!(err, ReviewError::ReviewRationale { .. }),
        ),
        (
            "reject with whitespace rationale",
            |spec| {
                spec.decision = "reject".to_string();
                spec.rationale = Some("  ".to_string());
            },
            |err| matches!(err, ReviewError::ReviewRationale { .. }),
        ),
        (
            "supersedes uid not a uuid",
            |spec| {
                spec.supersedes = Some("rev_not-a-uuid".to_string());
            },
            |err| matches!(err, ReviewError::NativeUidUuidV4 { .. }),
        ),
        (
            "supersedes uid wrong prefix",
            |spec| {
                spec.supersedes = Some(REQ_A.to_string());
            },
            |err| {
                matches!(
                    err,
                    ReviewError::NativeUidPrefix {
                        expected: "rev_",
                        ..
                    }
                )
            },
        ),
    ];
    for (name, mutate, matches_expectation) in cases {
        let mut spec = approve(REV_1, "REV-001");
        mutate(&mut spec);
        let err = expect_load_err(load_with_reviews(&[spec]), name);
        assert!(
            matches_expectation(review_err(&err)),
            "{name} produced the wrong error: {err:?}"
        );
    }
}

/// Equivalent review sets split and reordered across files produce
/// equal graphs (TEST-134).
#[test]
fn layout_and_record_order_produce_identical_graphs() {
    let set = || {
        let mut rejection = approve(REV_2, "REV-002");
        rejection.decision = "reject".to_string();
        rejection.reviewer = "bob@example.com".to_string();
        rejection.rationale = Some("needs work".to_string());
        let mut correction = approve(REV_3, "REV-003");
        correction.decision = "reject".to_string();
        correction.rationale = Some("re-reading changed my assessment".to_string());
        correction.supersedes = Some(REV_1.to_string());
        let mut independent = approve(REV_4, "REV-004");
        independent.reviewer = "carol@example.com".to_string();
        vec![
            approve(REV_1, "REV-001"),
            rejection,
            correction,
            independent,
        ]
    };
    let single =
        load_corpus(&[("all", review_file(&set()).as_str())]).expect("single layout loads");

    let specs = set();
    let one = review_file(&[specs[3].clone()]);
    let two = review_file(&[specs[2].clone(), specs[1].clone()]);
    let three = review_file(&[specs[0].clone()]);
    let split = load_corpus(&[
        ("x/one", one.as_str()),
        ("x/two", two.as_str()),
        ("y/three", three.as_str()),
    ])
    .expect("split layout loads");

    assert_eq!(
        single, split,
        "file layout and record order must not affect the loaded graph"
    );
    assert_eq!(single.len(), 6, "2 requirements + 4 reviews");
}

/// Reviews of one digest by different reviewers are all preserved
/// (TEST-134).
#[test]
fn independent_reviews_of_one_digest_are_preserved() {
    let mut second = approve(REV_2, "REV-002");
    second.reviewer = "bob@example.com".to_string();
    let graph =
        load_with_reviews(&[approve(REV_1, "REV-001"), second]).expect("independent reviews load");
    assert_eq!(graph.len(), 4, "2 requirements + 2 reviews");
    for uid in [REV_1, REV_2] {
        let review = expect_review(&graph, uid);
        assert_eq!(review.target, ReviewTarget::Requirement(REQ_A.to_string()));
        assert_eq!(review.reviewed_content_sha256.as_str(), "a".repeat(64));
        assert_eq!(review.edges, vec![(EdgeKind::Reviews, REQ_A.to_string())]);
    }
    assert_eq!(expect_review(&graph, REV_1).reviewer, "alice@example.com");
    assert_eq!(expect_review(&graph, REV_2).reviewer, "bob@example.com");
}
