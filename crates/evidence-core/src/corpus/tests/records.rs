//! Corpus-native record identity strictness tests (TEST-120).

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
fn records_reject_invalid_native_uid() {
    let err = native_requirement_error("00000000-0000-4000-8000-00000000000a");
    assert!(
        matches!(err, CorpusError::NativeUidPrefix { .. }),
        "native record uids must carry the req_ prefix, got: {err:?}"
    );

    let err = native_requirement_error("req_00000000-0000-1000-8000-00000000000a");
    assert!(
        matches!(err, CorpusError::NativeUidUuidV4 { .. }),
        "native record uid suffixes must be UUIDv4, got: {err:?}"
    );
}

fn native_requirement_error(uid: &str) -> CorpusError {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("reqs/bad.toml"),
        &format!(
            r#"
schema_version = 1

[[requirements]]
uid = "{uid}"
id = "R-A"
layer = "sys"
title = "a"
"#
        ),
    );
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nrequirements = [\"reqs/**/*.toml\"]\n",
    );
    CorpusIndex::load_graph(&dir.path().join("corpus.toml")).unwrap_err()
}
