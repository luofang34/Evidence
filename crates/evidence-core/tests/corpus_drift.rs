//! Re-ingestion drift comparison acceptance tests (TEST-192):
//! identical committed and candidate planes return explicit
//! equality, equivalent linked layouts and reordered review files
//! produce zero findings and byte-identical reports, and the
//! golden findings byte-lock the sorted canonical report.
//!
//! The golden `drift_findings_v1.golden` byte-locks the canonical
//! report of a drifted candidate; regenerate with
//! `EVIDENCE_UPDATE_FIXTURES=1`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;

use evidence_core::corpus::{
    DriftCategory, DriftOutcome, SourceNodeKind, compare_reingestion, content_digest,
    evaluate_all_patch_lifecycles, render_report_canonical,
};

#[path = "corpus_drift_testkit.rs"]
mod testkit;

use testkit::{
    EXTRA, NOTE, NodeSpec, PARA, SEC, base_nodes, baseline, candidate, categories,
    committed_patches, fixture_dir, fixture_recipe, graph_from, load_corpus, locator, node,
};

/// Identical committed and candidate planes return explicit
/// equality with zero drift findings (TEST-192).
#[test]
fn identical_planes_return_explicit_equality() {
    let corpus = load_corpus(false);
    let evaluations = evaluate_all_patch_lifecycles(&corpus).expect("evaluations");
    let graph = graph_from(base_nodes());
    let patches = committed_patches(&corpus);
    let report = compare_reingestion(
        &baseline(&corpus, &evaluations),
        &candidate(&fixture_recipe(), &graph, &patches, &evaluations),
    )
    .expect("equal planes compare");
    assert_eq!(report.outcome(), DriftOutcome::Equal);
    assert!(report.is_equal());
    assert!(report.findings.is_empty());
    let rendered = String::from_utf8(render_report_canonical(&report)).expect("utf8");
    assert!(rendered.contains("outcome = equal"));
    assert!(rendered.contains("category = \"output_equal\""));
}

/// Equivalent linked file layouts, split review files, and
/// reversed candidate construction produce zero findings and
/// byte-identical reports (TEST-192).
#[test]
fn reordered_layouts_and_maps_produce_zero_findings() {
    let corpus_a = load_corpus(false);
    let corpus_b = load_corpus(true);
    assert_eq!(corpus_a, corpus_b, "equivalent layouts load equal graphs");
    let mut reports = Vec::new();
    for corpus in [&corpus_a, &corpus_b] {
        let evaluations = evaluate_all_patch_lifecycles(corpus).expect("evaluations");
        // Candidate built in reverse insertion order with the patch
        // slice reversed.
        let graph = graph_from(base_nodes().into_iter().rev().collect());
        let mut patches = committed_patches(corpus);
        patches.reverse();
        let report = compare_reingestion(
            &baseline(corpus, &evaluations),
            &candidate(&fixture_recipe(), &graph, &patches, &evaluations),
        )
        .expect("reordered planes compare");
        assert!(report.is_equal(), "{:?}", report.findings);
        reports.push(render_report_canonical(&report));
    }
    assert_eq!(
        reports[0], reports[1],
        "equivalent layouts report byte-identically"
    );
}

/// The golden byte-locks the canonical sorted report of a drifted
/// candidate, stable across repeated runs and equivalent file
/// layouts (TEST-192).
#[test]
fn golden_findings_stable_across_runs_and_equivalent_layouts() {
    let mut drifted_recipe = fixture_recipe();
    drifted_recipe.parser_version = "0.13.5".to_string();
    let mut drifted_nodes = base_nodes();
    for node in &mut drifted_nodes {
        if node.uid == PARA {
            node.canonical_text = "goodbye".to_string();
            node.content_sha256 = content_digest(SourceNodeKind::Paragraph, "goodbye");
        }
        if node.uid == NOTE {
            node.locator = locator((61, 75));
        }
    }
    let drifted = graph_from(drifted_nodes);
    let mut renders = Vec::new();
    for split in [false, true, false] {
        let corpus = load_corpus(split);
        let evaluations = evaluate_all_patch_lifecycles(&corpus).expect("evaluations");
        let patches = committed_patches(&corpus);
        let report = compare_reingestion(
            &baseline(&corpus, &evaluations),
            &candidate(&drifted_recipe, &drifted, &patches, &evaluations),
        )
        .expect("drifted candidate compares");
        assert!(!report.is_equal());
        renders.push(render_report_canonical(&report));
    }
    assert_eq!(renders[0], renders[1], "layouts render identically");
    assert_eq!(renders[0], renders[2], "repeated runs render identically");
    let path = fixture_dir().join("drift_findings_v1.golden");
    if std::env::var_os("EVIDENCE_UPDATE_FIXTURES").is_some() {
        fs::write(&path, &renders[0]).expect("write fixture");
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
        committed, renders[0],
        "the canonical drift report drifted — the sorting and rendering contract is byte-locked"
    );
}
/// Added, removed, and reordered nodes reconcile deterministically
/// and findings stay sorted (TEST-193).
#[test]
fn added_removed_reordered_nodes_reconcile_deterministically() {
    let corpus = load_corpus(false);
    let evaluations = evaluate_all_patch_lifecycles(&corpus).expect("evaluations");
    let patches = committed_patches(&corpus);
    let recipe = fixture_recipe();
    // The paragraph moves after a newly added note; the committed
    // note vanishes.
    let section = node(
        &[],
        &NodeSpec {
            uid: SEC,
            parent: None,
            kind: SourceNodeKind::Section,
            ordinal: 0,
            label: Some("1 Intro"),
            text: "",
            byte_range: (0, 50),
        },
    );
    let candidate_nodes = vec![
        section.clone(),
        node(
            std::slice::from_ref(&section),
            &NodeSpec {
                uid: EXTRA,
                parent: Some(SEC),
                kind: SourceNodeKind::Note,
                ordinal: 0,
                label: None,
                text: "extra",
                byte_range: (51, 56),
            },
        ),
        node(
            std::slice::from_ref(&section),
            &NodeSpec {
                uid: PARA,
                parent: Some(SEC),
                kind: SourceNodeKind::Paragraph,
                ordinal: 1,
                label: None,
                text: "hello",
                byte_range: (57, 66),
            },
        ),
    ];
    let drifted = graph_from(candidate_nodes);
    let mut reports = Vec::new();
    for _ in 0..2 {
        reports.push(
            compare_reingestion(
                &baseline(&corpus, &evaluations),
                &candidate(&recipe, &drifted, &patches, &evaluations),
            )
            .expect("reordered nodes compare"),
        );
    }
    assert_eq!(reports[0], reports[1], "repeated runs report identically");
    let found = categories(&reports[0].findings);
    assert!(found.contains(&DriftCategory::NodeAdded), "{found:?}");
    assert!(found.contains(&DriftCategory::NodeRemoved), "{found:?}");
    assert!(
        found.contains(&DriftCategory::NodeOrdinalChanged),
        "{found:?}"
    );
    let keys: Vec<_> = reports[0]
        .findings
        .iter()
        .map(|finding| {
            (
                finding.category,
                finding.structural_path.clone(),
                finding.node_uid.clone(),
                finding.patch_uid.clone(),
            )
        })
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "findings stay in sort order");
}
