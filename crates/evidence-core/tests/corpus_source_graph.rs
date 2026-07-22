//! Structural source graph: golden normalization vectors and
//! layout-independence byte locks (TEST-170, TEST-172).
//!
//! The committed `source_graph_normalization_v1.golden` byte-locks
//! the prose/code normalization contracts and the content-digest
//! and fingerprint encodings: each prose and code vector renders
//! as `hex(input)` then `hex(normalized)`, each digest vector as
//! its lowercase hex digest. The committed
//! `source_graph_canonical_v1.golden` byte-locks the canonical
//! rendering of the fixture forest below. Regenerate both with
//! `EVIDENCE_UPDATE_FIXTURES=1`.
//!
//! The layout-independence test builds the fixture forest once,
//! writes it as two equivalent linked layouts — one sources file
//! and one graphs file versus split, record-reversed files — and
//! asserts both load to equal graphs and byte-identical canonical
//! renderings.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::{Path, PathBuf};

use evidence_core::corpus::{
    CorpusGraph, CorpusIndex, SourceNode, SourceNodeKind, content_digest, fingerprint,
    normalize_code, normalize_prose, render_source_graph_canonical,
};

const REV_A: &str = "src_00000000-0000-4000-8000-0000000000a1";
const NODE_A: &str = "snode_00000000-0000-4000-8000-0000000000b1";
const NODE_B: &str = "snode_00000000-0000-4000-8000-0000000000b2";
const NODE_C: &str = "snode_00000000-0000-4000-8000-0000000000b3";
const NODE_D: &str = "snode_00000000-0000-4000-8000-0000000000b4";
const NODE_E: &str = "snode_00000000-0000-4000-8000-0000000000b5";
const NODE_F: &str = "snode_00000000-0000-4000-8000-0000000000b6";
const NODE_G: &str = "snode_00000000-0000-4000-8000-0000000000b7";
const NODE_H: &str = "snode_00000000-0000-4000-8000-0000000000b8";

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus")
}

/// The prose normalization vectors: input strings exercising
/// folding, trimming, NFC composition, and mixed whitespace.
const PROSE_VECTORS: &[&str] = &[
    "  Hello   World  ",
    "e\u{301}lan  est\u{3000}tre\u{3000}s  bon",
    "a\r\nb\tc  d\ne",
    " \t\n ",
];

/// The code normalization vectors: CRLF/CR mapping, significant
/// spaces, blank lines, NFC composition.
const CODE_VECTORS: &[&str] = &[
    "fn main() {\r\n    x();\r\n}\r\n",
    "  keep  \n\n\ne\u{301}  ",
];

/// Render every golden normalization vector line in order.
fn render_normalization_golden() -> String {
    let mut out = String::new();
    for input in PROSE_VECTORS {
        out.push_str(&hex::encode(input));
        out.push('\n');
        out.push_str(&hex::encode(normalize_prose(input)));
        out.push('\n');
    }
    for input in CODE_VECTORS {
        out.push_str(&hex::encode(input));
        out.push('\n');
        out.push_str(&hex::encode(normalize_code(input)));
        out.push('\n');
    }
    for (kind, text) in [
        (SourceNodeKind::Paragraph, "First prose."),
        (SourceNodeKind::CodeBlock, "fn main() {\n    x();\n}\n"),
        (SourceNodeKind::Section, ""),
    ] {
        out.push_str(content_digest(kind, text).as_str());
        out.push('\n');
    }
    let ancestry = [(SourceNodeKind::Section, Some("1 Introduction"))];
    out.push_str(fingerprint(SourceNodeKind::Section, Some("1 Introduction"), &[]).as_str());
    out.push('\n');
    out.push_str(fingerprint(SourceNodeKind::Paragraph, None, &ancestry).as_str());
    out.push('\n');
    out.push_str(
        fingerprint(
            SourceNodeKind::CodeBlock,
            None,
            &[
                (SourceNodeKind::Section, Some("1 Introduction")),
                (SourceNodeKind::Section, Some("1.1 Details")),
            ],
        )
        .as_str(),
    );
    out.push('\n');
    out
}

#[test]
fn golden_normalization_vectors_byte_lock_prose_code_and_digests() {
    let rendered = render_normalization_golden();
    let path = fixture_dir().join("source_graph_normalization_v1.golden");
    if std::env::var_os("EVIDENCE_UPDATE_FIXTURES").is_some() {
        fs::write(&path, &rendered).expect("write fixture");
        return;
    }
    let committed = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing or unreadable fixture {}: {e}\n\
             hint: run with EVIDENCE_UPDATE_FIXTURES=1 to write it",
            path.display()
        )
    });
    assert_eq!(
        committed, rendered,
        "normalization or digest contracts drifted — they are byte-locked; \
         an intentional change requires a new encoding version"
    );
}

