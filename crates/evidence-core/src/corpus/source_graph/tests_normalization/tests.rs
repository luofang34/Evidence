//! Tests for canonical prose/code normalization and the digest
//! and fingerprint contracts (TEST-170). The golden vectors in
//! `tests/corpus_source_graph.rs` byte-lock the same contracts.

use super::normalization::{content_digest, fingerprint, normalize_code, normalize_prose};
use super::{SourceNode, SourceNodeKind};

/// Prose normalization: NFC composition, whitespace-run folding,
/// and trimming (TEST-170).
#[test]
fn prose_folds_nfc_whitespace_and_trim() {
    assert_eq!(normalize_prose("  Hello   World  "), "Hello World");
    assert_eq!(normalize_prose("a\r\nb\tc  d\ne"), "a b c d e");
    assert_eq!(normalize_prose(""), "");
    assert_eq!(normalize_prose(" \t\n "), "");
    // NFC: combining sequences compose.
    assert_eq!(normalize_prose("e\u{301}lan"), "\u{e9}lan");
    // Precomposed input is stable.
    assert_eq!(normalize_prose("\u{e9}lan"), "\u{e9}lan");
    // Ideographic space is whitespace and folds.
    assert_eq!(normalize_prose("a\u{3000}b"), "a b");
    // Interior single spaces are preserved exactly.
    assert_eq!(normalize_prose("one two three"), "one two three");
}

/// Code normalization: NFC and line-ending mapping only —
/// significant spaces and line boundaries are preserved exactly
/// (TEST-170).
#[test]
fn code_preserves_spaces_and_line_boundaries() {
    assert_eq!(
        normalize_code("fn main() {\r\n    x();\r\n}"),
        "fn main() {\n    x();\n}"
    );
    assert_eq!(normalize_code("a\rb\r\nc\n"), "a\nb\nc\n");
    // Leading/trailing spaces and blank lines survive.
    assert_eq!(
        normalize_code("  indented  \n\n\ncode  "),
        "  indented  \n\n\ncode  "
    );
    // NFC still applies, but never folding or trimming.
    assert_eq!(normalize_code("e\u{301} = 1"), "\u{e9} = 1");
    assert_eq!(normalize_code(""), "");
}

/// The content digest binds kind and exact text; the fingerprint
/// binds kind, label, and ancestry while excluding every
/// diagnostic position (TEST-170).
#[test]
fn fingerprint_excludes_positions_and_binds_ancestry() {
    // The digest domain separates kinds over identical text.
    let paragraph = content_digest(SourceNodeKind::Paragraph, "same text");
    let note = content_digest(SourceNodeKind::Note, "same text");
    assert_ne!(paragraph, note, "kind enters the content digest");
    let changed = content_digest(SourceNodeKind::Paragraph, "same text!");
    assert_ne!(paragraph, changed, "text enters the content digest");

    // Fingerprint inputs are kind, label, and ancestry only.
    let ancestry = [
        (SourceNodeKind::Section, Some("1 Introduction")),
        (SourceNodeKind::Section, None),
    ];
    let base = fingerprint(SourceNodeKind::Paragraph, None, &ancestry);
    assert_eq!(
        base,
        fingerprint(SourceNodeKind::Paragraph, None, &ancestry)
    );
    assert_ne!(
        base,
        fingerprint(SourceNodeKind::Paragraph, Some("caption"), &ancestry),
        "label enters the fingerprint"
    );
    assert_ne!(
        base,
        fingerprint(SourceNodeKind::ListItem, None, &ancestry),
        "kind enters the fingerprint"
    );
    let shorter_ancestry = [(SourceNodeKind::Section, Some("1 Introduction"))];
    assert_ne!(
        base,
        fingerprint(SourceNodeKind::Paragraph, None, &shorter_ancestry),
        "ancestry enters the fingerprint"
    );
    let relabeled_parent = [
        (SourceNodeKind::Section, Some("2 Overview")),
        (SourceNodeKind::Section, None),
    ];
    assert_ne!(
        base,
        fingerprint(SourceNodeKind::Paragraph, None, &relabeled_parent),
        "ancestor labels enter the fingerprint"
    );

    // Two nodes differing only in diagnostic position — ordinal,
    // byte range, page — carry equal fingerprints over equal
    // structural inputs.
    let low = fingerprint(SourceNodeKind::Paragraph, None, &ancestry);
    let high = fingerprint(SourceNodeKind::Paragraph, None, &ancestry);
    assert_eq!(low, high);

    // The two digest domains are disjoint: identical inputs under
    // the content and fingerprint tags digest differently.
    let content = content_digest(SourceNodeKind::Paragraph, "x");
    assert_ne!(
        content,
        fingerprint(SourceNodeKind::Paragraph, None, &[]),
        "domain tags separate the encodings"
    );
}

/// Recomputing a node's digest and fingerprint from its own
/// fields reproduces the stored values — the invariant validation
/// enforces (TEST-170).
#[test]
fn node_digests_recompute_from_committed_fields() {
    let section = SourceNode {
        uid: "snode_00000000-0000-4000-8000-0000000000b1".to_string(),
        source_revision_uid: "src_00000000-0000-4000-8000-0000000000a1".to_string(),
        parent_uid: None,
        kind: SourceNodeKind::Section,
        ordinal: 0,
        label: Some("1 Introduction".to_string()),
        canonical_text: String::new(),
        content_sha256: content_digest(SourceNodeKind::Section, ""),
        fingerprint: fingerprint(SourceNodeKind::Section, Some("1 Introduction"), &[]),
        locator: crate::corpus::SourceLocator::Markdown {
            path: crate::corpus::SafeRelPath::new("docs/spec.md").expect("safe path"),
            git_blob: None,
            anchor: None,
            heading_path: Vec::new(),
            byte_range: (0, 0),
        },
    };
    assert_eq!(
        section.content_sha256,
        content_digest(section.kind, &section.canonical_text)
    );
    assert_eq!(
        section.fingerprint,
        fingerprint(section.kind, section.label.as_deref(), &[])
    );
}
