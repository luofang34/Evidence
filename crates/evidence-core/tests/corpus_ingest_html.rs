//! HTML ingestion acceptance tests (TEST-181): the committed
//! fixture byte-locks its golden candidate projection, equivalent
//! serializations (attribute order, non-semantic whitespace)
//! produce equal output, and re-ingestion reuses committed uids
//! through structural-key reconciliation.
//!
//! The fixture `html_acceptance_v1.html` is an M4 acceptance
//! source — a small independently redistributable OIDC-shaped
//! HTML specification exercising Requirements Notation, normative
//! terminology definitions without capitalized requirement
//! keywords, heading ids and internal links, nested lists and a
//! definition list, a table with row and column spans, a pre/code
//! literal, note/example and figure-caption structure, closed-rule
//! metadata drops, and configured navigation/ToC/footer
//! exclusions. It is vendored — never fetched from a live site —
//! and is not a certification claim. The golden
//! `html_acceptance_v1.golden` byte-locks the canonical uid-free
//! candidate projection; regenerate with
//! `EVIDENCE_UPDATE_FIXTURES=1`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use evidence_core::corpus::{
    CandidateNode, HtmlIngestion, HtmlIngestionRecipe, IngestHtmlInput, SourceGraph, SourceNode,
    StructuralContentDigest, ingest_html, reconcile,
};
use evidence_core::hash::sha256;

const REV: &str = "src_00000000-0000-4000-8000-0000000000c2";
const CANONICAL_URL: &str = "https://example.org/spec/oidc-like.html";
const FINAL_URL: &str = "https://example.org/spec/oidc-like-final.html";

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus")
}

fn recipe() -> HtmlIngestionRecipe {
    HtmlIngestionRecipe {
        parser: "scraper".to_string(),
        parser_version: "0.27.0".to_string(),
        adapter_version: "1".to_string(),
        normalization_contract: "evidence/source-node-normalization/v1".to_string(),
        encoding: "utf-8".to_string(),
        inclusion_root: None,
        exclusion_selectors: [
            "div.toc".to_string(),
            "footer.site-footer".to_string(),
            "nav.site-nav".to_string(),
        ]
        .into_iter()
        .collect(),
        note_selectors: ["div.example".to_string(), "div.note".to_string()]
            .into_iter()
            .collect(),
        figure_caption_selectors: BTreeSet::new(),
        compatibility_modes: ["html5-optional-tags".to_string()].into_iter().collect(),
    }
}

fn fixture_input(bytes: &[u8]) -> IngestHtmlInput<'_> {
    IngestHtmlInput {
        bytes,
        media_type: "text/html",
        source_revision_uid: REV,
        canonical_url: CANONICAL_URL,
        final_url: Some(FINAL_URL.to_string()),
        input_digest: StructuralContentDigest::from_hex(&sha256(bytes)).expect("sha256 hex"),
        recipe: recipe(),
    }
}

fn ingest(bytes: &[u8]) -> HtmlIngestion {
    ingest_html(&fixture_input(bytes)).expect("fixture ingestion succeeds")
}

fn ingest_fixture() -> HtmlIngestion {
    let bytes = fs::read(fixture_dir().join("html_acceptance_v1.html")).expect("read fixture");
    ingest(&bytes)
}

/// The candidates of an ingestion, keyed by their minted uids —
/// the re-ingestion input shape.
fn candidates_of(nodes: &[SourceNode]) -> Vec<CandidateNode> {
    nodes
        .iter()
        .map(|node| CandidateNode {
            provisional_id: node.uid.clone(),
            parent_id: node.parent_uid.clone(),
            kind: node.kind,
            ordinal: node.ordinal,
            label: node.label.clone(),
            canonical_text: node.canonical_text.clone(),
            locator: node.locator.clone(),
        })
        .collect()
}