// ---------------------------------------------------------------
// Layout independence (TEST-172)
// ---------------------------------------------------------------

/// One fixture-forest node specification, in build order (parents
/// before children).
struct NodeSpec {
    uid: &'static str,
    parent: Option<&'static str>,
    kind: SourceNodeKind,
    ordinal: u32,
    label: Option<&'static str>,
    text: &'static str,
    anchor: Option<&'static str>,
    byte_range: (u64, u64),
}

/// The fixture forest: a section with prose and code children and
/// a nested subsection, plus a table tree.
fn fixture_forest() -> Vec<SourceNode> {
    let specs = [
        NodeSpec {
            uid: NODE_A,
            parent: None,
            kind: SourceNodeKind::Section,
            ordinal: 0,
            label: Some("1 Introduction"),
            text: "",
            anchor: Some("sec-1"),
            byte_range: (0, 120),
        },
        NodeSpec {
            uid: NODE_B,
            parent: Some(NODE_A),
            kind: SourceNodeKind::Paragraph,
            ordinal: 0,
            label: None,
            text: "First prose.",
            anchor: None,
            byte_range: (121, 133),
        },
        NodeSpec {
            uid: NODE_C,
            parent: Some(NODE_A),
            kind: SourceNodeKind::CodeBlock,
            ordinal: 1,
            label: None,
            text: "fn main() {\n    x();\n}\n",
            anchor: None,
            byte_range: (134, 160),
        },
        NodeSpec {
            uid: NODE_D,
            parent: Some(NODE_A),
            kind: SourceNodeKind::Section,
            ordinal: 2,
            label: Some("1.1 Details"),
            text: "",
            anchor: None,
            byte_range: (161, 175),
        },
        NodeSpec {
            uid: NODE_E,
            parent: Some(NODE_D),
            kind: SourceNodeKind::Note,
            ordinal: 0,
            label: None,
            text: "A note.",
            anchor: None,
            byte_range: (176, 183),
        },
        NodeSpec {
            uid: NODE_F,
            parent: None,
            kind: SourceNodeKind::Table,
            ordinal: 1,
            label: None,
            text: "",
            anchor: None,
            byte_range: (184, 220),
        },
        NodeSpec {
            uid: NODE_G,
            parent: Some(NODE_F),
            kind: SourceNodeKind::TableRow,
            ordinal: 0,
            label: None,
            text: "",
            anchor: None,
            byte_range: (185, 210),
        },
        NodeSpec {
            uid: NODE_H,
            parent: Some(NODE_G),
            kind: SourceNodeKind::TableCell,
            ordinal: 0,
            label: None,
            text: "cell text",
            anchor: None,
            byte_range: (186, 195),
        },
    ];
    let mut built: Vec<SourceNode> = Vec::new();
    for spec in &specs {
        let mut ancestry = Vec::new();
        let mut current = spec.parent;
        while let Some(uid) = current {
            let node = built
                .iter()
                .find(|node| node.uid == uid)
                .expect("parent built first");
            ancestry.push((node.kind, node.label.clone()));
            current = node.parent_uid.as_deref();
        }
        ancestry.reverse();
        let ancestry_refs: Vec<(SourceNodeKind, Option<&str>)> = ancestry
            .iter()
            .map(|(kind, label)| (*kind, label.as_deref()))
            .collect();
        built.push(SourceNode {
            uid: spec.uid.to_string(),
            source_revision_uid: REV_A.to_string(),
            parent_uid: spec.parent.map(str::to_string),
            kind: spec.kind,
            ordinal: spec.ordinal,
            label: spec.label.map(str::to_string),
            canonical_text: spec.text.to_string(),
            content_sha256: content_digest(spec.kind, spec.text),
            fingerprint: fingerprint(spec.kind, spec.label, &ancestry_refs),
            locator: evidence_core::corpus::SourceLocator::Markdown {
                path: evidence_core::corpus::SafeRelPath::new("docs/spec.md").expect("safe path"),
                git_blob: None,
                anchor: spec.anchor.map(str::to_string),
                heading_path: Vec::new(),
                byte_range: spec.byte_range,
            },
        });
    }
    built
}

