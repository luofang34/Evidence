//! Patch-plane, prerequisite, and determinism unit tests for the
//! drift comparison (TEST-193, TEST-194): every patch category,
//! stale-binding inapplicability, aggregate safety, fail-closed
//! prerequisites, sort order, and read-only immutability.

use std::collections::BTreeMap;

use super::super::graph::CorpusGraph;
use super::super::patch_lifecycle::PatchLifecycle;
use super::super::patch_testkit;
use super::super::source_graph::SourceNodeKind;
use super::super::source_graph::normalization::content_digest;
use super::super::source_patch::digest::reviewed_content_digest;
use super::tests_support::*;
use super::{DriftBaseline, DriftCategory, DriftError, compare_reingestion};

#[test]
fn patch_plane_categories_report_independently() {
    let graph = base_graph();
    let patch = patch_for(&graph, "hello!");
    let corpus = committed_corpus(&graph, Some(&patch));
    let recipe = fixture_recipe();
    let input = structural(patch_testkit::INPUT_HEX);
    let mut committed_evals = BTreeMap::new();
    committed_evals.insert(
        patch.uid.clone(),
        evaluation(&patch, PatchLifecycle::Approved),
    );
    let baseline = make_baseline(&corpus, recipe.digest(), input.clone(), &committed_evals);

    // Equal: same record, same approved state, clean application.
    let patches = [patch.clone()];
    let report = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input.clone(), &graph, &patches, &committed_evals),
    )
    .expect("equal patch planes compare");
    assert!(report.is_equal(), "{:?}", report.findings);

    // Removed: the candidate plane drops the patch.
    let report = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input.clone(), &graph, &[], &committed_evals),
    )
    .expect("removed patch compares");
    assert!(categories(&report).contains(&DriftCategory::PatchRemoved));

    // Changed: the candidate record alters one operation, moving
    // the reviewed-content digest.
    let changed = patch_for(&graph, "hello?");
    let changed_patches = [changed];
    let report = compare_reingestion(
        &baseline,
        &make_candidate(
            &recipe,
            input.clone(),
            &graph,
            &changed_patches,
            &committed_evals,
        ),
    )
    .expect("changed patch compares");
    assert!(categories(&report).contains(&DriftCategory::PatchChanged));

    // Stale and rejected states report, and the state change
    // between the planes is its own category.
    let mut stale_evals = BTreeMap::new();
    stale_evals.insert(patch.uid.clone(), evaluation(&patch, PatchLifecycle::Stale));
    let report = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input.clone(), &graph, &patches, &stale_evals),
    )
    .expect("stale patch compares");
    let found = categories(&report);
    assert!(found.contains(&DriftCategory::PatchStale));
    assert!(found.contains(&DriftCategory::ReviewStateChanged));
    assert!(!found.contains(&DriftCategory::PatchRejected));

    let mut rejected_evals = BTreeMap::new();
    rejected_evals.insert(
        patch.uid.clone(),
        evaluation(&patch, PatchLifecycle::Rejected),
    );
    let report = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input, &graph, &patches, &rejected_evals),
    )
    .expect("rejected patch compares");
    let found = categories(&report);
    assert!(found.contains(&DriftCategory::PatchRejected));
    assert!(found.contains(&DriftCategory::ReviewStateChanged));
}

#[test]
fn approved_patch_with_moved_bindings_is_unappliable_and_never_alters_effective() {
    let graph = base_graph();
    let patch = patch_for(&graph, "hello!");
    let corpus = committed_corpus(&graph, Some(&patch));
    let recipe = fixture_recipe();
    let mut moved_recipe = fixture_recipe();
    moved_recipe.adapter_version = "2".to_string();
    let input = structural(patch_testkit::INPUT_HEX);
    let mut evaluations = BTreeMap::new();
    evaluations.insert(
        patch.uid.clone(),
        evaluation(&patch, PatchLifecycle::Approved),
    );
    let baseline = make_baseline(&corpus, recipe.digest(), input.clone(), &evaluations);
    let patches = [patch.clone()];
    // The candidate recipe moved: the approved patch's recipe
    // binding no longer matches, so it cannot apply — but the
    // finding keeps it visible and the candidate effective graph
    // simply excludes it.
    let candidate = make_candidate(&moved_recipe, input, &graph, &patches, &evaluations);
    let report = compare_reingestion(&baseline, &candidate).expect("binding drift compares");
    let found = categories(&report);
    assert!(found.contains(&DriftCategory::RecipeChangedOrUnavailable));
    assert!(found.contains(&DriftCategory::PatchUnappliable));
    assert!(found.contains(&DriftCategory::EffectiveGraphChanged));
}

