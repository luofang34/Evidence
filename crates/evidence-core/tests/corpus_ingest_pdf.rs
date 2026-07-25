//! PDF ingestion acceptance tests (TEST-195..TEST-200): the
//! committed tool lock byte-locks its identity digest, the runner
//! contract holds without PATH lookup or workspace mutation, the
//! SDLS-shaped fixture byte-locks its golden canonical projection
//! with correct printed/physical page locators, repeated
//! execution reproduces identical extractor-output and graph
//! digests, the raw PICS extraction reports structural loss, the
//! approved curated patch restores the effective table rows and
//! cells, and an extractor-output change surfaces in its own
//! drift category.
//!
//! The fixtures are minimal hand-crafted, independently
//! redistributable PDFs (`tools/gen-pdf-acceptance-fixtures.py`)
//! plus the exact `pdftotext -bbox-layout` output of the pinned
//! Nix Poppler (`pdf_tool_lock_v1.toml`); see the module docs of
//! `evidence_core::corpus::ingest::pdf` for the contract. The
//! goldens byte-lock the tool-lock identity digest and the
//! canonical projection; regenerate with
//! `EVIDENCE_UPDATE_FIXTURES=1`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

#[path = "corpus_ingest_pdf/support.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
#[cfg(unix)]
use std::path::Path;

use evidence_core::corpus::{
    DriftBaseline, IngesterRecipe, PatchBindings, PatchLifecycleEvaluation, ReingestionCandidate,
    SourceNode, SourceNodeKind, StructuralContentDigest, compare_reingestion,
    effective_source_graph, evaluate_all_patch_lifecycles,
};
use evidence_core::hash::sha256;

use support::*;

#[test]
fn tool_lock_golden_digest_is_stable() {
    let raw =
        fs::read_to_string(fixture_dir().join("pdf_tool_lock_v1.toml")).expect("read tool lock");
    let lock =
        evidence_core::corpus::PdfToolLock::from_toml(&raw).expect("tool lock fixture validates");
    assert_golden(
        "pdf_tool_lock_v1.golden",
        format!("{}\n", lock.digest()).as_bytes(),
    );
}

#[cfg(unix)]
#[test]
fn runner_uses_no_path_lookup_and_leaves_workspace_untouched() {
    use std::os::unix::fs::PermissionsExt;

    use evidence_core::corpus::{PdfRunBounds, PdfRunError, run_pdftotext_blocking};

    // A bare name is a PATH lookup and never spawns.
    let error = run_pdftotext_blocking(
        Path::new("pdftotext"),
        Path::new("in.pdf"),
        &PdfRunBounds::default(),
    )
    .expect_err("bare name rejected");
    assert!(matches!(error, PdfRunError::PathLookupForbidden { .. }));

    // The child runs in its own isolated directory, never the
    // workspace, and the input stays byte-identical.
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().display().to_string();
    let exe = dir.path().join("fake-pdftotext");
    let script = format!(
        "#!/bin/sh\nfor last; do :; done\nprintf '<doc/>\\n' > \"$last\"\n\
         if [ \"$(pwd)\" = \"{workspace}\" ]; then : > \"{workspace}/ran-in-workspace\"; fi\n"
    );
    fs::write(&exe, script).expect("write fake");
    let mut permissions = fs::metadata(&exe).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&exe, permissions).expect("chmod");
    let input_path = dir.path().join("input.pdf");
    let pdf = fixture_bytes("pdf_pics_acceptance_v1.pdf");
    fs::write(&input_path, &pdf).expect("write input");
    let extraction = run_pdftotext_blocking(&exe, &input_path, &PdfRunBounds::default())
        .expect("fake extraction succeeds");
    assert_eq!(extraction.output_bytes, b"<doc/>\n");
    assert_eq!(extraction.output_digest.as_str(), sha256(b"<doc/>\n"));
    assert_eq!(
        fs::read(&input_path).expect("read input"),
        pdf,
        "the input is byte-identical"
    );
    assert!(
        !dir.path().join("ran-in-workspace").exists(),
        "the child never runs in the workspace"
    );
}

#[test]
fn sdls_fixture_produces_golden_canonical_nodes() {
    let ingestion = ingest("pdf_sdls_acceptance_v1.pdf", "pdf_sdls_bbox_v1.xhtml");
    assert_golden(
        "pdf_sdls_projection_v1.golden",
        &ingestion.canonical_projection(),
    );
    // Printed and physical page locators are exact.
    let page2 = ingestion
        .nodes
        .iter()
        .find(|node| node.canonical_text.starts_with("2.1 Left"))
        .expect("left-column paragraph");
    let evidence_core::corpus::SourceLocator::Pdf {
        physical_page,
        printed_label,
        ..
    } = &page2.locator
    else {
        panic!("pdf locator");
    };
    assert_eq!(*physical_page, 2);
    assert_eq!(printed_label.as_deref(), Some("2"));
}

#[test]
fn repeated_execution_reproduces_extractor_and_graph_digests() {
    let first = ingest("pdf_sdls_acceptance_v1.pdf", "pdf_sdls_bbox_v1.xhtml");
    let second = ingest("pdf_sdls_acceptance_v1.pdf", "pdf_sdls_bbox_v1.xhtml");
    assert_eq!(
        first.extractor_output_digest,
        second.extractor_output_digest
    );
    assert_eq!(first.output_digest, second.output_digest);
    assert_eq!(
        first.extractor_output_digest.as_str(),
        sha256(&fixture_bytes("pdf_sdls_bbox_v1.xhtml")),
        "the extractor-output digest covers the raw extractor bytes"
    );
}

