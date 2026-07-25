//! Drift category acceptance tests (TEST-193): targeted mutations
//! exercise every drift category independently, recipe, input,
//! parser, patch, review, and effective-output changes stay
//! distinguishable, semantic and diagnostic locator changes are
//! distinct, and added, removed, and reordered nodes reconcile
//! deterministically with sorted findings.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::{BTreeMap, BTreeSet};

use evidence_core::corpus::{
    DriftCategory, PatchLifecycle, PatchLifecycleEvaluation, ReingestionCandidate, SourceNodeKind,
    SourcePatchRecord, compare_reingestion, evaluate_all_patch_lifecycles, reviewed_content_digest,
};

#[path = "corpus_drift_testkit.rs"]
mod testkit;

use testkit::{
    EXTRA, NOTE, NodeSpec, PARA, SEC, base_nodes, baseline, candidate, categories,
    committed_patches, fixture_recipe, graph_from, graphs_toml, load_corpus, locator, node,
    source_toml, write,
};

const PATCH_B: &str = "patch_00000000-0000-4000-8000-0000000000b2";

/// A one-entry evaluation map in the given state.
fn eval_in(
    patch: &SourcePatchRecord,
    state: PatchLifecycle,
) -> BTreeMap<String, PatchLifecycleEvaluation> {
    BTreeMap::from([(
        patch.uid.clone(),
        PatchLifecycleEvaluation {
            patch_uid: patch.uid.clone(),
            state,
            current_digest: patch.reviewed_content_digest.clone(),
            effective_review_uids: Vec::new(),
        },
    )])
}

/// A second valid patch, on the note.
fn patch_b() -> SourcePatchRecord {
    let mut record = testkit::patch_record();
    record.uid = PATCH_B.to_string();
    record.human_id = "fix-note".to_string();
    record.operations = vec![evidence_core::corpus::PatchOperation::ReplaceContent {
        ordinal: 0,
        target_uid: NOTE.to_string(),
        expected_content_sha256: graph_from(base_nodes())
            .get(NOTE)
            .expect("note")
            .content_sha256
            .clone(),
        new_canonical_text: Some("a note!".to_string()),
        new_label: None,
    }];
    record.reviewed_content_digest = reviewed_content_digest(&record);
    record
}

