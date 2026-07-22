//! Tests for the closed per-format locator schema: variant
//! round-trips, malformed fields, mixed-format fields, and
//! locator/media agreement (TEST-171).

use super::error::SourceGraphError;
use super::locator::{LocatorRule, SourceLocator};
use super::records;
use crate::corpus::graph::CorpusGraph;

const REV_A: &str = "src_00000000-0000-4000-8000-0000000000a1";
const NODE_A: &str = "snode_00000000-0000-4000-8000-0000000000b1";
const NODE_B: &str = "snode_00000000-0000-4000-8000-0000000000b2";
const NODE_C: &str = "snode_00000000-0000-4000-8000-0000000000b3";
const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn file_with_locator(uid: &str, locator: &str) -> String {
    format!(
        r#"schema_version = 1

[[nodes]]
uid = "{uid}"
source_revision_uid = "{REV_A}"
kind = "paragraph"
ordinal = 0
canonical_text = "text"
content_sha256 = "{DIGEST_A}"
fingerprint = "{DIGEST_B}"

{locator}
"#
    )
}

fn load_content(content: &str) -> Result<CorpusGraph, SourceGraphError> {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("graphs.toml");
    std::fs::write(&path, content).expect("write records");
    let mut graph = CorpusGraph::new();
    records::load_source_graphs_into(&path, &mut graph)?;
    Ok(graph)
}

fn load_locator(uid: &str, locator: &str) -> Result<SourceLocator, SourceGraphError> {
    let graph = load_content(&file_with_locator(uid, locator))?;
    Ok(graph
        .source_graph(REV_A)
        .expect("revision graph")
        .get(uid)
        .expect("node")
        .locator
        .clone())
}

/// Every locator variant round-trips with every field populated
/// (TEST-171).
#[test]
fn markdown_html_pdf_locators_round_trip() {
    let markdown = load_locator(
        NODE_A,
        r#"[nodes.locator]
format = "markdown"
path = "docs/spec.md"
git_blob = "0123456789abcdef0123456789abcdef01234567"
anchor = "sec-1"
heading_path = ["Specification", "1 Introduction"]
byte_range = [0, 120]
"#,
    )
    .expect("markdown locator loads");
    let SourceLocator::Markdown {
        path,
        git_blob,
        anchor,
        heading_path,
        byte_range,
    } = markdown
    else {
        panic!("expected a markdown locator");
    };
    assert_eq!(path.as_str(), "docs/spec.md");
    assert_eq!(
        git_blob.as_deref(),
        Some("0123456789abcdef0123456789abcdef01234567")
    );
    assert_eq!(anchor.as_deref(), Some("sec-1"));
    assert_eq!(
        heading_path,
        vec!["Specification".to_string(), "1 Introduction".to_string()]
    );
    assert_eq!(byte_range, (0, 120));

    let html = load_locator(
        NODE_B,
        r#"[nodes.locator]
format = "html"
canonical_url = "https://example.org/specs/DOC-1"
final_url = "https://example.org/specs/DOC-1/latest"
fragment = "sec-2"
heading_path = ["Specification"]
dom_path = [0, 2, 1]
"#,
    )
    .expect("html locator loads");
    let SourceLocator::Html {
        canonical_url,
        final_url,
        fragment,
        heading_path,
        dom_path,
    } = html
    else {
        panic!("expected an html locator");
    };
    assert_eq!(canonical_url, "https://example.org/specs/DOC-1");
    assert_eq!(
        final_url.as_deref(),
        Some("https://example.org/specs/DOC-1/latest")
    );
    assert_eq!(fragment.as_deref(), Some("sec-2"));
    assert_eq!(heading_path, vec!["Specification".to_string()]);
    assert_eq!(dom_path, vec![0, 2, 1]);

    let pdf = load_locator(
        NODE_C,
        r#"[nodes.locator]
format = "pdf"
physical_page = 7
printed_label = "vii"
bbox = [72.0, 100.5, 300.25, 700.0]
"#,
    )
    .expect("pdf locator loads");
    let SourceLocator::Pdf {
        physical_page,
        printed_label,
        bbox,
    } = pdf
    else {
        panic!("expected a pdf locator");
    };
    assert_eq!(physical_page, 7);
    assert_eq!(printed_label.as_deref(), Some("vii"));
    assert_eq!(bbox, [72.0, 100.5, 300.25, 700.0]);

    // Minimal forms load too: optionals absent.
    let minimal = load_locator(
        NODE_A,
        r#"[nodes.locator]
format = "pdf"
physical_page = 1
bbox = [0.0, 0.0, 1.0, 1.0]
"#,
    )
    .expect("minimal pdf locator loads");
    assert!(
        matches!(
            minimal,
            SourceLocator::Pdf {
                physical_page: 1,
                printed_label: None,
                ..
            }
        ),
        "minimal pdf locator must carry no printed label"
    );
}

