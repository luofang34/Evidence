//! PDF ingestion contract and projection unit tests: the
//! fail-fast contract order and the layout-rule classifications
//! over the committed extractor-output fixtures.

use std::collections::BTreeSet;

use super::lock::PdfToolLock;
use super::*;

const REV: &str = "src_00000000-0000-4000-8000-0000000000d1";

fn fixture_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus")
}

fn recipe() -> PdfIngestionRecipe {
    let raw = std::fs::read_to_string(fixture_dir().join("pdf_tool_lock_v1.toml"))
        .expect("read tool lock fixture");
    PdfIngestionRecipe {
        tool_lock: PdfToolLock::from_toml(&raw).expect("tool lock fixture validates"),
        rules: PdfLayoutRules {
            header_bottom: 50.0,
            footer_top: 740.0,
            column_split_x: Some(306.0),
            max_heading_depth: 2,
            note_prefixes: BTreeSet::from(["NOTE".to_string()]),
            caption_prefixes: BTreeSet::from(["Figure".to_string()]),
            page_label_prefix: Some("Page ".to_string()),
        },
    }
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture_dir().join(name)).expect("read fixture")
}

fn input<'a>(pdf: &'a [u8], xhtml: &'a [u8]) -> IngestPdfInput<'a> {
    IngestPdfInput {
        bytes: pdf,
        media_type: PDF_MEDIA_TYPE,
        source_revision_uid: REV,
        canonical_path: "vendor/spec/sdls-like.pdf",
        input_digest: StructuralContentDigest::from_hex(&crate::hash::sha256(pdf))
            .expect("sha256 hex"),
        extractor_output: xhtml,
        recipe: recipe(),
    }
}

fn sdls() -> (Vec<u8>, Vec<u8>) {
    (
        fixture_bytes("pdf_sdls_acceptance_v1.pdf"),
        fixture_bytes("pdf_sdls_bbox_v1.xhtml"),
    )
}

#[test]
fn contract_violations_fail_in_the_documented_order() {
    let (pdf, xhtml) = sdls();
    // Media type first.
    let mut bad = input(&pdf, &xhtml);
    bad.media_type = "text/html";
    assert!(matches!(
        ingest_pdf(&bad),
        Err(PdfIngestError::MediaTypeMismatch { .. })
    ));
    // Then the input digest.
    let mut bad = input(&pdf, &xhtml);
    bad.input_digest = StructuralContentDigest::from_hex(&"0".repeat(64)).expect("hex");
    assert!(matches!(
        ingest_pdf(&bad),
        Err(PdfIngestError::InputDigestMismatch { .. })
    ));
    // Then the revision uid.
    let mut bad = input(&pdf, &xhtml);
    bad.source_revision_uid = "not-a-uid";
    assert!(matches!(
        ingest_pdf(&bad),
        Err(PdfIngestError::InvalidSourceRevisionUid { .. })
    ));
    // Then the canonical path.
    let mut bad = input(&pdf, &xhtml);
    bad.canonical_path = "../escape.pdf";
    assert!(matches!(
        ingest_pdf(&bad),
        Err(PdfIngestError::InvalidCanonicalPath { .. })
    ));
    // Then the recipe rules.
    let mut bad = input(&pdf, &xhtml);
    bad.recipe.rules.header_bottom = 800.0;
    assert!(matches!(
        ingest_pdf(&bad),
        Err(PdfIngestError::InvalidRules { .. })
    ));
    // Then the extractor output.
    let mut bad = input(&pdf, &xhtml);
    bad.extractor_output = b"not xml";
    assert!(matches!(ingest_pdf(&bad), Err(PdfIngestError::Bbox(_))));
}

#[test]
fn sdls_projection_classifies_sections_notes_captions_and_bands() {
    let (pdf, xhtml) = sdls();
    let ingestion = ingest_pdf(&input(&pdf, &xhtml)).expect("SDLS ingests");
    let kinds: Vec<SourceNodeKind> = ingestion.nodes.iter().map(|node| node.kind).collect();
    assert_eq!(
        kinds,
        [
            SourceNodeKind::Section,
            SourceNodeKind::Paragraph,
            SourceNodeKind::Section,
            SourceNodeKind::Paragraph,
            SourceNodeKind::Note,
            SourceNodeKind::Section,
            SourceNodeKind::Paragraph,
            SourceNodeKind::FigureCaption,
            SourceNodeKind::Paragraph,
        ]
    );
    // Numbering arrives through the label; nesting follows depth.
    let labels: Vec<Option<&str>> = ingestion
        .nodes
        .iter()
        .map(|node| node.label.as_deref())
        .collect();
    assert_eq!(
        labels[..3],
        [Some("1"), None, Some("1.1")],
        "sections carry numbering labels"
    );
    let one = &ingestion.nodes[0];
    let one_one = &ingestion.nodes[2];
    assert_eq!(one_one.parent_uid.as_deref(), Some(one.uid.as_str()));
    // Hyphenated words are never merged.
    assert!(
        ingestion.nodes[1]
            .canonical_text
            .contains("inter-\nnational")
    );
    // Page 2 nodes carry the printed label of their page.
    let page2 = &ingestion.nodes[6];
    assert_eq!(
        page2.locator,
        crate::corpus::SourceLocator::Pdf {
            physical_page: 2,
            printed_label: Some("2".to_string()),
            bbox: page2_bbox(page2),
        }
    );
    // The column rule orders the left column (with its caption)
    // before the right column.
    assert!(ingestion.nodes[6].canonical_text.starts_with("2.1 Left"));
    assert_eq!(ingestion.nodes[7].kind, SourceNodeKind::FigureCaption);
    assert!(ingestion.nodes[8].canonical_text.starts_with("2.2 Right"));
    // Every header and footer line drops with a typed diagnostic.
    let bands: Vec<PdfExcludedBand> = ingestion
        .diagnostics
        .iter()
        .filter_map(|diagnostic| match diagnostic.kind {
            PdfIngestDiagnosticKind::ExcludedByRule { band } => Some(band),
            _ => None,
        })
        .collect();
    assert_eq!(bands.len(), 4, "two headers and two footers drop");
}

fn page2_bbox(node: &SourceNode) -> [f64; 4] {
    match &node.locator {
        crate::corpus::SourceLocator::Pdf { bbox, .. } => *bbox,
        other => unreachable!("pdf nodes carry pdf locators, got {other:?}"),
    }
}

#[test]
fn pics_projection_reports_table_loss_without_table_nodes() {
    let pdf = fixture_bytes("pdf_pics_acceptance_v1.pdf");
    let xhtml = fixture_bytes("pdf_pics_bbox_v1.xhtml");
    let ingestion = ingest_pdf(&input(&pdf, &xhtml)).expect("PICS ingests");
    assert!(
        ingestion.nodes.iter().all(|node| !matches!(
            node.kind,
            SourceNodeKind::Table | SourceNodeKind::TableRow | SourceNodeKind::TableCell
        )),
        "no table structure is ever claimed"
    );
    let losses: Vec<&PdfIngestDiagnostic> = ingestion
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.kind,
                PdfIngestDiagnosticKind::StructuralLoss { construct: "table" }
            )
        })
        .collect();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].page, 1);
}
