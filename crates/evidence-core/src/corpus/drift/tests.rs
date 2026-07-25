//! Node-plane and identity-plane drift comparison unit tests
//! (TEST-192, TEST-193): explicit equality, recipe and input
//! planes, per-field node categories, the semantic/diagnostic
//! locator split, and reconciliation edge cases.

use std::collections::BTreeSet;

use super::super::source_graph::locator::{SafeRelPath, SourceLocator};
use super::super::source_graph::normalization::{content_digest, fingerprint};
use super::super::source_graph::{SourceGraph, SourceNodeKind};
use super::tests_support::*;
use super::{DriftCategory, DriftOutcome, compare_reingestion};

#[test]
fn equal_planes_yield_explicit_equality() {
    let (corpus, graph, input, evaluations) = fixture();
    let recipe = fixture_recipe();
    let baseline = make_baseline(&corpus, recipe.digest(), input.clone(), &evaluations);
    let candidate = make_candidate(&recipe, input, &graph, &[], &evaluations);
    let report = compare_reingestion(&baseline, &candidate).expect("equal planes compare");
    assert_eq!(report.outcome(), DriftOutcome::Equal);
    assert!(report.is_equal());
    assert!(report.findings.is_empty());
}

#[test]
fn recipe_and_input_planes_report_distinctly() {
    let (corpus, graph, input, evaluations) = fixture();
    let recipe = fixture_recipe();
    let mut other_recipe = fixture_recipe();
    other_recipe.parser_version = "0.13.5".to_string();

    let baseline = make_baseline(&corpus, recipe.digest(), input.clone(), &evaluations);
    let moved = make_candidate(&other_recipe, input.clone(), &graph, &[], &evaluations);
    let report = compare_reingestion(&baseline, &moved).expect("recipe drift compares");
    assert_eq!(
        categories(&report),
        BTreeSet::from([DriftCategory::RecipeChangedOrUnavailable])
    );

    let mut absent = make_candidate(&recipe, input.clone(), &graph, &[], &evaluations);
    absent.recipe = None;
    let report = compare_reingestion(&baseline, &absent).expect("absent recipe compares");
    assert_eq!(
        categories(&report),
        BTreeSet::from([DriftCategory::RecipeChangedOrUnavailable])
    );

    let mut missing_input = make_candidate(&recipe, input.clone(), &graph, &[], &evaluations);
    missing_input.verified_input_digest = None;
    let report = compare_reingestion(&baseline, &missing_input).expect("missing input compares");
    assert_eq!(
        categories(&report),
        BTreeSet::from([DriftCategory::VerifiedInputChanged])
    );

    let moved_input = make_candidate(
        &recipe,
        structural(&"e".repeat(64)),
        &graph,
        &[],
        &evaluations,
    );
    let report = compare_reingestion(&baseline, &moved_input).expect("input drift compares");
    assert_eq!(
        categories(&report),
        BTreeSet::from([DriftCategory::VerifiedInputChanged])
    );
}

