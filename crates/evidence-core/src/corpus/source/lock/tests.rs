//! Tests for lock derivation and the canonical render/parse pair
//! (TEST-149, TEST-150).

use super::fixtures::*;
use super::*;
use crate::corpus::{SourceCapture, SourceContentDigest, SourceError, SourceMaterial};

/// Derivation binds each effective head exactly once: the
/// superseded DOC-1 revision stays in the registry and lineage but
/// never appears as an active lock entry (TEST-149).
#[test]
fn derive_lock_binds_effective_heads_only() {
    let graph = four_document_graph();
    graph.validate().expect("fixture graph validates");
    let lock = derive_lock(&graph);
    assert_eq!(lock.schema_version, SUPPORTED_LOCK_SCHEMA);
    assert_eq!(lock.entries.len(), 4, "one entry per document key");
    let keys: Vec<&str> = lock
        .entries
        .iter()
        .map(|entry| entry.document_key.as_str())
        .collect();
    assert_eq!(keys, vec![DOC_1, DOC_2, DOC_3, DOC_4]);
    let uids: Vec<&str> = lock
        .entries
        .iter()
        .map(|entry| entry.source_uid.as_str())
        .collect();
    assert_eq!(uids, vec![SRC_1H, SRC_2, SRC_3, SRC_4]);
    assert!(
        !uids.contains(&SRC_1),
        "the superseded DOC-1 revision must not be an active lock entry"
    );

    let doc_1 = &lock.entries[0];
    assert_eq!(doc_1.availability, LockAvailability::Available);
    assert_eq!(
        doc_1.sha256.as_ref().map(|digest| digest.as_str()),
        Some(DIGEST_A)
    );
    assert_eq!(doc_1.capture_mode, Some(LockCaptureMode::Vendored));
    assert_eq!(doc_1.external_control, None);

    let doc_3 = &lock.entries[2];
    assert_eq!(
        doc_3.capture_mode,
        Some(LockCaptureMode::ExternalControlled)
    );
    assert_eq!(
        doc_3.external_control,
        Some(ExternalControlId {
            system: "plm-hd".to_string(),
            immutable_id: "DOC-3@revC".to_string(),
        })
    );
}

/// Equivalent layouts and insertion orders derive identical locks
/// and render byte-identical canonical output (TEST-149).
#[test]
fn layout_and_insertion_order_produce_identical_locks() {
    let forward = four_document_graph();
    // The same nodes in reverse insertion order.
    let mut reversed_nodes = four_document_nodes();
    reversed_nodes.reverse();
    let reversed = graph_of(reversed_nodes);
    // The same graph with the supersedes edge declared on an
    // otherwise identical node built separately — graph equality
    // already canonicalizes edge order at insert.
    let rebuilt = four_document_graph();

    let base = derive_lock(&forward);
    assert_eq!(derive_lock(&reversed), base);
    assert_eq!(derive_lock(&rebuilt), base);
    assert_eq!(
        render_lock_canonical(&derive_lock(&reversed)),
        render_lock_canonical(&base),
        "insertion order must never reach the canonical bytes"
    );
}

/// Recipe identity is not lock identity: capture-time and storage
/// details — the retrieval timestamp, the vendored payload path,
/// the title, and the canonical location — never move the lock.
/// The lock binds which digest and availability state the baseline
/// selected, never how the bytes were produced, where they are
/// mirrored, or whether the payload on disk still matches (TEST-149).
#[test]
fn recipe_and_storage_details_are_not_lock_identity() {
    let base = graph_of(vec![vendored_revision(SRC_2, DOC_2, DIGEST_B)]);
    base.validate().expect("fixture graph validates");
    let mut changed_node = vendored_revision(SRC_2, DOC_2, DIGEST_B);
    let revision = source_node_mut(&mut changed_node);
    revision.title = "a different title".to_string();
    revision.canonical_location = "https://mirror.example.org/other".to_string();
    let SourceMaterial::Available {
        retrieved_at,
        capture,
        ..
    } = &mut revision.material
    else {
        unreachable!("vendored fixture is available")
    };
    *retrieved_at = "2030-01-01T00:00:00Z".to_string();
    let SourceCapture::Vendored { path } = capture else {
        unreachable!("vendored fixture is vendored")
    };
    *path = "sources/elsewhere/deep-copy.pdf".to_string();
    let changed = graph_of(vec![changed_node]);
    changed.validate().expect("changed graph validates");
    assert_eq!(derive_lock(&base), derive_lock(&changed));
    assert_eq!(
        render_lock_canonical(&derive_lock(&base)),
        render_lock_canonical(&derive_lock(&changed)),
        "recipe and storage details must never move the canonical lock bytes"
    );
}

