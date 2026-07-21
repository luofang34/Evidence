//! Tests for deterministic review-file resolution and load order in
//! the corpus index (TEST-134).

use std::path::Path;

use crate::corpus::{CorpusError, CorpusIndex, EdgeKind};

const REQ_RECORDS: &str = r#"schema_version = 1

[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000a"
id = "R-A"
layer = "hlr"
title = "reviewed requirement"
"#;

const REVIEW_1: &str = r#"schema_version = 1

[[reviews]]
uid = "rev_00000000-0000-4000-8000-0000000000a1"
id = "REV-001"
requirement_uid = "req_00000000-0000-4000-8000-00000000000a"
content_schema = 1
reviewed_content_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
decision = "approve"
reviewer = "alice@example.com"
reviewed_at = "2026-07-01T10:00:00Z"
"#;

const REVIEW_2: &str = r#"schema_version = 1

[[reviews]]
uid = "rev_00000000-0000-4000-8000-0000000000a2"
id = "REV-002"
requirement_uid = "req_00000000-0000-4000-8000-00000000000a"
content_schema = 1
reviewed_content_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
decision = "approve"
reviewer = "bob@example.com"
reviewed_at = "2026-07-01T11:00:00Z"
"#;

const CORPUS_TOML: &str =
    "schema_version = 1\nrequirements = [\"reqs/**/*.toml\"]\nreviews = [\"reviews/**/*.toml\"]\n";

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

/// Review files resolve in sorted order, an empty glob fails
/// closed, requirement errors surface before review errors, and a
/// review of an absent requirement dangles (TEST-134).
#[test]
fn index_resolves_reviews_and_loads_requirements_first() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("reqs/records.toml"), REQ_RECORDS);
    write(&dir.path().join("reviews/b.toml"), REVIEW_2);
    write(&dir.path().join("reviews/a.toml"), REVIEW_1);
    write(&dir.path().join("corpus.toml"), CORPUS_TOML);
    let index = CorpusIndex::load(&dir.path().join("corpus.toml")).unwrap();
    let names: Vec<&str> = index
        .review_files
        .iter()
        .map(|path| path.file_name().unwrap().to_str().unwrap())
        .collect();
    assert_eq!(names, ["a.toml", "b.toml"], "resolution order is sorted");
    assert_eq!(index.requirement_files.len(), 1);
    let graph = CorpusIndex::load_graph(&dir.path().join("corpus.toml")).unwrap();
    assert_eq!(graph.len(), 3, "1 requirement + 2 reviews");

    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("reqs/bad.toml"), "schema_version = 999\n");
    write(
        &dir.path().join("reviews/bad.toml"),
        "schema_version = 999\n",
    );
    write(&dir.path().join("corpus.toml"), CORPUS_TOML);
    let err = CorpusIndex::load_graph(&dir.path().join("corpus.toml")).unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::RecordSchemaTooNew { ref path, .. }
                if path.starts_with(dir.path().join("reqs"))
        ),
        "requirement errors must surface before review errors, got: {err:?}"
    );

    let orphan = REVIEW_1.replace(
        "req_00000000-0000-4000-8000-00000000000a",
        "req_00000000-0000-4000-8000-000000000099",
    );
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("reqs/records.toml"), REQ_RECORDS);
    write(&dir.path().join("reviews/orphan.toml"), &orphan);
    write(&dir.path().join("corpus.toml"), CORPUS_TOML);
    let err = CorpusIndex::load_graph(&dir.path().join("corpus.toml")).unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::DanglingEdge {
                kind: EdgeKind::Reviews,
                ..
            }
        ),
        "a review of an absent requirement must dangle, got: {err:?}"
    );

    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("reqs/records.toml"), REQ_RECORDS);
    write(&dir.path().join("corpus.toml"), CORPUS_TOML);
    let err = CorpusIndex::load(&dir.path().join("corpus.toml")).unwrap_err();
    assert!(
        matches!(err, CorpusError::EmptyIndexEntry { .. }),
        "an empty review glob must fail closed, got: {err:?}"
    );
}