/// Targeted mutations exercise every drift category independently
/// (TEST-193).
#[test]
fn every_category_fires_independently() {
    let corpus = load_corpus(false);
    let committed_evals = evaluate_all_patch_lifecycles(&corpus).expect("evaluations");
    let recipe = fixture_recipe();
    let equal_graph = graph_from(base_nodes());
    let equal_patches = committed_patches(&corpus);
    let mut covered: BTreeSet<DriftCategory> = BTreeSet::new();

    // Unreconciled: a committed corpus whose two root paragraphs
    // share one structural fingerprint (kept out of the `run`
    // closure below because it uses a different baseline).
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("sources.toml"), &source_toml());
    let first = node(
        &[],
        &NodeSpec {
            uid: SEC,
            parent: None,
            kind: SourceNodeKind::Paragraph,
            ordinal: 0,
            label: None,
            text: "a",
            byte_range: (0, 5),
        },
    );
    let second = node(
        std::slice::from_ref(&first),
        &NodeSpec {
            uid: PARA,
            parent: None,
            kind: SourceNodeKind::Paragraph,
            ordinal: 1,
            label: None,
            text: "b",
            byte_range: (6, 10),
        },
    );
    write(
        &dir.path().join("graphs.toml"),
        &graphs_toml(&[first, second]),
    );
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources.toml\"]\nsource_graphs = [\"graphs.toml\"]\n",
    );
    let ambiguous_corpus =
        evidence_core::corpus::CorpusIndex::load_graph(&dir.path().join("corpus.toml"))
            .expect("ambiguous corpus loads");
    let no_evals = BTreeMap::new();
    let one = graph_from(vec![node(
        &[],
        &NodeSpec {
            uid: EXTRA,
            parent: None,
            kind: SourceNodeKind::Paragraph,
            ordinal: 0,
            label: None,
            text: "a",
            byte_range: (0, 5),
        },
    )]);
    let report = compare_reingestion(
        &baseline(&ambiguous_corpus, &no_evals),
        &candidate(&recipe, &one, &[], &no_evals),
    )
    .expect("ambiguous pool compares");
    covered.extend(categories(&report.findings));

    let mut run = |candidate: ReingestionCandidate<'_>| {
        let report = compare_reingestion(&baseline(&corpus, &committed_evals), &candidate)
            .expect("scenario compares");
        covered.extend(categories(&report.findings));
        report
    };

    // Recipe and input planes: absent identities.
    let mut absent = candidate(&recipe, &equal_graph, &equal_patches, &committed_evals);
    absent.recipe = None;
    absent.verified_input_digest = None;
    run(absent);

    // Node planes: label, fingerprint, text, content digest,
    // parent, semantic locator, addition — one mutated forest.
    let section = node(
        &[],
        &NodeSpec {
            uid: SEC,
            parent: None,
            kind: SourceNodeKind::Section,
            ordinal: 0,
            label: Some("1 Overview"),
            text: "",
            byte_range: (0, 50),
        },
    );
    let paragraph = node(
        std::slice::from_ref(&section),
        &NodeSpec {
            uid: PARA,
            parent: Some(SEC),
            kind: SourceNodeKind::Paragraph,
            ordinal: 0,
            label: None,
            text: "goodbye",
            byte_range: (51, 60),
        },
    );
    let mut moved_note = node(
        &[section.clone(), paragraph.clone()],
        &NodeSpec {
            uid: NOTE,
            parent: None,
            kind: SourceNodeKind::Note,
            ordinal: 1,
            label: None,
            text: "a note",
            byte_range: (61, 70),
        },
    );
    moved_note.locator = evidence_core::corpus::SourceLocator::Markdown {
        path: evidence_core::corpus::SafeRelPath::new("docs/doc.md").expect("safe path"),
        git_blob: None,
        anchor: Some("moved".to_string()),
        heading_path: Vec::new(),
        byte_range: (61, 70),
    };
    let extra = node(
        &[section.clone(), paragraph.clone()],
        &NodeSpec {
            uid: EXTRA,
            parent: Some(SEC),
            kind: SourceNodeKind::Paragraph,
            ordinal: 1,
            label: None,
            text: "extra",
            byte_range: (71, 80),
        },
    );
    let mutated = graph_from(vec![section, paragraph, moved_note, extra]);
    run(candidate(
        &recipe,
        &mutated,
        &equal_patches,
        &committed_evals,
    ));

    // Ordinal plane: the two children swap.
    let swapped = graph_from(vec![node(
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
    )]);
    let section = swapped.get(SEC).expect("section").clone();
    let swapped = graph_from(vec![
        section.clone(),
        node(
            std::slice::from_ref(&section),
            &NodeSpec {
                uid: PARA,
                parent: Some(SEC),
                kind: SourceNodeKind::Paragraph,
                ordinal: 1,
                label: None,
                text: "hello",
                byte_range: (51, 60),
            },
        ),
        node(
            std::slice::from_ref(&section),
            &NodeSpec {
                uid: NOTE,
                parent: Some(SEC),
                kind: SourceNodeKind::Note,
                ordinal: 0,
                label: None,
                text: "a note",
                byte_range: (61, 70),
            },
        ),
    ]);
    run(candidate(
        &recipe,
        &swapped,
        &equal_patches,
        &committed_evals,
    ));

    // Kind plane: the paragraph reclassifies as a note.
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
    let reclassified = graph_from(vec![
        section.clone(),
        node(
            std::slice::from_ref(&section),
            &NodeSpec {
                uid: PARA,
                parent: Some(SEC),
                kind: SourceNodeKind::Note,
                ordinal: 0,
                label: None,
                text: "hello",
                byte_range: (51, 60),
            },
        ),
        node(
            std::slice::from_ref(&section),
            &NodeSpec {
                uid: NOTE,
                parent: Some(SEC),
                kind: SourceNodeKind::Note,
                ordinal: 1,
                label: None,
                text: "a note",
                byte_range: (61, 70),
            },
        ),
    ]);
    run(candidate(
        &recipe,
        &reclassified,
        &equal_patches,
        &committed_evals,
    ));

    // Diagnostic plane: the paragraph's byte range slides.
    let mut diagnostic_nodes = base_nodes();
    for node in &mut diagnostic_nodes {
        if node.uid == PARA {
            node.locator = locator((52, 61));
        }
    }
    let diagnostic = graph_from(diagnostic_nodes);
    run(candidate(
        &recipe,
        &diagnostic,
        &equal_patches,
        &committed_evals,
    ));

    // Patch and review planes.
    let added_patches = [equal_patches.clone(), vec![patch_b()]].concat();
    let mut added_evals = committed_evals.clone();
    added_evals.extend(eval_in(&patch_b(), PatchLifecycle::Candidate));
    run(candidate(
        &recipe,
        &equal_graph,
        &added_patches,
        &added_evals,
    ));
    run(candidate(&recipe, &equal_graph, &[], &committed_evals));
    let mut changed_patch = equal_patches.clone();
    for patch in &mut changed_patch {
        if let evidence_core::corpus::PatchOperation::ReplaceContent {
            new_canonical_text, ..
        } = &mut patch.operations[0]
        {
            *new_canonical_text = Some("hello?".to_string());
        }
        patch.reviewed_content_digest = reviewed_content_digest(patch);
    }
    run(candidate(
        &recipe,
        &equal_graph,
        &changed_patch,
        &committed_evals,
    ));
    let stale = eval_in(&equal_patches[0], PatchLifecycle::Stale);
    run(candidate(&recipe, &equal_graph, &equal_patches, &stale));
    let rejected = eval_in(&equal_patches[0], PatchLifecycle::Rejected);
    let report = run(candidate(&recipe, &equal_graph, &equal_patches, &rejected));
    assert!(categories(&report.findings).contains(&DriftCategory::PatchRejected));

    // Every finding-carrying category fired; `OutputEqual` is the
    // equality marker and never a finding.
    let all: BTreeSet<DriftCategory> = [
        DriftCategory::RecipeChangedOrUnavailable,
        DriftCategory::VerifiedInputChanged,
        DriftCategory::NodeAdded,
        DriftCategory::NodeRemoved,
        DriftCategory::NodeUnreconciled,
        DriftCategory::NodeKindChanged,
        DriftCategory::NodeParentChanged,
        DriftCategory::NodeOrdinalChanged,
        DriftCategory::NodeLabelChanged,
        DriftCategory::NodeCanonicalTextChanged,
        DriftCategory::NodeContentDigestChanged,
        DriftCategory::NodeStructuralFingerprintChanged,
        DriftCategory::NodeSemanticLocatorChanged,
        DriftCategory::DiagnosticLocatorMoved,
        DriftCategory::PatchAdded,
        DriftCategory::PatchRemoved,
        DriftCategory::PatchChanged,
        DriftCategory::PatchStale,
        DriftCategory::PatchRejected,
        DriftCategory::PatchUnappliable,
        DriftCategory::ReviewStateChanged,
        DriftCategory::EffectiveGraphChanged,
    ]
    .into_iter()
    .collect();
    assert_eq!(
        covered,
        all,
        "missing categories: {:?}",
        all.difference(&covered)
    );
}

