//! Tests for the three ordered committed-lock gates and the
//! blocking reader (TEST-151).

use super::fixtures::*;
use super::*;
use crate::corpus::{EdgeKind, SourceError};

/// Canonical committed bytes of the fixture graph's derived lock.
fn canonical_text() -> String {
    let graph = four_document_graph();
    graph.validate().expect("fixture graph validates");
    String::from_utf8(render_lock_canonical(&derive_lock(&graph)))
        .expect("canonical bytes are UTF-8")
}

/// Split canonical lock text into (header, entry blocks).
fn split_blocks(text: &str) -> (&str, Vec<&str>) {
    let (header, entries) = text
        .split_once("\n\n")
        .expect("canonical text has a header and entries");
    let blocks = entries
        .split("\n\n")
        .map(|block| block.trim_end_matches('\n'))
        .collect();
    (header, blocks)
}

/// Reassemble (header, blocks) into lock text in canonical layout.
fn join_blocks(header: &str, blocks: &[&str]) -> String {
    format!("{header}\n\n{}\n", blocks.join("\n\n"))
}

/// A canonical committed lock validates against its graph (TEST-151).
#[test]
fn canonical_committed_lock_validates() {
    let graph = four_document_graph();
    let bytes = render_lock_canonical(&derive_lock(&graph));
    validate_committed_lock(&bytes, &graph).expect("canonical lock validates");

    // The empty baseline boundary: a graph with no sources derives
    // an empty lock, and the canonical empty form validates.
    let empty_graph = crate::corpus::CorpusGraph::new();
    empty_graph.validate().expect("empty graph validates");
    let empty_bytes = render_lock_canonical(&derive_lock(&empty_graph));
    assert_eq!(empty_bytes, b"schema_version = 1\n");
    validate_committed_lock(&empty_bytes, &empty_graph).expect("empty lock validates");
}

/// Gate 2 rejects every non-canonical byte form — reordered
/// entries, reordered fields, literal-string quoting, extra
/// whitespace, a comment line, and missing or extra trailing
/// newline — even though every variant parses to an equivalent
/// value (TEST-151).
#[test]
fn noncanonical_committed_bytes_fail_gate_two() {
    let canonical = canonical_text();
    let (header, blocks) = split_blocks(&canonical);

    let mut swapped = blocks.clone();
    swapped.swap(0, 1);
    let reordered_entries = join_blocks(header, &swapped);

    let reordered_fields = canonical.replacen(
        &format!("document_key = \"DOC-1\"\nsource_uid = \"{SRC_1H}\""),
        &format!("source_uid = \"{SRC_1H}\"\ndocument_key = \"DOC-1\""),
        1,
    );

    let cases: [(&str, String); 7] = [
        ("reordered entries", reordered_entries),
        ("reordered fields", reordered_fields),
        (
            "literal-string quoting",
            canonical.replacen("document_key = \"DOC-1\"", "document_key = 'DOC-1'", 1),
        ),
        (
            "extra whitespace",
            canonical.replacen("document_key = \"DOC-1\"", "document_key  =  \"DOC-1\"", 1),
        ),
        (
            "a comment line",
            canonical.replacen(
                "schema_version = 1\n",
                "schema_version = 1\n# a comment\n",
                1,
            ),
        ),
        (
            "missing trailing newline",
            canonical
                .strip_suffix('\n')
                .expect("canonical text ends with LF")
                .to_string(),
        ),
        ("extra trailing newline", format!("{canonical}\n")),
    ];
    let graph = four_document_graph();
    for (name, text) in cases {
        parse_lock(text.as_bytes()).expect("non-canonical variant must still parse");
        let err = validate_committed_lock(text.as_bytes(), &graph)
            .expect_err("non-canonical bytes must fail");
        assert!(
            matches!(err, SourceError::Lock(SourceLockError::NonCanonical { .. })),
            "{name} must fail with NonCanonical, got: {err:?}"
        );
    }
}

/// Gate 3 reports a missing effective-head entry and an entry with
/// no effective head, each naming the document key (TEST-151).
#[test]
fn missing_and_extra_entries_fail_closed() {
    let canonical = canonical_text();
    let (header, blocks) = split_blocks(&canonical);
    assert_eq!(blocks.len(), 4);

    // Drop the DOC-2 block: the graph still derives it.
    let missing_blocks: Vec<&str> = blocks
        .iter()
        .copied()
        .filter(|block| !block.contains("document_key = \"DOC-2\""))
        .collect();
    let missing_text = join_blocks(header, &missing_blocks);
    let graph = four_document_graph();
    let err = validate_committed_lock(missing_text.as_bytes(), &graph)
        .expect_err("a dropped entry must fail");
    assert!(
        matches!(
            err,
            SourceError::Lock(SourceLockError::Missing { ref document_key })
            if document_key.as_str() == DOC_2
        ),
        "expected Missing naming DOC-2, got: {err:?}"
    );

    // Append a canonical-form DOC-9 block the graph does not derive.
    let extra_block = format!(
        "[[entries]]\ndocument_key = \"DOC-9\"\nsource_uid = \"{SRC_1}\"\navailability = \"unavailable\""
    );
    let mut extra_blocks: Vec<String> = blocks.iter().map(|block| (*block).to_string()).collect();
    extra_blocks.push(extra_block);
    let extra_refs: Vec<&str> = extra_blocks.iter().map(String::as_str).collect();
    let extra_text = join_blocks(header, &extra_refs);
    let err = validate_committed_lock(extra_text.as_bytes(), &graph)
        .expect_err("an invented entry must fail");
    assert!(
        matches!(
            err,
            SourceError::Lock(SourceLockError::Extra { ref document_key })
            if document_key.as_str() == "DOC-9"
        ),
        "expected Extra naming DOC-9, got: {err:?}"
    );
}