#[test]
fn node_field_changes_emit_independent_categories() {
    let (corpus, _graph, input, evaluations) = fixture();
    let recipe = fixture_recipe();
    let baseline = make_baseline(&corpus, recipe.digest(), input.clone(), &evaluations);

    // Kind change: the paragraph becomes a note; text and digests
    // stay comparable, so exactly the kind and fingerprint move.
    let mut kind_moved = base_graph();
    let mut note = node(
        &base_graph(),
        PARA,
        Some(SEC),
        SourceNodeKind::Note,
        0,
        None,
        "hello",
    );
    note.fingerprint = fingerprint(
        SourceNodeKind::Note,
        None,
        &[(SourceNodeKind::Section, Some("1 Intro"))],
    );
    let uid = note.uid.clone();
    kind_moved.remove_node(&uid);
    kind_moved.insert(note).unwrap();
    let report = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input.clone(), &kind_moved, &[], &evaluations),
    )
    .expect("kind drift compares");
    let found = categories(&report);
    assert!(found.contains(&DriftCategory::NodeKindChanged), "{found:?}");
    assert!(found.contains(&DriftCategory::NodeStructuralFingerprintChanged));
    assert!(!found.contains(&DriftCategory::NodeCanonicalTextChanged));

    // Text change: canonical text, content digest, and the
    // effective graph move; kind, parent, ordinal, label do not.
    let mut text_moved = base_graph();
    let mut paragraph = text_moved.node_mut(PARA).expect("paragraph").clone();
    paragraph.canonical_text = "goodbye".to_string();
    paragraph.content_sha256 = content_digest(SourceNodeKind::Paragraph, "goodbye");
    text_moved.remove_node(PARA);
    text_moved.insert(paragraph).unwrap();
    let report = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input.clone(), &text_moved, &[], &evaluations),
    )
    .expect("text drift compares");
    let found = categories(&report);
    assert!(found.contains(&DriftCategory::NodeCanonicalTextChanged));
    assert!(found.contains(&DriftCategory::NodeContentDigestChanged));
    assert!(found.contains(&DriftCategory::EffectiveGraphChanged));
    assert!(!found.contains(&DriftCategory::NodeKindChanged));
    assert!(!found.contains(&DriftCategory::NodeLabelChanged));

    // Ordinal change: two children swap positions; their content
    // pairs them back to their committed identities, so the move
    // reports as ordinal drift only.
    let committed = {
        let mut graph = SourceGraph::new();
        graph
            .insert(node(
                &graph,
                SEC,
                None,
                SourceNodeKind::Section,
                0,
                Some("1 Intro"),
                "",
            ))
            .unwrap();
        graph
            .insert(node(
                &graph,
                PARA,
                Some(SEC),
                SourceNodeKind::Paragraph,
                0,
                None,
                "hello",
            ))
            .unwrap();
        graph
            .insert(node(
                &graph,
                EXTRA,
                Some(SEC),
                SourceNodeKind::Note,
                1,
                None,
                "a note",
            ))
            .unwrap();
        graph
    };
    let swapped = {
        let mut graph = SourceGraph::new();
        graph
            .insert(node(
                &graph,
                SEC,
                None,
                SourceNodeKind::Section,
                0,
                Some("1 Intro"),
                "",
            ))
            .unwrap();
        graph
            .insert(node(
                &graph,
                PARA,
                Some(SEC),
                SourceNodeKind::Paragraph,
                1,
                None,
                "hello",
            ))
            .unwrap();
        graph
            .insert(node(
                &graph,
                EXTRA,
                Some(SEC),
                SourceNodeKind::Note,
                0,
                None,
                "a note",
            ))
            .unwrap();
        graph
    };
    let corpus = committed_corpus(&committed, None);
    let baseline = make_baseline(&corpus, recipe.digest(), input.clone(), &evaluations);
    let report = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input, &swapped, &[], &evaluations),
    )
    .expect("ordinal drift compares");
    let ordinal_findings = report
        .findings
        .iter()
        .filter(|finding| finding.category == DriftCategory::NodeOrdinalChanged)
        .count();
    assert_eq!(ordinal_findings, 2, "{:?}", report.findings);
    assert!(!categories(&report).contains(&DriftCategory::NodeAdded));
    assert!(!categories(&report).contains(&DriftCategory::NodeRemoved));
}

