//! Unit tests for the HTML ingestion adapter: the recipe identity
//! (TEST-180), the structure mapping (TEST-182), and the
//! diagnostics and fail-closed contract (TEST-183).

use std::collections::BTreeSet;

use super::super::super::digest::StructuralContentDigest;
use super::{
    HtmlIngestDiagnosticKind, HtmlIngestError, HtmlIngestion, HtmlIngestionRecipe, IngestHtmlInput,
    ingest_html,
};
use crate::corpus::{SourceNode, SourceNodeKind};
use crate::hash::sha256;

const REV: &str = "src_00000000-0000-4000-8000-0000000000c2";
const URL: &str = "https://example.org/spec/oidc-like.html";

fn recipe() -> HtmlIngestionRecipe {
    HtmlIngestionRecipe {
        parser: "scraper".to_string(),
        parser_version: "0.27.0".to_string(),
        adapter_version: "1".to_string(),
        normalization_contract: "evidence/source-node-normalization/v1".to_string(),
        encoding: "utf-8".to_string(),
        inclusion_root: None,
        exclusion_selectors: BTreeSet::new(),
        note_selectors: ["div.note".to_string()].into_iter().collect(),
        figure_caption_selectors: BTreeSet::new(),
        compatibility_modes: BTreeSet::new(),
    }
}

fn input_with(bytes: &[u8], recipe: HtmlIngestionRecipe) -> IngestHtmlInput<'_> {
    IngestHtmlInput {
        bytes,
        media_type: super::HTML_MEDIA_TYPE,
        source_revision_uid: REV,
        canonical_url: URL,
        final_url: None,
        input_digest: StructuralContentDigest::from_hex(&sha256(bytes)).expect("sha256 hex"),
        recipe,
    }
}

fn input(bytes: &[u8]) -> IngestHtmlInput<'_> {
    input_with(bytes, recipe())
}

fn ingest(html: &str) -> HtmlIngestion {
    ingest_html(&input(html.as_bytes())).expect("ingestion succeeds")
}

fn kind_texts(outcome: &HtmlIngestion, kind: SourceNodeKind) -> Vec<&str> {
    outcome
        .nodes
        .iter()
        .filter(|node| node.kind == kind)
        .map(|node| node.canonical_text.as_str())
        .collect()
}

fn find<'n>(nodes: &'n [SourceNode], kind: SourceNodeKind, text: &str) -> &'n SourceNode {
    nodes
        .iter()
        .find(|node| node.kind == kind && node.canonical_text == text)
        .unwrap_or_else(|| panic!("expected a {kind:?} node with text {text:?}"))
}

// TEST-180

#[test]
fn recipe_canonical_bytes_bind_every_field_deterministically() {
    let baseline = recipe();
    let mut changed = baseline.clone();
    changed.adapter_version = "2".to_string();
    assert_ne!(
        baseline.digest(),
        changed.digest(),
        "a field change moves the recipe identity"
    );
    assert_eq!(
        baseline.canonical_bytes(),
        recipe().canonical_bytes(),
        "the encoding is deterministic for equal recipes"
    );
    assert!(
        baseline
            .canonical_bytes()
            .starts_with(b"evidence/html-ingester-recipe/v1\0"),
        "the encoding carries its domain tag"
    );
}

#[test]
fn recipe_set_order_is_non_semantic() {
    let mut first = recipe();
    first.exclusion_selectors = ["nav".to_string(), "footer".to_string()]
        .into_iter()
        .collect();
    let mut second = recipe();
    second.exclusion_selectors = ["footer".to_string(), "nav".to_string()]
        .into_iter()
        .collect();
    assert_eq!(first.digest(), second.digest());
}

