//! `corpus.toml` index strictness and fail-closed resolution tests
//! (TEST-119).

use std::path::Path;

use super::super::CorpusError;
use super::super::index::CorpusIndex;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

#[test]
fn index_parses_minimal() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("reqs/base.toml"),
        r#"
schema_version = 1

[[requirements]]
uid = "req_00000000-0000-4000-8000-00000000000a"
id = "R-A"
layer = "sys"
title = "a"
"#,
    );
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"reqs/**/*.toml\"]\n",
    );

    let index = CorpusIndex::load(&dir.path().join("corpus.toml")).unwrap();
    assert_eq!(index.requirement_files.len(), 1);

    let graph = CorpusIndex::load_graph(&dir.path().join("corpus.toml")).unwrap();
    assert_eq!(graph.len(), 1);
    assert!(
        graph
            .get("req_00000000-0000-4000-8000-00000000000a")
            .is_some()
    );
}

#[test]
fn index_rejects_unknown_field() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nfrobnicate = true\n",
    );
    let err = CorpusIndex::load(&dir.path().join("corpus.toml")).unwrap_err();
    assert!(
        matches!(err, CorpusError::IndexParse { .. }),
        "unknown index field must be a parse error, got: {err:?}"
    );
}

#[test]
fn index_refuses_newer_schema() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("corpus.toml"), "schema_version = 999\n");
    let err = CorpusIndex::load(&dir.path().join("corpus.toml")).unwrap_err();
    assert!(
        matches!(err, CorpusError::IndexSchemaTooNew { found: 999, .. }),
        "newer schema must refuse to load, got: {err:?}"
    );
}

#[test]
fn index_empty_resolution_is_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("empty")).unwrap();
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"empty/**/*.toml\"]\n",
    );
    let err = CorpusIndex::load(&dir.path().join("corpus.toml")).unwrap_err();
    assert!(
        matches!(err, CorpusError::EmptyIndexEntry { .. }),
        "an entry resolving to nothing must fail closed, got: {err:?}"
    );
}

#[test]
fn index_unsupported_kind_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources/x.toml\"]\n",
    );
    let err = CorpusIndex::load(&dir.path().join("corpus.toml")).unwrap_err();
    assert!(
        matches!(err, CorpusError::UnsupportedKind { kind: "sources" }),
        "an indexed-but-unloadable kind must refuse, got: {err:?}"
    );
}
