//! Typed review target acceptance tests (TEST-188, TEST-189,
//! TEST-191): the committed schema migration fixture proves the
//! exact wire change from legacy `requirement_uid` records to
//! typed `target = { kind, uid }` records, generic targets and
//! cross-kind edges fail closed through the public loader, and the
//! approval-gated effective graph changes only under a currently
//! approved patch while requirement approval outcomes stay
//! identical.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::path::{Path, PathBuf};

use evidence_core::corpus::{
    CorpusError, CorpusGraph, CorpusIndex, EdgeKind, InsertedNodeSpec, LifecycleEnforcement, Node,
    PatchBindings, PatchOperation, RequirementLifecycle, ReviewDecision, ReviewError, ReviewTarget,
    SafeRelPath, SourceGraph, SourceLocator, SourceNodeKind, SourcePatchRecord,
    StructuralContentDigest, effective_source_graph, evaluate_lifecycle, reviewed_content_digest,
    source_graph_digest, validate_approval_boundary,
};

const REQ_A: &str = "req_00000000-0000-4000-8000-00000000000a";
const REVISION: &str = "src_00000000-0000-4000-8000-0000000000a1";
const PATCH_A: &str = "patch_00000000-0000-4000-8000-0000000000b1";
const INSERTED: &str = "snode_00000000-0000-4000-8000-0000000000c1";
const REV_1: &str = "rev_00000000-0000-4000-8000-0000000000a1";
const REV_2: &str = "rev_00000000-0000-4000-8000-0000000000a2";
const RECIPE_HEX: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const INPUT_HEX: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const MEDIA: &str = "text/markdown";

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus/review_target_migration")
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(path, content).expect("write");
}

fn review_err(err: &CorpusError) -> &ReviewError {
    match err {
        CorpusError::Review(review_err) => review_err,
        other => panic!("expected a review error, got: {other:?}"),
    }
}

/// The committed migration fixture proves the exact wire change:
/// the schema-1 legacy record and the schema-2 typed record load
/// as the same requirement target with identical digests,
/// decisions, reviewer metadata, and lifecycle contribution
/// (TEST-188).
#[test]
fn schema_migration_fixture_proves_identical_requirement_review_semantics() {
    let graph = CorpusIndex::load_graph(&fixture_dir().join("corpus.toml"))
        .expect("the migration fixture corpus loads");
    let review = |uid: &str| match graph.get(uid) {
        Some(Node::Review(review)) => review,
        other => panic!("review {uid} missing: {other:?}"),
    };
    let legacy = review(REV_1);
    let typed = review(REV_2);
    assert_eq!(
        legacy.target,
        ReviewTarget::Requirement(REQ_A.to_string()),
        "the legacy requirement_uid record loads as a requirement target"
    );
    assert_eq!(legacy.target, typed.target);
    assert_eq!(legacy.content_schema, typed.content_schema);
    assert_eq!(
        legacy.reviewed_content_sha256, typed.reviewed_content_sha256,
        "the wire change never touches the reviewed-content digest"
    );
    assert_eq!(legacy.decision, typed.decision);
    assert_eq!(legacy.reviewer, typed.reviewer);
    assert_eq!(legacy.reviewed_at, typed.reviewed_at);
    assert_eq!(legacy.rationale, typed.rationale);
    assert_eq!(
        legacy.edges,
        vec![(EdgeKind::Reviews, REQ_A.to_string())],
        "the legacy record's edges are unchanged"
    );
    assert_eq!(legacy.edges, typed.edges);

    // Both records feed the requirement lifecycle identically.
    let evaluation = evaluate_lifecycle(&graph, REQ_A).expect("the fixture requirement evaluates");
    assert_eq!(
        evaluation.effective_review_uids,
        vec![REV_1.to_string(), REV_2.to_string()],
        "legacy and typed heads compose in uid order"
    );

    // The wire change is exactly the target shape: the legacy file
    // carries `requirement_uid`, the typed file a closed
    // `target = { kind, uid }` table under schema_version 2 — a
    // schema older tools (supporting only version 1) refuse through
    // the schema-version gate.
    let legacy_raw = std::fs::read_to_string(fixture_dir().join("reviews_v1/records.toml"))
        .expect("legacy fixture");
    let typed_raw = std::fs::read_to_string(fixture_dir().join("reviews_v2/records.toml"))
        .expect("typed fixture");
    assert!(legacy_raw.contains("schema_version = 1"));
    assert!(legacy_raw.contains("requirement_uid = \"req_"));
    assert!(!legacy_raw.contains("target = {"));
    assert!(typed_raw.contains("schema_version = 2"));
    assert!(typed_raw.contains("target = { kind = \"requirement\""));
    assert!(!typed_raw.contains("requirement_uid"));
}