#[test]
fn golden_candidate_projection_byte_locks_canonical_bytes() {
    let outcome = ingest_fixture();
    assert!(
        outcome.nodes.len() >= 20,
        "the fixture exercises the full construct set: {} nodes",
        outcome.nodes.len()
    );

    // The configured exclusions are absent from the nodes and
    // present, sorted, in the structural-loss diagnostics — beside
    // the closed-rule head drop, the unsupported image and custom
    // element, and the one dangling internal link.
    let excluded = ["footer.site-footer", "div.toc", "nav.site-nav"];
    for selector in excluded {
        assert!(
            outcome.diagnostics.iter().any(|diagnostic| matches!(
                &diagnostic.kind,
                evidence_core::corpus::HtmlIngestDiagnosticKind::ExcludedByRecipe { selector: found }
                    if found == selector
            )),
            "the exclusion of {selector} diagnoses"
        );
    }
    assert!(
        !outcome.nodes.iter().any(|node| {
            node.canonical_text.contains("Table of Contents")
                || node
                    .canonical_text
                    .contains("Copyright Example Consortium.")
        }),
        "excluded ToC and footer content is absent from the nodes"
    );
    assert_eq!(
        outcome.diagnostics.len(),
        7,
        "head drop, three exclusions, img, custom element, and one dangling link diagnose: {:?}",
        outcome.diagnostics
    );
    let sorted: Vec<(Vec<u32>, String)> = outcome
        .diagnostics
        .iter()
        .map(|d| (d.dom_path.clone(), format!("{:?}", d.kind)))
        .collect();
    let mut expected = sorted.clone();
    expected.sort();
    assert_eq!(sorted, expected, "diagnostics sort by DOM path and kind");

    let rendered = outcome.canonical_projection();
    assert_eq!(
        outcome.output_digest,
        StructuralContentDigest::from_hex(&sha256(&rendered)).expect("sha256 hex"),
        "the output digest is sha256 over the canonical projection"
    );

    let path = fixture_dir().join("html_acceptance_v1.golden");
    if std::env::var_os("EVIDENCE_UPDATE_FIXTURES").is_some() {
        fs::write(&path, &rendered).expect("write fixture");
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
        rendered, committed,
        "canonical candidate projection drifted from the committed golden; \
         if the change is intended, regenerate with EVIDENCE_UPDATE_FIXTURES=1"
    );
}

#[test]
fn attribute_order_and_whitespace_variants_produce_equal_output() {
    let bytes = fs::read(fixture_dir().join("html_acceptance_v1.html")).expect("read fixture");
    let baseline = ingest(&bytes);

    // An equivalent serialization: swapped attribute order, extra
    // whitespace inside tags, and reflowed prose whitespace. The
    // DOM and text are unchanged, so the canonical graph is equal.
    let text = String::from_utf8(bytes).expect("fixture is UTF-8");
    let variant = text
        .replace(
            r#"<a href="https://www.rfc-editor.org/rfc/rfc2119" rel="external">"#,
            r#"<a  rel="external"   href="https://www.rfc-editor.org/rfc/rfc2119" >"#,
        )
        .replace(
            "document are to be interpreted as described in",
            "document   are  to   be\n       interpreted as described in",
        )
        .replace("<dd>Data presented", "<dd>\n          Data presented");
    assert_ne!(text, variant, "the variant actually varies the bytes");
    let rerun = ingest(variant.as_bytes());

    assert_eq!(
        baseline.canonical_projection(),
        rerun.canonical_projection(),
        "equivalent serializations produce equal canonical projections"
    );
    assert_eq!(
        baseline.output_digest, rerun.output_digest,
        "equivalent serializations produce equal output digests"
    );
    assert_eq!(baseline.diagnostics, rerun.diagnostics);
}

#[test]
fn reingestion_reuses_committed_uids_through_reconciliation() {
    let first = ingest_fixture();
    let second = ingest_fixture();

    assert_eq!(
        first.canonical_projection(),
        second.canonical_projection(),
        "repeated ingestion renders an identical canonical projection"
    );
    assert_eq!(
        first.output_digest, second.output_digest,
        "the output identity plane is deterministic across runs"
    );
    let first_uids: BTreeSet<&str> = first.nodes.iter().map(|n| n.uid.as_str()).collect();
    let second_uids: BTreeSet<&str> = second.nodes.iter().map(|n| n.uid.as_str()).collect();
    assert!(
        first_uids.is_disjoint(&second_uids),
        "minted identities are fresh per run"
    );

    // Commit run one's nodes in document order, then in reversed
    // record order: reconciliation must reuse the same committed uid
    // for each structurally matching candidate either way.
    for reversed in [false, true] {
        let mut graph = SourceGraph::new();
        let mut records: Vec<&SourceNode> = first.nodes.iter().collect();
        if reversed {
            records.reverse();
        }
        for node in records {
            graph.insert(node.clone()).expect("committed nodes insert");
        }

        let reconciled = reconcile(&graph, candidates_of(&second.nodes));
        assert_eq!(reconciled.len(), first.nodes.len());
        for (index, entry) in reconciled.iter().enumerate() {
            assert_eq!(
                entry.uid, first.nodes[index].uid,
                "node {index} must reuse the committed uid (reversed layout: {reversed})"
            );
        }
    }
}