#[test]
fn pics_raw_extraction_reports_structural_loss() {
    let ingestion = ingest("pdf_pics_acceptance_v1.pdf", "pdf_pics_bbox_v1.xhtml");
    assert!(
        ingestion.nodes.iter().all(|node| !matches!(
            node.kind,
            SourceNodeKind::Table | SourceNodeKind::TableRow | SourceNodeKind::TableCell
        )),
        "the raw projection never claims table structure"
    );
    let losses = ingestion
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.kind,
                evidence_core::corpus::PdfIngestDiagnosticKind::StructuralLoss {
                    construct: "table"
                }
            )
        })
        .count();
    assert_eq!(losses, 1, "the table-shaped block reports loss");
    assert!(
        ingestion
            .nodes
            .iter()
            .any(|node| node.canonical_text.contains("R3 C1 Conditional")),
        "the table text is retained as a plain paragraph"
    );
}

#[test]
fn approved_patch_restores_effective_table_rows_and_cells() {
    let bindings = PatchBindings {
        recipe_digest: recipe().digest(),
        input_digest: StructuralContentDigest::from_hex(&sha256(&fixture_bytes(
            "pdf_pics_acceptance_v1.pdf",
        )))
        .expect("hex"),
    };

    // Candidate (unreviewed): never contributes.
    let pics = load_pics_corpus(false);
    let effective = effective_source_graph(&pics.corpus, REV, &bindings, MEDIA).expect("effective");
    assert!(effective.applied_patch_uids.is_empty());
    assert!(effective.graph.get(TBL).is_none());

    // Approved: the effective graph gains the restored table.
    let pics = load_pics_corpus(true);
    let effective = effective_source_graph(&pics.corpus, REV, &bindings, MEDIA).expect("effective");
    assert_eq!(effective.applied_patch_uids, vec![PATCH_UID.to_string()]);
    let table = effective.graph.get(TBL).expect("restored table");
    assert_eq!(table.kind, SourceNodeKind::Table);
    let row = effective.graph.get(ROW1).expect("restored row");
    assert_eq!(row.parent_uid.as_deref(), Some(TBL));
    let cells: Vec<&SourceNode> = effective
        .graph
        .nodes()
        .filter(|node| node.kind == SourceNodeKind::TableCell)
        .collect();
    assert_eq!(cells.len(), 6, "six restored cells");
    assert!(
        cells.iter().all(|cell| {
            let parent = cell.parent_uid.as_deref();
            parent == Some(ROW1) || parent == Some(ROW2)
        }),
        "every cell lands under its row"
    );
    // The committed parser plane is untouched.
    assert!(
        pics.corpus
            .source_graph(REV)
            .is_none_or(|committed| committed.get(TBL).is_none())
    );
}

/// A stand-in recipe for the drift comparator's recipe plane;
/// the extractor-output plane is what the test exercises.
fn plain_recipe() -> IngesterRecipe {
    IngesterRecipe {
        parser: "pdftotext".to_string(),
        parser_version: "25.10.0".to_string(),
        extensions: BTreeSet::new(),
        adapter_version: "1".to_string(),
        normalization_contract: "evidence/pdf-ingestion-recipe/v1".to_string(),
    }
}

#[test]
fn extractor_output_change_reports_its_own_drift_category() {
    let pics = load_pics_corpus(false);
    let extractor_hex = sha256(&fixture_bytes("pdf_pics_bbox_v1.xhtml"));
    let evaluations: BTreeMap<String, PatchLifecycleEvaluation> =
        evaluate_all_patch_lifecycles(&pics.corpus).expect("evaluations");
    let baseline = DriftBaseline {
        corpus: &pics.corpus,
        source_revision_uid: REV,
        recipe_digest: plain_recipe().digest(),
        input_digest: StructuralContentDigest::from_hex(&pics.input_hex).expect("hex"),
        extractor_output_digest: Some(
            StructuralContentDigest::from_hex(&extractor_hex).expect("hex"),
        ),
        patch_evaluations: &evaluations,
    };
    let committed = pics.corpus.source_graph(REV).expect("committed graph");
    let patches: Vec<evidence_core::corpus::SourcePatchRecord> =
        pics.corpus.source_patches().values().cloned().collect();
    let candidate_recipe = plain_recipe();
    let mut candidate = ReingestionCandidate {
        source_document: CANONICAL,
        recipe: Some(&candidate_recipe),
        verified_input_digest: Some(
            StructuralContentDigest::from_hex(&pics.input_hex).expect("hex"),
        ),
        extractor_output_digest: Some(
            StructuralContentDigest::from_hex(&extractor_hex).expect("hex"),
        ),
        parser_graph: committed,
        patches: &patches,
        patch_evaluations: &evaluations,
    };

    // Identical planes: zero findings.
    let report = compare_reingestion(&baseline, &candidate).expect("comparison");
    assert!(report.is_equal(), "identical planes report equality");

    // A changed extractor output fires exactly its own category.
    candidate.extractor_output_digest =
        Some(StructuralContentDigest::from_hex(&"1".repeat(64)).expect("hex"));
    let report = compare_reingestion(&baseline, &candidate).expect("comparison");
    assert_eq!(report.findings.len(), 1);
    assert_eq!(
        report.findings[0].category,
        evidence_core::corpus::DriftCategory::ExtractorOutputChanged
    );

    // An absent candidate extractor-output identity is drift, not
    // refusal.
    candidate.extractor_output_digest = None;
    let report = compare_reingestion(&baseline, &candidate).expect("comparison");
    assert_eq!(report.findings.len(), 1);
    assert_eq!(
        report.findings[0].category,
        evidence_core::corpus::DriftCategory::ExtractorOutputChanged
    );
}