/// An unavailable effective head derives an explicit unavailable
/// entry with no digest and no capture mode — never a synthetic
/// value (TEST-149).
#[test]
fn unavailable_heads_derive_without_digest_or_capture() {
    let graph = graph_of(vec![unavailable_revision(SRC_4, DOC_4)]);
    graph.validate().expect("fixture graph validates");
    let lock = derive_lock(&graph);
    assert_eq!(lock.entries.len(), 1);
    let entry = &lock.entries[0];
    assert_eq!(entry.availability, LockAvailability::Unavailable);
    assert_eq!(entry.sha256, None, "unavailable heads never get a digest");
    assert_eq!(entry.capture_mode, None);
    assert_eq!(entry.external_control, None);
}

/// Vendored paths, retrieval timestamps, and unavailability reasons
/// are storage and audit details: none may enter the lock value or
/// its canonical bytes (TEST-149).
#[test]
fn vendored_paths_timestamps_and_reasons_never_enter_the_lock() {
    let graph = four_document_graph();
    let rendered = String::from_utf8(render_lock_canonical(&derive_lock(&graph)))
        .expect("canonical bytes are UTF-8");
    for excluded in [VENDORED_PATH, RETRIEVED_AT, UNAVAILABLE_REASON] {
        assert!(
            !rendered.contains(excluded),
            "canonical lock bytes must not contain {excluded:?}"
        );
    }
}

/// The canonical template is pinned: sorted entries, fixed field
/// order, basic-string quoting, one blank line between tables, and
/// a single trailing newline (TEST-150).
#[test]
fn canonical_render_pins_entry_order_field_order_and_trailing_newline() {
    let graph = four_document_graph();
    let rendered = render_lock_canonical(&derive_lock(&graph));
    let expected = format!(
        "schema_version = 1\n\
         \n\
         [[entries]]\n\
         document_key = \"DOC-1\"\n\
         source_uid = \"{SRC_1H}\"\n\
         availability = \"available\"\n\
         sha256 = \"{DIGEST_A}\"\n\
         capture_mode = \"vendored\"\n\
         \n\
         [[entries]]\n\
         document_key = \"DOC-2\"\n\
         source_uid = \"{SRC_2}\"\n\
         availability = \"available\"\n\
         sha256 = \"{DIGEST_B}\"\n\
         capture_mode = \"hash_only\"\n\
         \n\
         [[entries]]\n\
         document_key = \"DOC-3\"\n\
         source_uid = \"{SRC_3}\"\n\
         availability = \"available\"\n\
         sha256 = \"{DIGEST_C}\"\n\
         capture_mode = \"external_controlled\"\n\
         external_control = {{ system = \"plm-hd\", immutable_id = \"DOC-3@revC\" }}\n\
         \n\
         [[entries]]\n\
         document_key = \"DOC-4\"\n\
         source_uid = \"{SRC_4}\"\n\
         availability = \"unavailable\"\n"
    );
    assert_eq!(
        String::from_utf8(rendered.clone()).expect("canonical bytes are UTF-8"),
        expected
    );
    assert!(rendered.ends_with(b"\n"), "exactly one trailing newline");
    assert!(!rendered.ends_with(b"\n\n"), "no trailing blank line");

    // An out-of-order value renders the same canonical bytes: the
    // renderer re-sorts, so no construction path affects output.
    let mut shuffled = derive_lock(&graph);
    shuffled.entries.reverse();
    assert_eq!(render_lock_canonical(&shuffled), rendered);

    // The empty lock is exactly the schema line.
    let empty = SourceLock {
        schema_version: SUPPORTED_LOCK_SCHEMA,
        entries: Vec::new(),
    };
    assert_eq!(render_lock_canonical(&empty), b"schema_version = 1\n");
}