/// Gate 3 reports a changed uid, digest, availability, capture
/// mode, or external identity, naming the document key and the
/// field (TEST-151).
#[test]
fn changed_fields_fail_naming_the_field() {
    let canonical = canonical_text();
    let cases: [(&str, String); 5] = [
        (
            "uid",
            canonical.replacen(
                &format!("source_uid = \"{SRC_2}\""),
                &format!("source_uid = \"{SRC_1}\""),
                1,
            ),
        ),
        (
            "digest",
            canonical.replacen(
                &format!("sha256 = \"{DIGEST_B}\""),
                &format!("sha256 = \"{DIGEST_A}\""),
                1,
            ),
        ),
        (
            "availability",
            canonical.replacen(
                &format!(
                    "document_key = \"DOC-4\"\nsource_uid = \"{SRC_4}\"\navailability = \"unavailable\"\n"
                ),
                &format!(
                    "document_key = \"DOC-4\"\nsource_uid = \"{SRC_4}\"\navailability = \"available\"\nsha256 = \"{DIGEST_A}\"\ncapture_mode = \"hash_only\"\n"
                ),
                1,
            ),
        ),
        (
            "capture_mode",
            canonical.replacen(
                "capture_mode = \"hash_only\"",
                "capture_mode = \"vendored\"",
                1,
            ),
        ),
        (
            "external_identity",
            canonical.replacen("immutable_id = \"DOC-3@revC\"", "immutable_id = \"DOC-3@revD\"", 1),
        ),
    ];
    let graph = four_document_graph();
    for (field, text) in cases {
        let err = validate_committed_lock(text.as_bytes(), &graph)
            .expect_err("a changed field must fail");
        assert!(
            matches!(
                err,
                SourceError::Lock(SourceLockError::Changed {
                    field: changed_field,
                    ..
                }) if changed_field == field
            ),
            "expected Changed naming field {field}, got: {err:?}"
        );
    }
}

/// The graph is validated before the gates run: an invalid graph
/// fails with the typed graph error even when the committed bytes
/// are themselves degenerate (TEST-151).
#[test]
fn graph_is_validated_before_the_gates() {
    // A fork: SRC_1 superseded by both SRC_2 and SRC_3 under DOC-1.
    let mut successor_b = vendored_revision(SRC_2, DOC_1, DIGEST_B);
    source_node_mut(&mut successor_b)
        .edges
        .push((EdgeKind::Supersedes, SRC_1.to_string()));
    let mut successor_c = vendored_revision(SRC_3, DOC_1, DIGEST_C);
    source_node_mut(&mut successor_c)
        .edges
        .push((EdgeKind::Supersedes, SRC_1.to_string()));
    let graph = graph_of(vec![
        vendored_revision(SRC_1, DOC_1, DIGEST_A),
        successor_b,
        successor_c,
    ]);
    assert!(
        graph.validate().is_err(),
        "the forked fixture graph must be invalid"
    );
    let err = validate_committed_lock(b"not even toml", &graph)
        .expect_err("an invalid graph must fail before the gates");
    assert!(
        matches!(err, SourceError::Lock(SourceLockError::InvalidGraph { .. })),
        "expected InvalidGraph, got: {err:?}"
    );
}

/// The blocking reader returns the parsed lock and never mutates
/// the file; an unreadable path fails closed (TEST-151).
#[test]
fn read_lock_blocking_reads_without_mutation() {
    let canonical = canonical_text();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sources.lock");
    std::fs::write(&path, &canonical).expect("write lock file");

    let lock = read_lock_blocking(&path).expect("read parses");
    let expected = parse_lock(canonical.as_bytes()).expect("canonical bytes parse");
    assert_eq!(lock, expected);
    let after = std::fs::read(&path).expect("re-read lock file");
    assert_eq!(
        after,
        canonical.as_bytes(),
        "the reader must not mutate the file"
    );

    let err = read_lock_blocking(&dir.path().join("absent.lock"))
        .expect_err("an unreadable path must fail");
    assert!(
        matches!(err, SourceError::Lock(SourceLockError::Read { .. })),
        "expected Read, got: {err:?}"
    );
}

/// An unavailable effective head round-trips through derive,
/// render, parse, and the full three-gate validation without ever
/// carrying a digest (TEST-151).
#[test]
fn unavailable_head_round_trips_digest_free_through_the_gates() {
    let graph = graph_of(vec![unavailable_revision(SRC_4, DOC_4)]);
    graph.validate().expect("fixture graph validates");
    let bytes = render_lock_canonical(&derive_lock(&graph));
    let text = String::from_utf8(bytes.clone()).expect("canonical bytes are UTF-8");
    assert!(
        !text.contains("sha256"),
        "an unavailable-only lock must carry no digest line: {text}"
    );
    assert!(!text.contains("capture_mode"));

    let parsed = parse_lock(&bytes).expect("canonical bytes parse");
    assert_eq!(parsed.entries.len(), 1);
    assert_eq!(parsed.entries[0].sha256, None);
    assert_eq!(
        parsed.entries[0].availability,
        LockAvailability::Unavailable
    );

    validate_committed_lock(&bytes, &graph).expect("the round-tripped lock validates");
}