#[test]
fn malformed_and_duplicate_candidate_patches_degrade_to_findings() {
    let graph = base_graph();
    let patch = patch_for(&graph, "hello!");
    let corpus = committed_corpus(&graph, Some(&patch));
    let recipe = fixture_recipe();
    let input = structural(patch_testkit::INPUT_HEX);
    let mut evaluations = BTreeMap::new();
    evaluations.insert(
        patch.uid.clone(),
        evaluation(&patch, PatchLifecycle::Candidate),
    );
    let baseline = make_baseline(&corpus, recipe.digest(), input.clone(), &evaluations);

    // A corrupted reviewed-content digest is one finding, and an
    // independent added patch still reports afterwards.
    let mut malformed = patch.clone();
    malformed.reviewed_content_digest = structural(&"1".repeat(64));
    let mut added = patch_for(&graph, "hello!?");
    added.uid = "patch_00000000-0000-4000-8000-0000000000b2".to_string();
    added.human_id = "fix-other".to_string();
    added.reviewed_content_digest = reviewed_content_digest(&added);
    let mut candidate_evals = evaluations.clone();
    candidate_evals.insert(
        added.uid.clone(),
        evaluation(&added, PatchLifecycle::Candidate),
    );
    let patches = [malformed, added];
    let report = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input.clone(), &graph, &patches, &candidate_evals),
    )
    .expect("malformed patch compares");
    let found = categories(&report);
    assert!(
        found.contains(&DriftCategory::PatchUnappliable),
        "{found:?}"
    );
    assert!(found.contains(&DriftCategory::PatchAdded), "{found:?}");

    // Two distinct records sharing one candidate uid report once,
    // deterministically, and suppress nothing else.
    let mut duplicate = patch.clone();
    duplicate.rationale = "a different rationale".to_string();
    duplicate.reviewed_content_digest = structural(&"2".repeat(64));
    let patches = [patch.clone(), duplicate];
    let report = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input, &graph, &patches, &evaluations),
    )
    .expect("duplicate uid compares");
    let unappliable = report
        .findings
        .iter()
        .filter(|finding| finding.category == DriftCategory::PatchUnappliable)
        .count();
    assert_eq!(unappliable, 1);
}