/// Write a temp corpus with one requirement and the given review
/// files, returning the load result.
fn load_review_corpus(review_files: &[(&str, &str)]) -> Result<CorpusGraph, CorpusError> {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        &dir.path().join("reqs/records.toml"),
        &std::fs::read_to_string(fixture_dir().join("reqs/records.toml")).expect("reqs"),
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

const TYPED_APPROVAL: &str = r#"
[[reviews]]
uid = "rev_00000000-0000-4000-8000-0000000000a1"
id = "REV-001"
target = { kind = "requirement", uid = "req_00000000-0000-4000-8000-00000000000a" }
content_schema = 1
reviewed_content_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
decision = "approve"
reviewer = "alice@example.com"
reviewed_at = "2026-07-01T10:00:00Z"
"#;

/// Generic target kinds, cross-kind uids, and cross-kind edges
/// fail closed through the public loader (TEST-189).
#[test]
fn generic_targets_and_cross_kind_edges_fail_closed() {
    let generic_kind = format!(
        "schema_version = 2\n{}",
        TYPED_APPROVAL.replace("kind = \"requirement\"", "kind = \"requirement_v2\"")
    );
    let cross_kind_uid = format!(
        "schema_version = 2\n{}",
        TYPED_APPROVAL.replace(REQ_A, PATCH_A)
    );
    let legacy_patch_uid = format!(
        "schema_version = 1\n{}",
        TYPED_APPROVAL.replace(
            "target = { kind = \"requirement\", uid = \"req_00000000-0000-4000-8000-00000000000a\" }",
            "requirement_uid = \"patch_00000000-0000-4000-8000-0000000000b1\"",
        )
    );
    let patch_review_without_patch = format!(
        "schema_version = 2\n{}",
        TYPED_APPROVAL.replace(
            "target = { kind = \"requirement\", uid = \"req_00000000-0000-4000-8000-00000000000a\" }",
            "target = { kind = \"curated_patch\", uid = \"patch_00000000-0000-4000-8000-0000000000b1\" }",
        )
    );

    let err = load_review_corpus(&[("records", generic_kind.as_str())])
        .expect_err("a generic target kind must fail closed");
    assert!(
        matches!(review_err(&err), ReviewError::RecordParse { .. }),
        "generic kind, got: {err:?}"
    );

    let err = load_review_corpus(&[("records", cross_kind_uid.as_str())])
        .expect_err("a cross-kind uid must fail closed");
    assert!(
        matches!(
            review_err(&err),
            ReviewError::NativeUidPrefix {
                expected: "req_",
                ..
            }
        ),
        "requirement kind with patch uid, got: {err:?}"
    );

    let err = load_review_corpus(&[("records", legacy_patch_uid.as_str())])
        .expect_err("a legacy requirement_uid naming a patch must fail closed");
    assert!(
        matches!(
            review_err(&err),
            ReviewError::NativeUidPrefix {
                expected: "req_",
                ..
            }
        ),
        "legacy field with patch uid, got: {err:?}"
    );

    let err = load_review_corpus(&[("records", patch_review_without_patch.as_str())])
        .expect_err("a patch review with no committed patch must fail closed");
    assert!(
        matches!(
            err,
            CorpusError::DanglingEdge {
                kind: evidence_core::corpus::EdgeKind::Reviews,
                ..
            }
        ),
        "uncommitted patch target, got: {err:?}"
    );
}

/// The fixture patch record: one root insert over the revision's
/// empty parser graph.
fn patch_record() -> SourcePatchRecord {
    let mut record = SourcePatchRecord {
        uid: PATCH_A.to_string(),
        human_id: "PATCH-001".to_string(),
        source_revision_uid: REVISION.to_string(),
        recipe_digest: StructuralContentDigest::from_hex(RECIPE_HEX).expect("hex"),
        input_digest: StructuralContentDigest::from_hex(INPUT_HEX).expect("hex"),
        pre_patch_graph_digest: source_graph_digest(&SourceGraph::new()),
        reviewed_content_digest: StructuralContentDigest::from_hex(&"0".repeat(64)).expect("hex"),
        author: "curator@example.com".to_string(),
        rationale: "restore the intended structure".to_string(),
        created_at: "2026-07-01T10:00:00Z".to_string(),
        operations: vec![PatchOperation::Insert {
            ordinal: 0,
            expected_parent_uid: None,
            node: InsertedNodeSpec {
                uid: INSERTED.to_string(),
                kind: SourceNodeKind::Section,
                ordinal: 0,
                label: None,
                canonical_text: "curated section".to_string(),
                locator: SourceLocator::Markdown {
                    path: SafeRelPath::new("docs/doc.md").expect("safe path"),
                    git_blob: None,
                    anchor: None,
                    heading_path: Vec::new(),
                    byte_range: (0, 10),
                },
            },
        }],
    };
    record.reviewed_content_digest = reviewed_content_digest(&record);
    record
}

fn patch_toml(record: &SourcePatchRecord) -> String {
    format!(
        "schema_version = 1\n\n[patch]\nuid = \"{}\"\nhuman_id = \"{}\"\n\
         source_revision_uid = \"{}\"\nrecipe_digest = \"{}\"\ninput_digest = \"{}\"\n\
         pre_patch_graph_digest = \"{}\"\nreviewed_content_digest = \"{}\"\n\
         author = \"{}\"\nrationale = \"{}\"\ncreated_at = \"{}\"\n\
         \n[[patch.operations]]\nop = \"insert\"\nordinal = 0\n\
         node = {{ uid = \"{INSERTED}\", kind = \"section\", ordinal = 0, \
         canonical_text = \"curated section\", \
         locator = {{ format = \"markdown\", path = \"docs/doc.md\", byte_range = [0, 10] }} }}\n",
        record.uid,
        record.human_id,
        record.source_revision_uid,
        record.recipe_digest,
        record.input_digest,
        record.pre_patch_graph_digest,
        record.reviewed_content_digest,
        record.author,
        record.rationale,
        record.created_at,
    )
}

fn source_toml() -> String {
    format!(
        "schema_version = 1\n\n[[sources]]\nuid = \"{REVISION}\"\nid = \"DOC-1\"\n\
         document_key = \"doc\"\ntitle = \"fixture document\"\nmedia_type = \"{MEDIA}\"\n\
         canonical_location = \"https://example.org/doc/rev-a\"\n\n\
         [sources.material]\nstate = \"unavailable\"\nreason = \"fixture\"\n"
    )
}

/// One schema-2 review record targeting the fixture patch.
fn patch_review_toml(uid: &str, id: &str, decision: &str, digest: &str) -> String {
    let mut out = format!(
        "\n[[reviews]]\nuid = \"{uid}\"\nid = \"{id}\"\n\
         target = {{ kind = \"curated_patch\", uid = \"{PATCH_A}\" }}\n\
         content_schema = 1\nreviewed_content_sha256 = \"{digest}\"\ndecision = \"{decision}\"\n\
         reviewer = \"alice@example.com\"\nreviewed_at = \"2026-07-01T10:00:00Z\"\n"
    );
    if decision == "reject" {
        out.push_str("rationale = \"not ready\"\n");
    }
    out
}

/// Load the patch corpus with the given review records (schema-2
/// review file).
fn load_patch_corpus(review_records: &str) -> CorpusGraph {
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("sources/records.toml"), &source_toml());
    write(
        &dir.path().join("patches/p.toml"),
        &patch_toml(&patch_record()),
    );
    write(
        &dir.path().join("reviews/records.toml"),
        &format!("schema_version = 2\n{review_records}"),
    );
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources/**/*.toml\"]\n\
         source_patches = [\"patches/**/*.toml\"]\nreviews = [\"reviews/**/*.toml\"]\n",
    );
    CorpusIndex::load_graph(&dir.path().join("corpus.toml")).expect("patch corpus loads")
}