/// Canonical bytes parse back into the rendered value; the empty
/// lock round-trips too (TEST-150).
#[test]
fn render_parse_round_trip() {
    let graph = four_document_graph();
    let lock = derive_lock(&graph);
    let parsed = parse_lock(&render_lock_canonical(&lock)).expect("canonical bytes parse");
    assert_eq!(parsed, lock);

    let empty = SourceLock {
        schema_version: SUPPORTED_LOCK_SCHEMA,
        entries: Vec::new(),
    };
    let parsed_empty =
        parse_lock(&render_lock_canonical(&empty)).expect("empty canonical bytes parse");
    assert_eq!(parsed_empty, empty);
}

/// Parsing accepts any entry order and preserves it; order
/// enforcement is the canonicality gate's concern (TEST-150).
#[test]
fn parse_accepts_any_entry_order() {
    let canonical = render_lock_canonical(&derive_lock(&four_document_graph()));
    let parsed = parse_lock(&canonical).expect("canonical bytes parse");
    let mut reordered = parsed.clone();
    reordered.entries.reverse();
    // Rebuilding committed bytes in reversed entry order parses
    // cleanly and keeps the committed order.
    let reversed_text = reversed_lock_text(&canonical);
    let parsed_reversed = parse_lock(reversed_text.as_bytes()).expect("any order parses");
    assert_eq!(parsed_reversed.entries, reordered.entries);
    assert_ne!(
        render_lock_canonical(&parsed_reversed),
        reversed_text.as_bytes(),
        "the canonicality gate, not the parser, rejects entry order"
    );
}

/// Reverse the `[[entries]]` blocks of canonical lock text without
/// touching field order inside a block.
fn reversed_lock_text(canonical: &[u8]) -> String {
    let text = String::from_utf8(canonical.to_vec()).expect("canonical bytes are UTF-8");
    let (header, entries) = text
        .split_once("\n\n")
        .expect("canonical text has a header and entries");
    let mut blocks: Vec<&str> = entries
        .split("\n\n")
        .map(|block| block.trim_end_matches('\n'))
        .collect();
    blocks.reverse();
    format!("{header}\n\n{}\n", blocks.join("\n\n"))
}

/// One document key on two entries fails at parse with a typed
/// duplicate-key error (TEST-150).
#[test]
fn duplicate_document_key_fails_at_parse() {
    let text = format!(
        "schema_version = 1\n\
         \n\
         [[entries]]\n\
         document_key = \"DOC-1\"\n\
         source_uid = \"{SRC_1H}\"\n\
         availability = \"unavailable\"\n\
         \n\
         [[entries]]\n\
         document_key = \"DOC-1\"\n\
         source_uid = \"{SRC_2}\"\n\
         availability = \"unavailable\"\n"
    );
    let err = parse_lock(text.as_bytes()).expect_err("duplicate keys must fail");
    assert!(
        matches!(
            err,
            SourceError::Lock(SourceLockError::DuplicateKey { ref document_key })
            if document_key.as_str() == DOC_1
        ),
        "expected DuplicateKey naming DOC-1, got: {err:?}"
    );
}

/// A malformed digest — uppercase, short, or non-hex — fails closed
/// at parse through the validating digest type (TEST-150).
#[test]
fn malformed_digest_fails_at_parse() {
    for (name, digest) in [
        ("uppercase", DIGEST_A.to_uppercase()),
        ("short", DIGEST_A[..63].to_string()),
        ("non-hex", format!("{}g", &DIGEST_A[..63])),
    ] {
        let text = format!(
            "schema_version = 1\n\
             \n\
             [[entries]]\n\
             document_key = \"DOC-1\"\n\
             source_uid = \"{SRC_1H}\"\n\
             availability = \"available\"\n\
             sha256 = \"{digest}\"\n\
             capture_mode = \"hash_only\"\n"
        );
        let err = parse_lock(text.as_bytes()).expect_err("malformed digest must fail");
        assert!(
            matches!(err, SourceError::Lock(SourceLockError::Parse { .. })),
            "{name} digest must fail with Parse, got: {err:?}"
        );
    }
}

