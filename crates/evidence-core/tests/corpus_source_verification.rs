//! End-to-end offline source verification through `CorpusIndex`
//! (TEST-155): a tempdir corpus carrying all four capture modes
//! loads through `CorpusIndex::load_graph`, its derived lock
//! round-trips through `read_lock_blocking`, and
//! `verify_effective_sources` reports each head's typed state —
//! with `https://` canonical locations, no network access, and no
//! mutation of registry, lock, or payload bytes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use evidence_core::corpus::{
    CorpusIndex, SourceVerificationState, derive_lock, read_lock_blocking, render_lock_canonical,
    verify_effective_sources,
};

const SRC_1: &str = "src_00000000-0000-4000-8000-0000000000a1";
const SRC_2: &str = "src_00000000-0000-4000-8000-0000000000a2";
const SRC_3: &str = "src_00000000-0000-4000-8000-0000000000a3";
const SRC_4: &str = "src_00000000-0000-4000-8000-0000000000a4";
const VENDORED_PATH: &str = "sources/doc-1/rev-c.pdf";
const PAYLOAD_BYTES: &[u8] = b"DOC-1 rev C payload bytes\n";
const UNAVAILABLE_REASON: &str = "export control blocks capture";

/// One available record's TOML block.
fn available_record(uid: &str, document_key: &str, sha256: &str, capture_toml: &str) -> String {
    format!(
        "\n[[sources]]\nuid = \"{uid}\"\nid = \"id of {uid}\"\ndocument_key = \"{document_key}\"\ntitle = \"title of {uid}\"\nmedia_type = \"application/pdf\"\ncanonical_location = \"https://example.org/specs/{uid}\"\n[sources.material]\nstate = \"available\"\nretrieved_at = \"2026-07-01T10:00:00Z\"\nsha256 = \"{sha256}\"\n\n[sources.material.capture]\n{capture_toml}"
    )
}

/// Write a file, creating parent directories.
fn write(path: &Path, content: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(path, content).expect("write");
}

/// Read-only byte snapshot of every file beneath `root`, keyed by
/// relative path; symlinks are never followed.
fn snapshot_tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .map(|entry| entry.expect("walk entry"))
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| {
            (
                entry
                    .path()
                    .strip_prefix(root)
                    .expect("under root")
                    .to_path_buf(),
                fs::read(entry.path()).expect("read"),
            )
        })
        .collect()
}

/// The full pipeline — record files, payload bytes, graph load,
/// lock derive/render/read-back, batch verification — completes
/// offline and read-only (TEST-155).
#[test]
fn corpus_index_loads_and_verifies_end_to_end() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let vendored_digest = evidence_core::hash::sha256(PAYLOAD_BYTES);

    write(
        &root.join("corpus.toml"),
        b"schema_version = 1\nsources = [\"sources/**/*.toml\"]\n",
    );
    let vendored_capture = format!("mode = \"vendored\"\npath = \"{VENDORED_PATH}\"\n");
    let unavailable_record = format!(
        "\n[[sources]]\nuid = \"{SRC_4}\"\nid = \"id of {SRC_4}\"\ndocument_key = \"DOC-4\"\ntitle = \"title of {SRC_4}\"\nmedia_type = \"application/pdf\"\ncanonical_location = \"https://example.org/specs/{SRC_4}\"\n[sources.material]\nstate = \"unavailable\"\nreason = \"{UNAVAILABLE_REASON}\"\n"
    );
    let records = format!(
        "schema_version = 1\n{}{}{}{}",
        available_record(SRC_1, "DOC-1", &vendored_digest, &vendored_capture),
        available_record(SRC_2, "DOC-2", &"b".repeat(64), "mode = \"hash_only\"\n"),
        available_record(
            SRC_3,
            "DOC-3",
            &"c".repeat(64),
            "mode = \"external_controlled\"\nsystem = \"plm-hd\"\nimmutable_id = \"DOC-3@revC\"\n",
        ),
        unavailable_record,
    );
    write(&root.join("sources/records.toml"), records.as_bytes());
    write(&root.join(VENDORED_PATH), PAYLOAD_BYTES);

    let graph =
        CorpusIndex::load_graph(&root.join("corpus.toml")).expect("the corpus loads and validates");
    let lock_bytes = render_lock_canonical(&derive_lock(&graph));
    write(&root.join("sources.lock"), &lock_bytes);
    // The committed lock reads back through the blocking reader.
    let lock = read_lock_blocking(&root.join("sources.lock")).expect("the lock parses");
    assert_eq!(lock.entries.len(), 4);

    let before = snapshot_tree(root);
    let results =
        verify_effective_sources(root, &graph, &lock_bytes).expect("the global prerequisites pass");
    assert_eq!(
        results
            .iter()
            .map(|entry| entry.document_key.as_str())
            .collect::<Vec<_>>(),
        vec!["DOC-1", "DOC-2", "DOC-3", "DOC-4"],
        "one sorted entry per effective head"
    );
    let state_of = |document_key: &str| -> &SourceVerificationState {
        results
            .iter()
            .find(|entry| entry.document_key == document_key)
            .unwrap_or_else(|| panic!("entry for {document_key}"))
            .outcome
            .as_ref()
            .unwrap_or_else(|_| panic!("{document_key} yields a state"))
    };
    assert_eq!(state_of("DOC-1"), &SourceVerificationState::VerifiedBytes);
    assert_eq!(state_of("DOC-2"), &SourceVerificationState::DigestDeclared);
    assert_eq!(
        state_of("DOC-3"),
        &SourceVerificationState::ExternallyControlled
    );
    assert_eq!(
        state_of("DOC-4"),
        &SourceVerificationState::Unavailable {
            reason: UNAVAILABLE_REASON.to_string(),
        }
    );
    assert_eq!(
        snapshot_tree(root),
        before,
        "registry, lock, and payload bytes are untouched"
    );
}