fn effective_of(graph: &CorpusGraph) -> evidence_core::corpus::EffectiveSourceGraph {
    let bindings = PatchBindings {
        recipe_digest: StructuralContentDigest::from_hex(RECIPE_HEX).expect("hex"),
        input_digest: StructuralContentDigest::from_hex(INPUT_HEX).expect("hex"),
    };
    effective_source_graph(graph, REVISION, &bindings, MEDIA).expect("effective graph computes")
}

/// Only a currently approved patch alters the effective graph;
/// candidate, rejected, and stale patches never do; layout never
/// affects the output; and the approval boundary proves the
/// effective curated content (TEST-191).
#[test]
fn approved_patch_affects_effective_graph_candidate_rejected_stale_never_do() {
    let digest = patch_record().reviewed_content_digest.as_str().to_string();

    // Candidate: no reviews.
    let graph = load_patch_corpus("");
    let effective = effective_of(&graph);
    assert!(effective.applied_patch_uids.is_empty());
    assert!(effective.graph.get(INSERTED).is_none());

    // Approved: the patch contributes, the boundary passes, and
    // the parser plane is untouched.
    let approval = patch_review_toml(REV_1, "REV-001", "approve", &digest);
    let graph = load_patch_corpus(&approval);
    let effective = effective_of(&graph);
    assert_eq!(effective.applied_patch_uids, vec![PATCH_A.to_string()]);
    assert!(
        effective.graph.get(INSERTED).is_some(),
        "the approved patch's curated node is effective"
    );
    assert!(
        graph
            .source_graph(REVISION)
            .is_none_or(|committed| committed.get(INSERTED).is_none()),
        "the committed parser graph is never mutated"
    );
    validate_approval_boundary(&graph, LifecycleEnforcement::Required)
        .expect("approved producible content passes the boundary");

    // Rejected: never contributes.
    let rejection = patch_review_toml(REV_1, "REV-001", "reject", &digest);
    let graph = load_patch_corpus(&rejection);
    let effective = effective_of(&graph);
    assert!(effective.applied_patch_uids.is_empty());
    assert!(effective.graph.get(INSERTED).is_none());

    // Stale: an older-digest approval never contributes.
    let stale = patch_review_toml(REV_1, "REV-001", "approve", &"a".repeat(64));
    let graph = load_patch_corpus(&stale);
    let effective = effective_of(&graph);
    assert!(effective.applied_patch_uids.is_empty());
    assert!(effective.graph.get(INSERTED).is_none());

    // Conflicting current decisions — one approval, one rejection
    // of the same digest — are rejected by precedence and never
    // contribute.
    let conflict = format!(
        "{}{}",
        patch_review_toml(REV_1, "REV-001", "approve", &digest),
        patch_review_toml(REV_2, "REV-002", "reject", &digest)
            .replace("alice@example.com", "bob@example.com"),
    );
    let graph = load_patch_corpus(&conflict);
    let effective = effective_of(&graph);
    assert!(effective.applied_patch_uids.is_empty());
    assert!(effective.graph.get(INSERTED).is_none());

    // Layout independence: the same approvals split across files
    // and reordered produce the identical effective graph.
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("sources/records.toml"), &source_toml());
    write(
        &dir.path().join("patches/p.toml"),
        &patch_toml(&patch_record()),
    );
    write(
        &dir.path().join("reviews/a.toml"),
        &format!(
            "schema_version = 2\n{}",
            patch_review_toml(REV_2, "REV-002", "approve", &digest)
        ),
    );
    write(
        &dir.path().join("reviews/b.toml"),
        &format!(
            "schema_version = 2\n{}",
            patch_review_toml(REV_1, "REV-001", "approve", &digest)
        ),
    );
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources/**/*.toml\"]\n\
         source_patches = [\"patches/**/*.toml\"]\nreviews = [\"reviews/**/*.toml\"]\n",
    );
    let split = CorpusIndex::load_graph(&dir.path().join("corpus.toml")).expect("split loads");
    let single_approvals = format!(
        "{}{}",
        patch_review_toml(REV_1, "REV-001", "approve", &digest),
        patch_review_toml(REV_2, "REV-002", "approve", &digest),
    );
    let single = load_patch_corpus(&single_approvals);
    assert_eq!(effective_of(&split), effective_of(&single));

    // Requirement approval outcomes are unchanged in a corpus that
    // also carries patches: a requirement approval still evaluates
    // Approved through the untouched v1 path.
    let reqs = std::fs::read_to_string(fixture_dir().join("reqs/records.toml")).expect("reqs");
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("reqs/records.toml"), &reqs);
    write(&dir.path().join("sources/records.toml"), &source_toml());
    write(
        &dir.path().join("patches/p.toml"),
        &patch_toml(&patch_record()),
    );
    write(
        &dir.path().join("reviews/records.toml"),
        "schema_version = 1\n",
    );
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"reqs/**/*.toml\"]\nsources = [\"sources/**/*.toml\"]\n\
         source_patches = [\"patches/**/*.toml\"]\nreviews = [\"reviews/**/*.toml\"]\n",
    );
    let graph = CorpusIndex::load_graph(&dir.path().join("corpus.toml")).expect("corpus loads");
    let req_digest = evidence_core::corpus::review_content_digest_v1(
        &graph.review_content(REQ_A).expect("requirement content"),
    );
    write(
        &dir.path().join("reviews/records.toml"),
        &format!(
            "schema_version = 1\n\n[[reviews]]\nuid = \"{REV_1}\"\nid = \"REV-001\"\n\
             requirement_uid = \"{REQ_A}\"\ncontent_schema = 1\n\
             reviewed_content_sha256 = \"{req_digest}\"\ndecision = \"approve\"\n\
             reviewer = \"alice@example.com\"\nreviewed_at = \"2026-07-01T10:00:00Z\"\n"
        ),
    );
    let graph = CorpusIndex::load_graph(&dir.path().join("corpus.toml")).expect("corpus reloads");
    let evaluation = evaluate_lifecycle(&graph, REQ_A).expect("requirement evaluates");
    assert_eq!(
        evaluation.state,
        RequirementLifecycle::Approved,
        "requirement approval outcomes are identical with patches present"
    );
    assert_eq!(
        graph.reviews_for_patch(PATCH_A).len(),
        0,
        "requirement reviews never leak into the patch plane"
    );
}