/// Semantic locator changes and diagnostic-position movement are
/// distinct categories (TEST-193).
#[test]
fn semantic_and_diagnostic_locator_changes_are_distinct() {
    let corpus = load_corpus(false);
    let evaluations = evaluate_all_patch_lifecycles(&corpus).expect("evaluations");
    let patches = committed_patches(&corpus);
    let recipe = fixture_recipe();

    let mut diagnostic_nodes = base_nodes();
    for node in &mut diagnostic_nodes {
        if node.uid == PARA {
            node.locator = locator((100, 200));
        }
    }
    let diagnostic = graph_from(diagnostic_nodes);
    let report = compare_reingestion(
        &baseline(&corpus, &evaluations),
        &candidate(&recipe, &diagnostic, &patches, &evaluations),
    )
    .expect("diagnostic movement compares");
    let found = categories(&report.findings);
    assert!(found.contains(&DriftCategory::DiagnosticLocatorMoved));
    assert!(!found.contains(&DriftCategory::NodeSemanticLocatorChanged));

    let mut semantic_nodes = base_nodes();
    for node in &mut semantic_nodes {
        if node.uid == PARA {
            node.locator = evidence_core::corpus::SourceLocator::Markdown {
                path: evidence_core::corpus::SafeRelPath::new("docs/other.md").expect("safe path"),
                git_blob: None,
                anchor: None,
                heading_path: Vec::new(),
                byte_range: (51, 60),
            };
        }
    }
    let semantic = graph_from(semantic_nodes);
    let report = compare_reingestion(
        &baseline(&corpus, &evaluations),
        &candidate(&recipe, &semantic, &patches, &evaluations),
    )
    .expect("semantic movement compares");
    let found = categories(&report.findings);
    assert!(found.contains(&DriftCategory::NodeSemanticLocatorChanged));
    assert!(!found.contains(&DriftCategory::DiagnosticLocatorMoved));
}