/// Serialize one node as a `[[nodes]]` TOML record.
fn node_record_toml(node: &SourceNode) -> String {
    let mut out = String::from("[[nodes]]\n");
    out.push_str(&format!("uid = \"{}\"\n", node.uid));
    out.push_str(&format!(
        "source_revision_uid = \"{}\"\n",
        node.source_revision_uid
    ));
    if let Some(parent) = &node.parent_uid {
        out.push_str(&format!("parent_uid = \"{parent}\"\n"));
    }
    out.push_str(&format!("kind = \"{}\"\n", node.kind.as_str()));
    out.push_str(&format!("ordinal = {}\n", node.ordinal));
    if let Some(label) = &node.label {
        out.push_str(&format!("label = \"{label}\"\n"));
    }
    out.push_str(&format!(
        "canonical_text = {}\n",
        toml_basic(&node.canonical_text)
    ));
    out.push_str(&format!("content_sha256 = \"{}\"\n", node.content_sha256));
    out.push_str(&format!("fingerprint = \"{}\"\n", node.fingerprint));
    let evidence_core::corpus::SourceLocator::Markdown {
        anchor, byte_range, ..
    } = &node.locator
    else {
        panic!("fixture forest carries markdown locators only");
    };
    out.push_str("\n[nodes.locator]\nformat = \"markdown\"\npath = \"docs/spec.md\"\n");
    if let Some(anchor) = anchor {
        out.push_str(&format!("anchor = \"{anchor}\"\n"));
    }
    out.push_str(&format!(
        "byte_range = [{}, {}]\n",
        byte_range.0, byte_range.1
    ));
    out
}

/// Minimal TOML basic-string escaping for the fixture texts.
fn toml_basic(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

const SOURCE_TOML: &str = r#"schema_version = 1

[[sources]]
uid = "src_00000000-0000-4000-8000-0000000000a1"
id = "SRC-1"
document_key = "DOC-1"
title = "spec rev C"
media_type = "text/markdown"
canonical_location = "https://example.org/specs/DOC-1/rev-c"

[sources.material]
state = "available"
retrieved_at = "2026-07-01T10:00:00Z"
sha256 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

[sources.material.capture]
mode = "hash_only"
"#;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

/// Write the forest as two equivalent linked layouts and load
/// both. Layout A: one sources file, one graphs file, records in
/// forest order. Layout B: the source under a glob, graphs split
/// across two files with records reversed within each.
fn load_both_layouts(forest: &[SourceNode]) -> (CorpusGraph, CorpusGraph) {
    let dir_a = tempfile::tempdir().expect("tempdir");
    write(&dir_a.path().join("sources.toml"), SOURCE_TOML);
    let mut graphs = String::from("schema_version = 1\n\n");
    for node in forest {
        graphs.push_str(&node_record_toml(node));
        graphs.push('\n');
    }
    write(&dir_a.path().join("graphs.toml"), &graphs);
    write(
        &dir_a.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources.toml\"]\nsource_graphs = [\"graphs.toml\"]\n",
    );

    let dir_b = tempfile::tempdir().expect("tempdir");
    write(&dir_b.path().join("sources/doc.toml"), SOURCE_TOML);
    let mut first = String::from("schema_version = 1\n\n");
    for node in forest[..4].iter().rev() {
        first.push_str(&node_record_toml(node));
        first.push('\n');
    }
    let mut second = String::from("schema_version = 1\n\n");
    for node in forest[4..].iter().rev() {
        second.push_str(&node_record_toml(node));
        second.push('\n');
    }
    write(&dir_b.path().join("graphs/z.toml"), &first);
    write(&dir_b.path().join("graphs/a.toml"), &second);
    write(
        &dir_b.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources/**/*.toml\"]\nsource_graphs = [\"graphs/**/*.toml\"]\n",
    );

    let graph_a =
        CorpusIndex::load_graph(&dir_a.path().join("corpus.toml")).expect("layout A loads");
    let graph_b =
        CorpusIndex::load_graph(&dir_b.path().join("corpus.toml")).expect("layout B loads");
    (graph_a, graph_b)
}

#[test]
fn reordered_layouts_load_equal_graphs_and_identical_canonical_bytes() {
    let (graph_a, graph_b) = load_both_layouts(&fixture_forest());
    assert_eq!(
        graph_a, graph_b,
        "equivalent linked layouts load to equal graphs"
    );
    let bytes_a = render_source_graph_canonical(&graph_a);
    let bytes_b = render_source_graph_canonical(&graph_b);
    assert_eq!(
        bytes_a, bytes_b,
        "equivalent layouts render byte-identical canonical forms"
    );

    let path = fixture_dir().join("source_graph_canonical_v1.golden");
    if std::env::var_os("EVIDENCE_UPDATE_FIXTURES").is_some() {
        fs::write(&path, &bytes_a).expect("write fixture");
        return;
    }
    let committed = fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "missing or unreadable fixture {}: {e}\n\
             hint: run with EVIDENCE_UPDATE_FIXTURES=1 to write it",
            path.display()
        )
    });
    assert_eq!(
        committed, bytes_a,
        "the canonical rendering drifted — the ordering contract is byte-locked"
    );
}
