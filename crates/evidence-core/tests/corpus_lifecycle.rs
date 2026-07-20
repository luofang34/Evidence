//! Lifecycle evaluation end-to-end through the corpus index:
//! equivalent corpora laid out as one review file or three
//! reordered files evaluate identically (TEST-136).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::BTreeMap;
use std::path::Path;

use evidence_core::corpus::{
    CorpusIndex, LifecycleEvaluation, RequirementLifecycle, evaluate_all_lifecycles,
    review_content_digest_v1,
};

const REQ_A: &str = "req_00000000-0000-4000-8000-00000000000a";
const REQ_B: &str = "req_00000000-0000-4000-8000-00000000000b";
const REV_A1: &str = "rev_00000000-0000-4000-8000-0000000000a1";
const REV_A2: &str = "rev_00000000-0000-4000-8000-0000000000a2";
const REV_B1: &str = "rev_00000000-0000-4000-8000-0000000000b1";

const REQ_RECORDS: &str = r#"
[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000a"
id = "R-A"
layer = "hlr"
title = "first reviewed requirement"
description = "normative prose of R-A"

[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000b"
id = "R-B"
layer = "llr"
title = "second reviewed requirement"
description = "normative prose of R-B"
derives_from = ["req_00000000-0000-4000-8000-00000000000a"]
"#;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

/// Load the requirements-only corpus in `dir` to learn the exact
/// current digest of both requirements.
fn current_digests(dir: &Path) -> (String, String) {
    write(
        &dir.join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"reqs/**/*.toml\"]\n",
    );
    let graph = CorpusIndex::load_graph(&dir.join("corpus.toml")).expect("requirements load");
    let digest = |uid: &str| {
        review_content_digest_v1(
            &graph
                .review_content(uid)
                .expect("requirement projects content"),
        )
        .as_str()
        .to_string()
    };
    (digest(REQ_A), digest(REQ_B))
}

#[allow(clippy::too_many_arguments)]
fn review_record(
    uid: &str,
    id: &str,
    requirement_uid: &str,
    digest: &str,
    decision: &str,
    reviewer: &str,
    rationale: Option<&str>,
) -> String {
    let mut out = format!(
        "\n[[reviews]]\nuid = \"{uid}\"\nid = \"{id}\"\nrequirement_uid = \"{requirement_uid}\"\n\
         content_schema = 1\nreviewed_content_sha256 = \"{digest}\"\ndecision = \"{decision}\"\n\
         reviewer = \"{reviewer}\"\nreviewed_at = \"2026-07-01T10:00:00Z\"\n"
    );
    if let Some(rationale) = rationale {
        out.push_str(&format!("rationale = \"{rationale}\"\n"));
    }
    out
}

/// Load one corpus layout end-to-end through the index and evaluate
/// every requirement. `single_file` selects one review file;
/// otherwise the same records split, reordered, across three files.
fn layout_evaluations(single_file: bool) -> BTreeMap<String, LifecycleEvaluation> {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("reqs/records.toml"),
        &format!("schema_version = 1\n{REQ_RECORDS}"),
    );
    let (digest_a, digest_b) = current_digests(dir.path());

    let approve_a = review_record(
        REV_A1,
        "REV-001",
        REQ_A,
        &digest_a,
        "approve",
        "alice@example.com",
        None,
    );
    let reject_a = review_record(
        REV_A2,
        "REV-002",
        REQ_A,
        &digest_a,
        "reject",
        "bob@example.com",
        Some("the rationale field is thin"),
    );
    let approve_b = review_record(
        REV_B1,
        "REV-003",
        REQ_B,
        &digest_b,
        "approve",
        "carol@example.com",
        None,
    );

    if single_file {
        write(
            &dir.path().join("reviews/records.toml"),
            &format!("schema_version = 1\n{approve_a}{reject_a}{approve_b}"),
        );
    } else {
        write(
            &dir.path().join("reviews/x/one.toml"),
            &format!("schema_version = 1\n{approve_b}"),
        );
        write(
            &dir.path().join("reviews/x/two.toml"),
            &format!("schema_version = 1\n{reject_a}"),
        );
        write(
            &dir.path().join("reviews/y/three.toml"),
            &format!("schema_version = 1\n{approve_a}"),
        );
    }
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"reqs/**/*.toml\"]\nreviews = [\"reviews/**/*.toml\"]\n",
    );
    let graph = CorpusIndex::load_graph(&dir.path().join("corpus.toml")).expect("layout loads");
    evaluate_all_lifecycles(&graph).expect("evaluations succeed")
}

/// Layout A (one review file) and layout B (three files, records
/// reordered across directories) produce identical evaluations
/// (TEST-136).
#[test]
fn equivalent_file_layouts_evaluate_identically() {
    let one_file = layout_evaluations(true);
    let three_files = layout_evaluations(false);
    assert_eq!(
        one_file, three_files,
        "file layout and record order must not change any evaluation"
    );
    assert_eq!(one_file.len(), 2);
    assert_eq!(
        one_file[REQ_A].state,
        RequirementLifecycle::Rejected,
        "the current-digest rejection takes precedence across files"
    );
    assert_eq!(
        one_file[REQ_A].effective_review_uids,
        vec![REV_A1.to_string(), REV_A2.to_string()]
    );
    assert_eq!(one_file[REQ_B].state, RequirementLifecycle::Approved);
    assert_eq!(
        one_file[REQ_B].effective_review_uids,
        vec![REV_B1.to_string()]
    );
}
