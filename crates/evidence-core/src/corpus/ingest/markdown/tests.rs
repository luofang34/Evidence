//! Adapter tests (TEST-178, TEST-179): structure projections,
//! anchors and heading paths, normalization, byte ranges, and the
//! typed diagnostic taxonomy.

use super::super::{
    IngestDiagnostic, IngestDiagnosticKind, IngestMarkdownInput, IngesterRecipe, MarkdownIngestion,
};
use super::adapter::checked_byte_range;
use super::ingest_markdown;
use crate::corpus::{SourceNode, SourceNodeKind, StructuralContentDigest};
use crate::hash::sha256;

const REV: &str = "src_00000000-0000-4000-8000-0000000000bb";

fn input_for(bytes: &[u8]) -> IngestMarkdownInput<'_> {
    IngestMarkdownInput {
        bytes,
        media_type: "text/markdown",
        source_revision_uid: REV,
        canonical_path: "docs/spec.md",
        input_digest: StructuralContentDigest::from_hasher_output(sha256(bytes)),
        git_blob: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
        recipe: IngesterRecipe {
            parser: "pulldown-cmark".to_string(),
            parser_version: "0.13.4".to_string(),
            extensions: ["footnotes".to_string(), "tables".to_string()]
                .into_iter()
                .collect(),
            adapter_version: "1".to_string(),
            normalization_contract: "evidence/source-node-normalization/v1".to_string(),
        },
    }
}

fn ingest(text: &str) -> MarkdownIngestion {
    ingest_markdown(&input_for(text.as_bytes())).expect("ingestion succeeds")
}

/// Find the one node of `kind` whose canonical text is `text`.
fn node_by_text<'n>(nodes: &'n [SourceNode], kind: SourceNodeKind, text: &str) -> &'n SourceNode {
    let found: Vec<&SourceNode> = nodes
        .iter()
        .filter(|n| n.kind == kind && n.canonical_text == text)
        .collect();
    assert_eq!(found.len(), 1, "expected one {kind:?} node {text:?}");
    found[0]
}

/// Find the one section node labeled `label`.
fn section_by_label<'n>(nodes: &'n [SourceNode], label: &str) -> &'n SourceNode {
    let found: Vec<&SourceNode> = nodes
        .iter()
        .filter(|n| n.kind == SourceNodeKind::Section && n.label.as_deref() == Some(label))
        .collect();
    assert_eq!(found.len(), 1, "expected one section {label:?}");
    found[0]
}