/// Malformed per-variant fields fail closed with the typed rule,
/// and unsafe paths fail at deserialization (TEST-171).
#[test]
fn malformed_locator_fields_fail_closed() {
    let cases: [(&str, &str, LocatorRule); 13] = [
        (
            "reversed byte range",
            "[nodes.locator]\nformat = \"markdown\"\npath = \"docs/spec.md\"\nbyte_range = [9, 3]\n",
            LocatorRule::ByteRangeReversed,
        ),
        (
            "short git blob",
            "[nodes.locator]\nformat = \"markdown\"\npath = \"docs/spec.md\"\ngit_blob = \"0123ab\"\nbyte_range = [0, 1]\n",
            LocatorRule::GitBlobHex,
        ),
        (
            "blank anchor",
            "[nodes.locator]\nformat = \"markdown\"\npath = \"docs/spec.md\"\nanchor = \"  \"\nbyte_range = [0, 1]\n",
            LocatorRule::Blank,
        ),
        (
            "blank heading component",
            "[nodes.locator]\nformat = \"markdown\"\npath = \"docs/spec.md\"\nheading_path = [\"Spec\", \"\"]\nbyte_range = [0, 1]\n",
            LocatorRule::Blank,
        ),
        (
            "schemeless canonical url",
            "[nodes.locator]\nformat = \"html\"\ncanonical_url = \"example.org/spec\"\n",
            LocatorRule::UrlScheme,
        ),
        (
            "schemeless final url",
            "[nodes.locator]\nformat = \"html\"\ncanonical_url = \"https://example.org/\"\nfinal_url = \"example.org/x\"\n",
            LocatorRule::UrlScheme,
        ),
        (
            "whitespace url",
            "[nodes.locator]\nformat = \"html\"\ncanonical_url = \"https://example.org/a b\"\n",
            LocatorRule::UrlScheme,
        ),
        (
            "blank fragment",
            "[nodes.locator]\nformat = \"html\"\ncanonical_url = \"https://example.org/\"\nfragment = \" \"\n",
            LocatorRule::Blank,
        ),
        (
            "zero page",
            "[nodes.locator]\nformat = \"pdf\"\nphysical_page = 0\nbbox = [0.0, 0.0, 1.0, 1.0]\n",
            LocatorRule::PageZero,
        ),
        (
            "blank printed label",
            "[nodes.locator]\nformat = \"pdf\"\nphysical_page = 1\nprinted_label = \" \"\nbbox = [0.0, 0.0, 1.0, 1.0]\n",
            LocatorRule::Blank,
        ),
        (
            "non-finite bbox",
            "[nodes.locator]\nformat = \"pdf\"\nphysical_page = 1\nbbox = [0.0, inf, 1.0, 1.0]\n",
            LocatorRule::BboxNonFinite,
        ),
        (
            "negative bbox",
            "[nodes.locator]\nformat = \"pdf\"\nphysical_page = 1\nbbox = [-1.0, 0.0, 1.0, 1.0]\n",
            LocatorRule::BboxNegative,
        ),
        (
            "reversed bbox",
            "[nodes.locator]\nformat = \"pdf\"\nphysical_page = 1\nbbox = [5.0, 0.0, 1.0, 1.0]\n",
            LocatorRule::BboxReversed,
        ),
    ];
    for (name, locator, rule) in cases {
        let err = load_content(&file_with_locator(NODE_A, locator))
            .expect_err("malformed locator must fail closed");
        assert!(
            matches!(
                err,
                SourceGraphError::InvalidLocatorField {
                    rule: actual,
                    ref node_uid,
                    ..
                } if actual == rule && node_uid == NODE_A
            ),
            "{name}: expected InvalidLocatorField with {rule}, got: {err:?}"
        );
    }

    // Unsafe paths fail at deserialization through the validating
    // SafeRelPath newtype. `a\\b.md` doubles the backslash so the
    // TOML basic string decodes to a backslash-bearing value.
    for path in ["../outside.md", "/absolute.md", "a//b.md", "a\\\\b.md", ""] {
        let locator = format!(
            "[nodes.locator]\nformat = \"markdown\"\npath = \"{path}\"\nbyte_range = [0, 1]\n"
        );
        let err = load_content(&file_with_locator(NODE_A, &locator))
            .expect_err("unsafe path must fail closed");
        assert!(
            matches!(err, SourceGraphError::RecordParse { .. }),
            "path {path:?}: expected RecordParse, got: {err:?}"
        );
    }
}