#[test]
fn recipe_selector_changes_move_recipe_identity() {
    let open = recipe();
    let mut closed = recipe();
    closed.exclusion_selectors = ["nav.site-nav".to_string()].into_iter().collect();
    assert_ne!(
        open.digest(),
        closed.digest(),
        "an exclusion-selector change moves the recipe identity plane"
    );

    let html = r#"<!DOCTYPE html><html><body>
        <nav class="site-nav"><p>Navigation only.</p></nav>
        <h1 id="s">Section</h1><p>Normative text.</p>
        </body></html>"#;
    let retained = ingest_html(&input_with(html.as_bytes(), open)).expect("ingestion succeeds");
    let pruned = ingest_html(&input_with(html.as_bytes(), closed)).expect("ingestion succeeds");
    assert!(
        retained
            .nodes
            .iter()
            .any(|node| node.canonical_text == "Navigation only."),
        "without the exclusion the nav content projects"
    );
    assert!(
        !pruned
            .nodes
            .iter()
            .any(|node| node.canonical_text == "Navigation only."),
        "with the exclusion the nav content is absent from the nodes"
    );
    assert_ne!(
        retained.output_digest, pruned.output_digest,
        "the recipe change moves the comparison output"
    );
    assert_eq!(pruned.diagnostics.len(), 1);
    assert!(matches!(
        pruned.diagnostics[0].kind,
        HtmlIngestDiagnosticKind::ExcludedByRecipe { .. }
    ));
}

// TEST-182

#[test]
fn headings_lists_definitions_and_code_project_structure() {
    let outcome = ingest(
        r#"<!DOCTYPE html><html><body>
        <h1 id="top">1. Top</h1>
        <p>Intro paragraph.</p>
        <h2>1.1. Sub</h2>
        <ul><li>Alpha<ul><li>Alpha one</li></ul></li><li>Beta</li></ul>
        <dl><dt>Term</dt><dd>Its definition.</dd></dl>
        <pre>literal  spacing
  kept</pre>
        </body></html>"#,
    );
    let top = find(&outcome.nodes, SourceNodeKind::Section, "1. Top");
    let sub = find(&outcome.nodes, SourceNodeKind::Section, "1.1. Sub");
    assert_eq!(top.parent_uid, None, "the h1 roots");
    assert_eq!(
        sub.parent_uid.as_deref(),
        Some(top.uid.as_str()),
        "the h2 nests under the h1"
    );
    let top_fragment = match &top.locator {
        crate::corpus::SourceLocator::Html { fragment, .. } => fragment.clone(),
        other => panic!("expected an HTML locator, got {other:?}"),
    };
    assert_eq!(top_fragment.as_deref(), Some("top"));
    let sub_fragment = match &sub.locator {
        crate::corpus::SourceLocator::Html { fragment, .. } => fragment.clone(),
        other => panic!("expected an HTML locator, got {other:?}"),
    };
    assert_eq!(
        sub_fragment.as_deref(),
        Some("1-1-sub"),
        "an id-less heading slugs deterministically"
    );
    assert_eq!(
        kind_texts(&outcome, SourceNodeKind::ListItem),
        vec!["Alpha", "Alpha one", "Beta"],
        "nested list items project flat in document order"
    );
    let term = find(&outcome.nodes, SourceNodeKind::DefinitionTerm, "Term");
    assert_eq!(term.label.as_deref(), Some("Term"));
    assert_eq!(
        kind_texts(&outcome, SourceNodeKind::DefinitionBody),
        vec!["Its definition."]
    );
    assert_eq!(
        kind_texts(&outcome, SourceNodeKind::CodeBlock),
        vec!["literal  spacing\n  kept"],
        "pre content keeps significant whitespace"
    );
    assert!(
        outcome.diagnostics.is_empty(),
        "the document projects losslessly: {:?}",
        outcome.diagnostics
    );
}

#[test]
fn tables_project_header_rows_and_span_slots() {
    let outcome = ingest(
        r#"<!DOCTYPE html><html><body>
        <h1 id="t">Table section</h1>
        <table>
          <thead><tr><th>Claim</th><th colspan="2">Presence</th></tr></thead>
          <tbody>
            <tr><td rowspan="2">sub</td><td>required</td><td>always</td></tr>
            <tr><td>type</td><td>string</td></tr>
          </tbody>
        </table>
        </body></html>"#,
    );
    let table = outcome
        .nodes
        .iter()
        .find(|node| node.kind == SourceNodeKind::Table)
        .expect("a table node");
    let rows: Vec<&SourceNode> = outcome
        .nodes
        .iter()
        .filter(|node| {
            node.kind == SourceNodeKind::TableRow
                && node.parent_uid.as_deref() == Some(table.uid.as_str())
        })
        .collect();
    assert_eq!(rows.len(), 3, "three rows project");
    assert_eq!(
        rows[0].label.as_deref(),
        Some("header 1"),
        "the thead row carries the header-row label"
    );
    assert_eq!(rows[1].label, None);
    let cells_of = |row: &SourceNode| -> Vec<&str> {
        outcome
            .nodes
            .iter()
            .filter(|node| {
                node.kind == SourceNodeKind::TableCell
                    && node.parent_uid.as_deref() == Some(row.uid.as_str())
            })
            .map(|node| node.canonical_text.as_str())
            .collect()
    };
    assert_eq!(
        cells_of(rows[0]),
        vec!["Claim", "Presence", ""],
        "the colspan projects one cell per covered slot"
    );
    assert_eq!(cells_of(rows[1]), vec!["sub", "required", "always"]);
    assert_eq!(
        cells_of(rows[2]),
        vec!["", "type", "string"],
        "the rowspan covers its column with an empty continuation cell"
    );
}