#[test]
fn headings_lists_tables_code_notes_project_structure() {
    let text = "# 1 Root {#root}\n\nIntro text.\n\n## 1.1 Child\n\n- outer\n  - inner\n- last\n\n| A | B |\n|---|---|\n| x | y |\n\n> [!WARNING]\n> Careful here.\n\n[^f]: note body\n";
    let outcome = ingest(text);
    assert!(outcome.diagnostics.is_empty());
    let nodes = &outcome.nodes;

    let root = section_by_label(nodes, "1 Root");
    assert_eq!(root.parent_uid, None);
    assert_eq!(root.ordinal, 0);
    assert_eq!(root.locator_anchor(), Some("root"));
    assert_eq!(root.locator_heading_path(), &["1 Root".to_string()]);

    let intro = node_by_text(nodes, SourceNodeKind::Paragraph, "Intro text.");
    assert_eq!(intro.parent_uid.as_deref(), Some(root.uid.as_str()));
    assert_eq!(intro.ordinal, 0);
    assert_eq!(intro.locator_heading_path(), &["1 Root".to_string()]);

    let child = section_by_label(nodes, "1.1 Child");
    assert_eq!(child.parent_uid.as_deref(), Some(root.uid.as_str()));
    assert_eq!(child.ordinal, 1);
    assert_eq!(child.locator_anchor(), Some("1-1-child"));
    assert_eq!(
        child.locator_heading_path(),
        &["1 Root".to_string(), "1.1 Child".to_string()]
    );

    // Nested list items project flat under the enclosing section in
    // document order.
    for (text, ordinal) in [("outer", 0), ("inner", 1), ("last", 2)] {
        let item = node_by_text(nodes, SourceNodeKind::ListItem, text);
        assert_eq!(item.parent_uid.as_deref(), Some(child.uid.as_str()));
        assert_eq!(item.ordinal, ordinal, "list item {text:?}");
    }

    // Table: header row is the first row; cells parent under rows.
    let table = nodes
        .iter()
        .find(|n| n.kind == SourceNodeKind::Table)
        .expect("one table");
    assert_eq!(table.parent_uid.as_deref(), Some(child.uid.as_str()));
    assert_eq!(table.ordinal, 3);
    let rows: Vec<&SourceNode> = nodes
        .iter()
        .filter(|n| n.kind == SourceNodeKind::TableRow)
        .collect();
    assert_eq!(rows.len(), 2);
    for (row, ordinal, cells) in [(rows[0], 0, ["A", "B"]), (rows[1], 1, ["x", "y"])] {
        assert_eq!(row.parent_uid.as_deref(), Some(table.uid.as_str()));
        assert_eq!(row.ordinal, ordinal);
        for (cell_text, cell_ordinal) in cells.iter().zip([0, 1]) {
            let cell = node_by_text(nodes, SourceNodeKind::TableCell, cell_text);
            assert_eq!(cell.parent_uid.as_deref(), Some(row.uid.as_str()));
            assert_eq!(cell.ordinal, cell_ordinal);
        }
    }

    // The alert marker is stripped from the note; the footnote
    // definition becomes a labeled note.
    let note = node_by_text(nodes, SourceNodeKind::Note, "Careful here.");
    assert_eq!(note.parent_uid.as_deref(), Some(child.uid.as_str()));
    assert_eq!(note.ordinal, 4);
    let footnote = node_by_text(nodes, SourceNodeKind::Note, "note body");
    assert_eq!(footnote.label.as_deref(), Some("footnote:f"));
    assert_eq!(footnote.parent_uid.as_deref(), Some(child.uid.as_str()));
    assert_eq!(footnote.ordinal, 5);

    // Every node binds the input's revision, path, and git blob.
    for node in nodes {
        assert_eq!(node.source_revision_uid, REV);
        assert_eq!(
            node.locator_git_blob(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert!(node.uid.starts_with("snode_"));
    }
}

#[test]
fn anchors_heading_paths_and_ordinals_project() {
    let text = "## Alpha\n\n## Beta {#custom-beta}\n\n## Beta!\n\n## Beta?\n";
    let outcome = ingest(text);
    assert!(outcome.diagnostics.is_empty());
    let nodes = &outcome.nodes;
    assert_eq!(nodes.len(), 4);

    let anchors: Vec<Option<&str>> = nodes.iter().map(|n| n.locator_anchor()).collect();
    assert_eq!(
        anchors,
        [
            Some("alpha"),
            Some("custom-beta"),
            Some("beta"),
            Some("beta-2"),
        ],
        "explicit ids win; generated slugs dedup with -2"
    );
    for (index, node) in nodes.iter().enumerate() {
        assert_eq!(node.ordinal, index as u32, "root siblings are contiguous");
        assert_eq!(node.parent_uid, None);
        assert_eq!(node.locator_heading_path().len(), 1);
    }
    // The explicit-id marker is stripped from the section's label
    // and canonical text.
    let beta = section_by_label(nodes, "Beta");
    assert_eq!(beta.canonical_text, "Beta");
}

#[test]
fn code_whitespace_survives_and_prose_normalizes() {
    let text = "Para   with\tmulti   space\nand NFD e\u{301}lan.\n\n```text\nline  one\n\n    indented   keeps\n```\n";
    let outcome = ingest(text);
    let paragraph = outcome
        .nodes
        .iter()
        .find(|n| n.kind == SourceNodeKind::Paragraph)
        .expect("one paragraph");
    assert_eq!(
        paragraph.canonical_text, "Para with multi space and NFD élan.",
        "prose folds whitespace runs and composes NFC"
    );
    let code = outcome
        .nodes
        .iter()
        .find(|n| n.kind == SourceNodeKind::CodeBlock)
        .expect("one code block");
    assert_eq!(
        code.canonical_text, "line  one\n\n    indented   keeps\n",
        "code preserves significant spaces, blank lines, and the trailing newline"
    );
}

#[test]
fn byte_ranges_slice_valid_utf8_and_unaligned_ranges_fail() {
    let text = "# T\n\nBody élan text.\n\n```\ncode é\n```\n";
    let bytes = text.as_bytes();
    let outcome = ingest(text);
    assert!(!outcome.nodes.is_empty());
    for node in &outcome.nodes {
        let (start, end) = node.locator_byte_range();
        assert!(start <= end, "ordered bounds for {}", node.uid);
        assert!((end as usize) <= bytes.len(), "in bounds for {}", node.uid);
        let slice = std::str::from_utf8(&bytes[start as usize..end as usize]);
        assert!(
            slice.is_ok(),
            "byte range [{start}, {end}] must slice valid UTF-8 for {}",
            node.uid
        );
    }

    // The checked constructor rejects malformed ranges.
    assert!(checked_byte_range("é", 0, 1).is_err(), "mid-character end");
    assert!(
        checked_byte_range("é", 1, 2).is_err(),
        "mid-character start"
    );
    assert!(checked_byte_range("ab", 0, 3).is_err(), "out of bounds");
    assert!(checked_byte_range("ab", 2, 1).is_err(), "reversed bounds");
    assert_eq!(
        checked_byte_range("ab", 1, 1).expect("empty at boundary"),
        (1, 1)
    );
    assert_eq!(checked_byte_range("ab", 0, 2).expect("full span"), (0, 2));
}

#[test]
fn duplicate_anchors_and_malformed_ids_produce_typed_diagnostics() {
    let text = "# One {#dup}\n\n# Two {#dup}\n\n# Three {#bad id}\n\n# Four {#}\n";
    let outcome = ingest(text);
    let kinds: Vec<&IngestDiagnosticKind> = outcome.diagnostics.iter().map(|d| &d.kind).collect();
    assert_eq!(kinds.len(), 3, "one duplicate and two malformed");
    assert!(
        kinds.iter().any(
            |k| matches!(k, IngestDiagnosticKind::DuplicateAnchor { anchor } if anchor == "dup")
        ),
        "the second dup claim diagnoses"
    );
    assert!(
        kinds.iter().any(
            |k| matches!(k, IngestDiagnosticKind::MalformedExplicitId { raw } if raw == "bad id")
        ),
        "a spaced id is malformed"
    );
    assert!(
        kinds.iter().any(
            |k| matches!(k, IngestDiagnosticKind::MalformedExplicitId { raw } if raw.is_empty())
        ),
        "an empty id is malformed"
    );

    // Nodes are still produced: the duplicate keeps the author's
    // claim; malformed headings slug from their full text.
    assert_eq!(outcome.nodes.len(), 4);
    let two = section_by_label(&outcome.nodes, "Two");
    assert_eq!(two.locator_anchor(), Some("dup"));
    let three = section_by_label(&outcome.nodes, "Three {#bad id}");
    assert_eq!(three.locator_anchor(), Some("three-bad-id"));
    let four = section_by_label(&outcome.nodes, "Four {#}");
    assert_eq!(four.locator_anchor(), Some("four"));

    // Diagnostics carry ranges into the source and sort by range,
    // kind, and detail.
    assert_sorted(&outcome.diagnostics);
    for diagnostic in &outcome.diagnostics {
        let (start, end) = diagnostic.byte_range;
        assert!(start <= end && (end as usize) <= text.len());
    }
}

#[test]
fn unsupported_html_and_lossy_constructs_produce_sorted_diagnostics() {
    let text = "Text with <b>inline</b> html.\n\n<div>block</div>\n\n![alt](img.png)\n\n---\n\n[^a]: first\n\n[^a]: second\n";
    let outcome = ingest(text);
    let diagnostics = &outcome.diagnostics;
    assert_eq!(diagnostics.len(), 6, "3 raw HTML + 3 lossy constructs");

    let html_count = diagnostics
        .iter()
        .filter(|d| matches!(d.kind, IngestDiagnosticKind::UnsupportedRawHtml))
        .count();
    assert_eq!(html_count, 3, "two inline tags and one block line");
    for construct in ["image", "thematic-break", "footnote-definition"] {
        assert!(
            diagnostics.iter().any(|d| matches!(
                &d.kind,
                IngestDiagnosticKind::LossyConstruct { construct: c } if *c == construct
            )),
            "lossy construct {construct:?} diagnoses"
        );
    }
    assert_sorted(diagnostics);

    // Nothing is silently dropped: surrounding text and the image's
    // alt text remain; the first footnote definition survives.
    let paragraph = node_by_text(
        &outcome.nodes,
        SourceNodeKind::Paragraph,
        "Text with inline html.",
    );
    assert_eq!(paragraph.kind, SourceNodeKind::Paragraph);
    node_by_text(&outcome.nodes, SourceNodeKind::Paragraph, "alt");
    let footnote = node_by_text(&outcome.nodes, SourceNodeKind::Note, "first");
    assert_eq!(footnote.label.as_deref(), Some("footnote:a"));
    assert!(
        outcome.nodes.iter().all(|n| n.canonical_text != "second"),
        "the duplicate definition produces no node"
    );
}

/// Assert the diagnostic list is sorted by (range, kind, detail).
fn assert_sorted(diagnostics: &[IngestDiagnostic]) {
    let mut sorted = diagnostics.to_vec();
    sorted.sort_by(|a, b| {
        (a.byte_range.0, a.byte_range.1, &a.kind, &a.detail).cmp(&(
            b.byte_range.0,
            b.byte_range.1,
            &b.kind,
            &b.detail,
        ))
    });
    assert_eq!(&sorted, diagnostics, "diagnostics must be sorted");
}

/// Locator accessors for the Markdown variant every node carries.
trait MarkdownLocator {
    fn locator_anchor(&self) -> Option<&str>;
    fn locator_heading_path(&self) -> &[String];
    fn locator_byte_range(&self) -> (u64, u64);
    fn locator_git_blob(&self) -> Option<&str>;
}

impl MarkdownLocator for SourceNode {
    fn locator_anchor(&self) -> Option<&str> {
        match &self.locator {
            crate::corpus::SourceLocator::Markdown { anchor, .. } => anchor.as_deref(),
            _ => panic!("markdown locator expected"),
        }
    }

    fn locator_heading_path(&self) -> &[String] {
        match &self.locator {
            crate::corpus::SourceLocator::Markdown { heading_path, .. } => heading_path,
            _ => panic!("markdown locator expected"),
        }
    }

    fn locator_byte_range(&self) -> (u64, u64) {
        match &self.locator {
            crate::corpus::SourceLocator::Markdown { byte_range, .. } => *byte_range,
            _ => panic!("markdown locator expected"),
        }
    }

    fn locator_git_blob(&self) -> Option<&str> {
        match &self.locator {
            crate::corpus::SourceLocator::Markdown { git_blob, .. } => git_blob.as_deref(),
            _ => panic!("markdown locator expected"),
        }
    }
}