#[test]
fn locator_changes_split_semantic_from_diagnostic() {
    let (corpus, _graph, input, evaluations) = fixture();
    let recipe = fixture_recipe();
    let baseline = make_baseline(&corpus, recipe.digest(), input.clone(), &evaluations);

    // Diagnostic-only movement: the byte range slides.
    let mut diagnostic = base_graph();
    let mut paragraph = diagnostic.node_mut(PARA).expect("paragraph").clone();
    paragraph.locator = locator((5, 15));
    diagnostic.remove_node(PARA);
    diagnostic.insert(paragraph).unwrap();
    let report = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input.clone(), &diagnostic, &[], &evaluations),
    )
    .expect("diagnostic movement compares");
    let found = categories(&report);
    assert!(found.contains(&DriftCategory::DiagnosticLocatorMoved));
    assert!(
        !found.contains(&DriftCategory::NodeSemanticLocatorChanged),
        "diagnostic positions never count as semantic drift: {found:?}"
    );
    // The effective digest covers the canonical rendering, which
    // pins locator bytes — the plane identity moves, but never
    // under a semantic category.

    // Semantic movement: the anchor changes.
    let mut semantic = base_graph();
    let mut paragraph = semantic.node_mut(PARA).expect("paragraph").clone();
    paragraph.locator = SourceLocator::Markdown {
        path: SafeRelPath::new("docs/doc.md").unwrap(),
        git_blob: None,
        anchor: Some("moved".to_string()),
        heading_path: Vec::new(),
        byte_range: (0, 10),
    };
    semantic.remove_node(PARA);
    semantic.insert(paragraph).unwrap();
    let report = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input, &semantic, &[], &evaluations),
    )
    .expect("semantic movement compares");
    let found = categories(&report);
    assert!(found.contains(&DriftCategory::NodeSemanticLocatorChanged));
    assert!(!found.contains(&DriftCategory::DiagnosticLocatorMoved));
}

#[test]
fn node_added_removed_and_unreconciled_report() {
    let (corpus, _graph, input, evaluations) = fixture();
    let recipe = fixture_recipe();
    let baseline = make_baseline(&corpus, recipe.digest(), input.clone(), &evaluations);

    // Added: a candidate-only note under the section.
    let mut added = base_graph();
    added
        .insert(node(
            &added,
            EXTRA,
            Some(SEC),
            SourceNodeKind::Note,
            1,
            None,
            "new",
        ))
        .unwrap();
    let report = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input.clone(), &added, &[], &evaluations),
    )
    .expect("added node compares");
    let found = categories(&report);
    assert!(found.contains(&DriftCategory::NodeAdded));
    let finding = report
        .findings
        .iter()
        .find(|finding| finding.category == DriftCategory::NodeAdded)
        .expect("added finding");
    assert_eq!(finding.node_uid.as_deref(), Some(EXTRA));
    assert!(
        !finding
            .node_uid
            .as_deref()
            .is_none_or(|uid| uid == SEC || uid == PARA),
        "an added node reports its candidate uid, never a minted one"
    );

    // Removed: the candidate drops the paragraph.
    let removed = {
        let mut graph = base_graph();
        graph.remove_node(PARA);
        graph
    };
    let report = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input.clone(), &removed, &[], &evaluations),
    )
    .expect("removed node compares");
    let found = categories(&report);
    assert!(found.contains(&DriftCategory::NodeRemoved));
    assert!(!found.contains(&DriftCategory::NodeAdded));

    // Unreconciled: two committed root paragraphs share one
    // structural fingerprint, so the pool is formally ambiguous.
    let mut ambiguous = SourceGraph::new();
    ambiguous
        .insert(node(
            &ambiguous,
            SEC,
            None,
            SourceNodeKind::Paragraph,
            0,
            None,
            "a",
        ))
        .unwrap();
    ambiguous
        .insert(node(
            &ambiguous,
            PARA,
            None,
            SourceNodeKind::Paragraph,
            1,
            None,
            "b",
        ))
        .unwrap();
    let corpus = committed_corpus(&ambiguous, None);
    let baseline = make_baseline(&corpus, recipe.digest(), input.clone(), &evaluations);
    let mut one = SourceGraph::new();
    one.insert(node(
        &one,
        EXTRA,
        None,
        SourceNodeKind::Paragraph,
        0,
        None,
        "a",
    ))
    .unwrap();
    let report = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input, &one, &[], &evaluations),
    )
    .expect("ambiguous pool compares");
    let found = categories(&report);
    assert!(
        found.contains(&DriftCategory::NodeUnreconciled),
        "{found:?}"
    );
    assert!(found.contains(&DriftCategory::NodeRemoved));
}