#[test]
fn notes_figures_fragments_and_dom_paths_project() {
    let outcome = ingest(
        r#"<!DOCTYPE html><html><body>
        <h1 id="sec">Section</h1>
        <div class="note"><p>Advisory text.</p></div>
        <figure><figcaption>Figure 1: Flow</figcaption></figure>
        </body></html>"#,
    );
    assert_eq!(
        kind_texts(&outcome, SourceNodeKind::Note),
        vec!["Advisory text."]
    );
    assert_eq!(
        kind_texts(&outcome, SourceNodeKind::FigureCaption),
        vec!["Figure 1: Flow"]
    );
    let note = find(&outcome.nodes, SourceNodeKind::Note, "Advisory text.");
    match &note.locator {
        crate::corpus::SourceLocator::Html {
            canonical_url,
            heading_path,
            dom_path,
            fragment,
            ..
        } => {
            assert_eq!(canonical_url, URL);
            assert_eq!(heading_path, &vec!["Section".to_string()]);
            assert_eq!(fragment, &None);
            assert!(
                dom_path.len() >= 3,
                "the DOM path locates the note below html/body: {dom_path:?}"
            );
        }
        other => panic!("expected an HTML locator, got {other:?}"),
    }
    let section = find(&outcome.nodes, SourceNodeKind::Section, "Section");
    assert_eq!(
        note.parent_uid.as_deref(),
        Some(section.uid.as_str()),
        "the note attaches to the enclosing section"
    );
}

// TEST-183

#[test]
fn exclusions_and_closed_rule_drops_produce_sorted_diagnostics() {
    let mut recipe = recipe();
    recipe.exclusion_selectors = ["nav.site-nav".to_string()].into_iter().collect();
    let outcome = ingest_html(&input_with(
        r#"<!DOCTYPE html><html><head><title>T</title><style>p{}</style></head><body>
        <nav class="site-nav"><p>Navigation.</p></nav>
        <h1 id="s">Section</h1>
        <p>Kept <x-unknown>content</x-unknown> here.</p>
        <script>var a = 1;</script>
        </body></html>"#
            .as_bytes(),
        recipe,
    ))
    .expect("ingestion succeeds");
    assert!(
        !outcome
            .nodes
            .iter()
            .any(|node| node.canonical_text.contains("Navigation.")),
        "excluded content is absent from the nodes"
    );
    let kinds: Vec<&HtmlIngestDiagnosticKind> = outcome
        .diagnostics
        .iter()
        .map(|diagnostic| &diagnostic.kind)
        .collect();
    assert_eq!(
        kinds.len(),
        4,
        "head, nav, script, and the unknown element diagnose: {kinds:?}"
    );
    assert!(
        kinds
            .iter()
            .any(|kind| matches!(kind, HtmlIngestDiagnosticKind::ExcludedByRecipe { selector } if selector == "nav.site-nav")),
        "the exclusion names its selector"
    );
    let drops = kinds
        .iter()
        .filter(|kind| matches!(kind, HtmlIngestDiagnosticKind::DroppedByClosedRule { .. }))
        .count();
    assert_eq!(drops, 2, "head and script drop by the closed rule");
    assert!(
        kinds
            .iter()
            .any(|kind| matches!(kind, HtmlIngestDiagnosticKind::UnsupportedElement { tag } if tag == "x-unknown")),
        "the unknown element diagnoses"
    );
    let sorted: Vec<(Vec<u32>, HtmlIngestDiagnosticKind, String)> = outcome
        .diagnostics
        .iter()
        .map(|d| (d.dom_path.clone(), d.kind.clone(), d.detail.clone()))
        .collect();
    let mut expected = sorted.clone();
    expected.sort();
    assert_eq!(
        sorted, expected,
        "diagnostics sort by DOM path, kind, detail"
    );
    let paragraph = find(
        &outcome.nodes,
        SourceNodeKind::Paragraph,
        "Kept content here.",
    );
    assert!(
        paragraph.canonical_text.contains("content"),
        "the unconfigured region's text is retained"
    );
}