/// Unknown fields — top-level, per-entry, or inside the
/// external-control table — fail closed, and an unavailable entry
/// carrying a digest or capture mode fails deserialization instead
/// of being invented or silently dropped (TEST-150).
#[test]
fn unknown_field_and_unavailable_with_digest_fail_at_parse() {
    let base_entry = format!(
        "[[entries]]\n\
         document_key = \"DOC-1\"\n\
         source_uid = \"{SRC_1H}\"\n"
    );
    let cases: [(&str, String); 5] = [
        (
            "unknown top-level field",
            format!(
                "schema_version = 1\nbogus = true\n\n{base_entry}availability = \"unavailable\"\n"
            ),
        ),
        (
            "unknown entry field",
            format!(
                "schema_version = 1\n\n{base_entry}availability = \"unavailable\"\nbogus = 1\n"
            ),
        ),
        (
            "unavailable entry carrying a digest",
            format!(
                "schema_version = 1\n\n{base_entry}availability = \"unavailable\"\nsha256 = \"{DIGEST_A}\"\n"
            ),
        ),
        (
            "unavailable entry carrying a capture mode",
            format!(
                "schema_version = 1\n\n{base_entry}availability = \"unavailable\"\ncapture_mode = \"hash_only\"\n"
            ),
        ),
        (
            "unknown external-control field",
            format!(
                "schema_version = 1\n\n{base_entry}availability = \"available\"\nsha256 = \"{DIGEST_A}\"\ncapture_mode = \"external_controlled\"\nexternal_control = {{ system = \"plm-hd\", immutable_id = \"X@1\", bogus = \"y\" }}\n"
            ),
        ),
    ];
    for (name, text) in cases {
        let err = parse_lock(text.as_bytes()).expect_err("degenerate input must fail");
        assert!(
            matches!(err, SourceError::Lock(SourceLockError::Parse { .. })),
            "{name} must fail with Parse, got: {err:?}"
        );
    }

    // The capture-mode/external-control invariant fails closed in
    // both directions.
    for (name, capture_lines) in [
        (
            "external-controlled without the identity",
            "capture_mode = \"external_controlled\"\n".to_string(),
        ),
        (
            "identity without external-controlled",
            "capture_mode = \"vendored\"\nexternal_control = { system = \"plm-hd\", immutable_id = \"X@1\" }\n".to_string(),
        ),
    ] {
        let text = format!(
            "schema_version = 1\n\n{base_entry}availability = \"available\"\nsha256 = \"{DIGEST_A}\"\n{capture_lines}"
        );
        let err = parse_lock(text.as_bytes()).expect_err("inconsistent capture must fail");
        assert!(
            matches!(err, SourceError::Lock(SourceLockError::Parse { .. })),
            "{name} must fail with Parse, got: {err:?}"
        );
    }
}

/// A lock declaring a newer schema refuses to load with a typed
/// too-new error naming the found and supported versions (TEST-150).
#[test]
fn newer_schema_fails_closed() {
    let text = "schema_version = 2\n";
    let err = parse_lock(text.as_bytes()).expect_err("newer schema must fail");
    assert!(
        matches!(
            err,
            SourceError::Lock(SourceLockError::SchemaTooNew {
                found: 2,
                supported: SUPPORTED_LOCK_SCHEMA,
            })
        ),
        "expected SchemaTooNew, got: {err:?}"
    );
}

/// The pinned escaping contract round-trips: quotes, backslashes,
/// shorthand and `\uXXXX` control escapes, and raw non-ASCII UTF-8
/// all parse back to the exact value (TEST-150).
#[test]
fn escaping_round_trips_special_strings() {
    let tricky = "quote\" backslash\\ tab\t newline\n del\u{7F} bell\u{07} ünïcödé 漢字";
    let lock = SourceLock {
        schema_version: SUPPORTED_LOCK_SCHEMA,
        entries: vec![SourceLockEntry {
            document_key: tricky.to_string(),
            source_uid: SRC_1H.to_string(),
            availability: LockAvailability::Available,
            sha256: Some(SourceContentDigest::from_hex(DIGEST_A).unwrap()),
            capture_mode: Some(LockCaptureMode::ExternalControlled),
            external_control: Some(ExternalControlId {
                system: tricky.to_string(),
                immutable_id: "plain".to_string(),
            }),
        }],
    };
    let rendered = render_lock_canonical(&lock);
    let text = String::from_utf8(rendered.clone()).expect("canonical bytes are UTF-8");
    assert!(
        text.contains(
            "quote\\\" backslash\\\\ tab\\t newline\\n del\\u007F bell\\u0007 ünïcödé 漢字"
        ),
        "escaping must follow the pinned minimal contract, got: {text}"
    );
    let parsed = parse_lock(&rendered).expect("escaped bytes parse");
    assert_eq!(parsed, lock, "escaping must round-trip exactly");
}
