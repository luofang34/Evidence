//! Tests for deterministic review-file resolution and load order in
//! the corpus index (TEST-134), and for source-kind activation,
//! source-first load order, and the still-unsupported source_graphs
//! kind (TEST-143).

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

const SOURCE_1: &str = r#"schema_version = 1

[[sources]]
uid = "src_00000000-0000-4000-8000-0000000000a1"
id = "SRC-1"
document_key = "DOC-1"
title = "spec rev C"
media_type = "application/pdf"
canonical_location = "https://example.org/specs/DOC-1/rev-c"

[sources.material]
state = "available"
retrieved_at = "2026-07-01T10:00:00Z"
sha256 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

[sources.material.capture]
mode = "hash_only"
"#;

const SOURCES_CORPUS_TOML: &str = "schema_version = 1\nsources = [\"sources/**/*.toml\"]\nrequirements = [\"reqs/**/*.toml\"]\nreviews = [\"reviews/**/*.toml\"]\n";

/// Source files resolve in sorted order and load before
/// requirements and reviews: a source error surfaces ahead of a
/// requirement error, which surfaces ahead of a review error
/// (TEST-143).
#[test]
fn index_resolves_sources_and_loads_them_before_requirements() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("sources/b.toml"), SOURCE_1);
    write(
        &dir.path().join("sources/a.toml"),
        &SOURCE_1
            .replace(
                "src_00000000-0000-4000-8000-0000000000a1",
                "src_00000000-0000-4000-8000-0000000000a2",
            )
            .replace("SRC-1", "SRC-2"),
    );
    write(&dir.path().join("reqs/records.toml"), REQ_RECORDS);
    write(&dir.path().join("reviews/a.toml"), REVIEW_1);
    write(&dir.path().join("reviews/b.toml"), REVIEW_2);
    write(&dir.path().join("corpus.toml"), SOURCES_CORPUS_TOML);
    let index = CorpusIndex::load(&dir.path().join("corpus.toml")).unwrap();
    let names: Vec<&str> = index
        .source_files
        .iter()
        .map(|path| path.file_name().unwrap().to_str().unwrap())
        .collect();
    assert_eq!(names, ["a.toml", "b.toml"], "resolution order is sorted");
    assert_eq!(index.requirement_files.len(), 1);
    assert_eq!(index.review_files.len(), 2);
    let graph = CorpusIndex::load_graph(&dir.path().join("corpus.toml")).unwrap();
    assert_eq!(graph.len(), 5, "2 sources + 1 requirement + 2 reviews");

    // A source failure surfaces ahead of requirement and review
    // failures, proving load order.
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("sources/bad.toml"),
        "schema_version = 999\n",
    );
    write(&dir.path().join("reqs/bad.toml"), "schema_version = 999\n");
    write(
        &dir.path().join("reviews/bad.toml"),
        "schema_version = 999\n",
    );
    write(&dir.path().join("corpus.toml"), SOURCES_CORPUS_TOML);
    let err = CorpusIndex::load_graph(&dir.path().join("corpus.toml")).unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::Source(crate::corpus::SourceError::RecordSchemaTooNew {
                ref path,
                found: 999,
                ..
            }) if path.starts_with(dir.path().join("sources"))
        ),
        "source errors must surface before requirement and review errors, got: {err:?}"
    );
}

/// `source_graphs` stays an unsupported kind after `sources` is
/// activated (TEST-143).
#[test]
fn index_source_graphs_kind_stays_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nsource_graphs = [\"graphs/x.toml\"]\n",
    );
    let err = CorpusIndex::load(&dir.path().join("corpus.toml")).unwrap_err();
    assert!(
        matches!(
            err,
            CorpusError::UnsupportedKind {
                kind: "source_graphs"
            }
        ),
        "source_graphs must stay unsupported, got: {err:?}"
    );
}

/// A `sources` entry resolving to no files fails closed, like every
/// other kind (TEST-143).
#[test]
fn index_sources_entry_resolving_to_nothing_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("empty")).unwrap();
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"empty/**/*.toml\"]\n",
    );
    let err = CorpusIndex::load(&dir.path().join("corpus.toml")).unwrap_err();
    assert!(
        matches!(err, CorpusError::EmptyIndexEntry { .. }),
        "a sources entry resolving to nothing must fail closed, got: {err:?}"
    );
}