#[test]
fn entity_and_resolution_constructs_fail_closed() {
    let html = r#"<!DOCTYPE html [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><html><body><p>x</p></body></html>"#;
    let error = ingest_html(&input(html.as_bytes())).expect_err("entity declarations fail closed");
    assert!(
        matches!(error, HtmlIngestError::ExternalResolution { .. }),
        "an external entity declaration fails closed: {error}"
    );

    let mut ids = recipe();
    ids.inclusion_root = Some("main.missing".to_string());
    let error = ingest_html(&input_with(
        b"<!DOCTYPE html><html><body><p>x</p></body></html>",
        ids,
    ))
    .expect_err("an unmatched inclusion root fails closed");
    assert!(
        matches!(
            error,
            HtmlIngestError::InclusionRootNotFound { ref selector } if selector == "main.missing"
        ),
        "the unmatched inclusion root carries its selector: {error}"
    );

    let mut bad = recipe();
    bad.exclusion_selectors = ["nav >> footer".to_string()].into_iter().collect();
    let error = ingest_html(&input_with(
        b"<!DOCTYPE html><html><body><p>x</p></body></html>",
        bad,
    ))
    .expect_err("an invalid selector fails closed");
    assert!(
        matches!(error, HtmlIngestError::InvalidSelector { .. }),
        "an invalid recipe selector fails closed: {error}"
    );
}

#[test]
fn encoding_size_depth_and_media_type_failures_carry_typed_context() {
    let mut wrong_media = input(b"<html></html>");
    wrong_media.media_type = "text/plain";
    let error = ingest_html(&wrong_media).expect_err("media type mismatch fails closed");
    assert!(
        matches!(error, HtmlIngestError::MediaTypeMismatch { ref found } if found == "text/plain"),
        "the media-type failure carries the asserted type: {error}"
    );

    let mut missing = recipe();
    missing.encoding = String::new();
    let error = ingest_html(&input_with(b"<html></html>", missing))
        .expect_err("a missing encoding declaration fails closed");
    assert!(
        matches!(error, HtmlIngestError::MissingEncoding),
        "a missing encoding fails closed: {error}"
    );

    let mut latin = recipe();
    latin.encoding = "iso-8859-1".to_string();
    let error = ingest_html(&input_with(b"<html></html>", latin))
        .expect_err("an unsupported encoding fails closed");
    assert!(
        matches!(
            error,
            HtmlIngestError::UnsupportedEncoding { ref encoding } if encoding == "iso-8859-1"
        ),
        "the encoding failure carries the declared label: {error}"
    );

    let bytes = b"<html>\xff\xfe</html>";
    let error = ingest_html(&input(bytes)).expect_err("non-UTF-8 input fails closed");
    assert!(
        matches!(error, HtmlIngestError::NonUtf8 { offset: 6 }),
        "the UTF-8 failure carries the first invalid offset: {error}"
    );

    let oversized = vec![b' '; super::MAX_INPUT_BYTES + 1];
    let error = ingest_html(&input(&oversized)).expect_err("oversized input fails closed");
    assert!(
        matches!(
            error,
            HtmlIngestError::InputTooLarge { size, limit }
                if size == super::MAX_INPUT_BYTES + 1 && limit == super::MAX_INPUT_BYTES
        ),
        "the size failure carries the size and the limit: {error}"
    );

    let deep = format!(
        "<!DOCTYPE html><html><body>{}{}</body></html>",
        "<div>".repeat(300),
        "</div>".repeat(300)
    );
    let error = ingest_html(&input(deep.as_bytes())).expect_err("deep nesting fails closed");
    assert!(
        matches!(
            error,
            HtmlIngestError::NestingTooDeep { depth, limit } if depth == limit + 1
        ),
        "the depth failure carries the depth and the limit: {error}"
    );
}