/// A field from another variant's schema — or an unknown format
/// tag — fails deserialization, so mixed-format locators can
/// never load (TEST-171).
#[test]
fn mixed_format_fields_fail_closed() {
    let cases: [&str; 4] = [
        // DOM path on a markdown locator.
        "[nodes.locator]\nformat = \"markdown\"\npath = \"docs/spec.md\"\nbyte_range = [0, 1]\ndom_path = [0]\n",
        // Byte range on an html locator.
        "[nodes.locator]\nformat = \"html\"\ncanonical_url = \"https://example.org/\"\nbyte_range = [0, 1]\n",
        // Filesystem path on a pdf locator.
        "[nodes.locator]\nformat = \"pdf\"\nphysical_page = 1\nbbox = [0.0, 0.0, 1.0, 1.0]\npath = \"docs/spec.md\"\n",
        // An unknown format tag.
        "[nodes.locator]\nformat = \"docx\"\npath = \"docs/spec.md\"\n",
    ];
    for locator in cases {
        let err = load_content(&file_with_locator(NODE_A, locator))
            .expect_err("mixed-format locator must fail closed");
        assert!(
            matches!(err, SourceGraphError::RecordParse { .. }),
            "expected RecordParse for {locator:?}, got: {err:?}"
        );
    }
}

/// A locator variant that disagrees with the revision's declared
/// media type fails validation (TEST-171).
#[test]
fn locator_media_mismatch_fails_closed() {
    use crate::corpus::source_graph::normalization::{content_digest, fingerprint};
    use crate::corpus::{Node, SourceMaterial, SourceNode, SourceNodeKind, SourceRevisionNode};

    fn revision(uid: &str, media_type: &str) -> Node {
        Node::SourceRevision(SourceRevisionNode {
            uid: uid.to_string(),
            id: format!("REV-{uid}"),
            document_key: format!("DOC-{uid}"),
            title: "spec".to_string(),
            media_type: media_type.to_string(),
            canonical_location: "https://example.org/spec".to_string(),
            material: SourceMaterial::Unavailable {
                reason: "test fixture".to_string(),
            },
            edges: Vec::new(),
        })
    }

    let mut graph = CorpusGraph::new();
    graph
        .insert(revision(REV_A, "text/markdown"))
        .expect("insert revision");
    let text = "prose";
    graph
        .insert_source_node(SourceNode {
            uid: NODE_A.to_string(),
            source_revision_uid: REV_A.to_string(),
            parent_uid: None,
            kind: SourceNodeKind::Paragraph,
            ordinal: 0,
            label: None,
            canonical_text: text.to_string(),
            content_sha256: content_digest(SourceNodeKind::Paragraph, text),
            fingerprint: fingerprint(SourceNodeKind::Paragraph, None, &[]),
            locator: SourceLocator::Pdf {
                physical_page: 2,
                printed_label: None,
                bbox: [0.0, 0.0, 10.0, 10.0],
            },
        })
        .expect("insert node");
    let err = graph.validate().expect_err("media mismatch must fail");
    match err {
        crate::corpus::CorpusError::SourceGraph(SourceGraphError::LocatorMediaMismatch {
            revision_uid,
            node_uid,
            locator_format,
            media_type,
        }) => {
            assert_eq!(revision_uid, REV_A);
            assert_eq!(node_uid, NODE_A);
            assert_eq!(locator_format, "pdf");
            assert_eq!(media_type, "text/markdown");
        }
        other => panic!("expected LocatorMediaMismatch through the corpus wrapper, got: {other:?}"),
    }

    // Case-insensitive agreement passes.
    let mut graph = CorpusGraph::new();
    graph
        .insert(revision(REV_A, "TEXT/MARKDOWN"))
        .expect("insert revision");
    graph
        .insert_source_node(SourceNode {
            uid: NODE_A.to_string(),
            source_revision_uid: REV_A.to_string(),
            parent_uid: None,
            kind: SourceNodeKind::Paragraph,
            ordinal: 0,
            label: None,
            canonical_text: text.to_string(),
            content_sha256: content_digest(SourceNodeKind::Paragraph, text),
            fingerprint: fingerprint(SourceNodeKind::Paragraph, None, &[]),
            locator: SourceLocator::Markdown {
                path: crate::corpus::SafeRelPath::new("docs/spec.md").expect("safe path"),
                git_blob: None,
                anchor: None,
                heading_path: Vec::new(),
                byte_range: (0, 5),
            },
        })
        .expect("insert node");
    graph
        .validate()
        .expect("media agreement is case-insensitive");
}