#[test]
fn invalid_prerequisites_refuse_before_comparison() {
    let (corpus, graph, input, evaluations) = fixture();
    let recipe = fixture_recipe();
    let baseline = make_baseline(&corpus, recipe.digest(), input.clone(), &evaluations);

    // Unknown source revision.
    let unknown = DriftBaseline {
        source_revision_uid: "src_00000000-0000-4000-8000-0000000000ff",
        ..make_baseline(&corpus, recipe.digest(), input.clone(), &evaluations)
    };
    let result = compare_reingestion(
        &unknown,
        &make_candidate(&recipe, input.clone(), &graph, &[], &evaluations),
    );
    assert!(matches!(
        result,
        Err(DriftError::UnknownSourceRevision { .. })
    ));

    // Candidate node bound to another revision.
    let mut foreign = base_graph();
    let mut node = foreign.node_mut(PARA).expect("paragraph").clone();
    node.source_revision_uid = "src_00000000-0000-4000-8000-0000000000ff".to_string();
    foreign.remove_node(PARA);
    foreign.insert(node).unwrap();
    let result = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input.clone(), &foreign, &[], &evaluations),
    );
    assert!(matches!(
        result,
        Err(DriftError::CandidateRevisionMismatch { .. })
    ));

    // Candidate graph failing standalone validation.
    let mut broken = base_graph();
    let mut node = broken.node_mut(PARA).expect("paragraph").clone();
    node.content_sha256 = structural(&"f".repeat(64));
    broken.remove_node(PARA);
    broken.insert(node).unwrap();
    let result = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input.clone(), &broken, &[], &evaluations),
    );
    assert!(matches!(result, Err(DriftError::InvalidCandidateGraph(_))));

    // Missing committed evaluation.
    let graph = base_graph();
    let patch = patch_for(&graph, "hello!");
    let corpus = committed_corpus(&graph, Some(&patch));
    let empty_evals = BTreeMap::new();
    let baseline = make_baseline(&corpus, recipe.digest(), input.clone(), &empty_evals);
    let result = compare_reingestion(
        &baseline,
        &make_candidate(&recipe, input, &graph, &[], &empty_evals),
    );
    assert!(matches!(
        result,
        Err(DriftError::MissingPatchEvaluation {
            plane: "committed",
            ..
        })
    ));

    // Invalid committed baseline: the patch's pre-graph binding
    // does not recompute against the committed graph.
    let mut bad_patch = patch_for(&graph, "hello!");
    bad_patch.pre_patch_graph_digest = structural(&"9".repeat(64));
    bad_patch.reviewed_content_digest = reviewed_content_digest(&bad_patch);
    let mut bad_corpus = CorpusGraph::new();
    bad_corpus.insert(patch_testkit::revision_node()).unwrap();
    for source_node in graph.nodes() {
        bad_corpus.insert_source_node(source_node.clone()).unwrap();
    }
    bad_corpus.insert_source_patch(bad_patch).unwrap();
    let baseline = make_baseline(
        &bad_corpus,
        recipe.digest(),
        structural(patch_testkit::INPUT_HEX),
        &empty_evals,
    );
    let result = compare_reingestion(
        &baseline,
        &make_candidate(
            &recipe,
            structural(patch_testkit::INPUT_HEX),
            &graph,
            &[],
            &empty_evals,
        ),
    );
    assert!(matches!(result, Err(DriftError::InvalidBaseline(_))));
}

#[test]
fn findings_sort_by_category_path_and_uid() {
    let (corpus, _graph, input, evaluations) = fixture();
    let recipe = fixture_recipe();
    let baseline = make_baseline(&corpus, recipe.digest(), input.clone(), &evaluations);
    // Recipe drift plus node drift: the recipe category ranks
    // before every node category regardless of discovery order.
    let mut moved_recipe = fixture_recipe();
    moved_recipe.parser = "comrak".to_string();
    let mut text_moved = base_graph();
    let mut paragraph = text_moved.node_mut(PARA).expect("paragraph").clone();
    paragraph.canonical_text = "goodbye".to_string();
    paragraph.content_sha256 = content_digest(SourceNodeKind::Paragraph, "goodbye");
    text_moved.remove_node(PARA);
    text_moved.insert(paragraph).unwrap();
    let report = compare_reingestion(
        &baseline,
        &make_candidate(&moved_recipe, input, &text_moved, &[], &evaluations),
    )
    .expect("mixed drift compares");
    let keys: Vec<DriftCategory> = report
        .findings
        .iter()
        .map(|finding| finding.category)
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "findings are emitted in sort order");
    assert_eq!(keys[0], DriftCategory::RecipeChangedOrUnavailable);
}

#[test]
fn comparison_leaves_every_input_untouched() {
    let graph = base_graph();
    let patch = patch_for(&graph, "hello!");
    let corpus = committed_corpus(&graph, Some(&patch));
    let recipe = fixture_recipe();
    let input = structural(patch_testkit::INPUT_HEX);
    let mut evaluations = BTreeMap::new();
    evaluations.insert(
        patch.uid.clone(),
        evaluation(&patch, PatchLifecycle::Approved),
    );
    let corpus_before = corpus.clone();
    let graph_before = graph.clone();
    let patches = [patch.clone()];
    let baseline = make_baseline(&corpus, recipe.digest(), input.clone(), &evaluations);
    let candidate = make_candidate(&recipe, input, &graph, &patches, &evaluations);
    let report = compare_reingestion(&baseline, &candidate).expect("comparison runs");
    assert!(report.is_equal());
    assert_eq!(corpus, corpus_before, "the committed corpus is untouched");
    assert_eq!(graph, graph_before, "the candidate graph is untouched");
}
