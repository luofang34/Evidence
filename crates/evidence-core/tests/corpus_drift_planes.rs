//! Drift comparison boundary acceptance tests (TEST-194):
//! invalid prerequisites fail before comparison with typed
//! context, one malformed patch never suppresses later independent
//! findings, stale and unapproved patches never alter effective
//! output while remaining visible as patch and review drift, and
//! comparison leaves every input byte-identical.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::BTreeMap;

use evidence_core::corpus::{
    CorpusGraph, DriftBaseline, DriftCategory, DriftError, EdgeKind, Node, PatchLifecycle,
    PatchLifecycleEvaluation, ReviewDecision, ReviewNode, ReviewTarget, SourceNodeKind,
    SourcePatchRecord, compare_reingestion, evaluate_all_patch_lifecycles, reviewed_content_digest,
};

#[path = "corpus_drift_testkit.rs"]
mod testkit;

use testkit::{
    NOTE, PARA, PATCH_A, base_nodes, baseline, candidate, categories, committed_patches,
    fixture_recipe, graph_from, load_corpus, structural, write,
};

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

/// Invalid prerequisites fail before any comparison with typed
/// context (TEST-194).
#[test]
fn invalid_prerequisites_fail_before_comparison() {
    let corpus = load_corpus(false);
    let evaluations = evaluate_all_patch_lifecycles(&corpus).expect("evaluations");
    let recipe = fixture_recipe();
    let graph = graph_from(base_nodes());
    let patches = committed_patches(&corpus);

    // A committed baseline that fails validation: a review with a
    // dangling `Reviews` edge inserted through the public graph.
    // The copied review nodes already dangle without the patch
    // plane (unreachable through the public API), so `validate`
    // fails closed either way.
    let mut broken = CorpusGraph::new();
    for node in corpus.nodes() {
        broken.insert(node.clone()).expect("node inserts");
    }
    broken
        .insert(Node::Review(ReviewNode {
            uid: "rev_00000000-0000-4000-8000-0000000000ff".to_string(),
            id: "REV-099".to_string(),
            target: ReviewTarget::CuratedPatch(
                "patch_00000000-0000-4000-8000-0000000000ff".to_string(),
            ),
            content_schema: 1,
            reviewed_content_sha256: evidence_core::corpus::ReviewContentDigest::from_hex(
                &"a".repeat(64),
            )
            .expect("hex"),
            decision: ReviewDecision::Approve,
            reviewer: "mallory@example.com".to_string(),
            reviewed_at: "2026-07-01T10:00:00Z".to_string(),
            rationale: None,
            edges: vec![(
                EdgeKind::Reviews,
                "patch_00000000-0000-4000-8000-0000000000ff".to_string(),
            )],
        }))
        .expect("dangling review inserts");
    // The broken graph also needs the patch plane to validate; the
    // dangling review edge is enough to fail `validate`.
    let result = compare_reingestion(
        &baseline(&broken, &evaluations),
        &candidate(&recipe, &graph, &patches, &evaluations),
    );
    assert!(
        matches!(result, Err(DriftError::InvalidBaseline(_))),
        "got: {result:?}"
    );

    // Unknown source revision.
    let unknown = DriftBaseline {
        source_revision_uid: "src_00000000-0000-4000-8000-0000000000ff",
        ..baseline(&corpus, &evaluations)
    };
    let result = compare_reingestion(
        &unknown,
        &candidate(&recipe, &graph, &patches, &evaluations),
    );
    assert!(matches!(
        result,
        Err(DriftError::UnknownSourceRevision { .. })
    ));

    // Candidate node bound to another revision.
    let mut foreign_nodes = base_nodes();
    for node in &mut foreign_nodes {
        if node.uid == PARA {
            node.source_revision_uid = "src_00000000-0000-4000-8000-0000000000ff".to_string();
        }
    }
    let foreign = graph_from(foreign_nodes);
    let result = compare_reingestion(
        &baseline(&corpus, &evaluations),
        &candidate(&recipe, &foreign, &patches, &evaluations),
    );
    assert!(matches!(
        result,
        Err(DriftError::CandidateRevisionMismatch { .. })
    ));

    // Candidate graph failing standalone validation: a corrupted
    // stored digest.
    let mut corrupt_nodes = base_nodes();
    for node in &mut corrupt_nodes {
        if node.uid == PARA {
            node.content_sha256 = structural(&"f".repeat(64));
        }
    }
    let corrupt = graph_from(corrupt_nodes);
    let result = compare_reingestion(
        &baseline(&corpus, &evaluations),
        &candidate(&recipe, &corrupt, &patches, &evaluations),
    );
    assert!(matches!(result, Err(DriftError::InvalidCandidateGraph(_))));

    // A committed patch with no review evaluation.
    let empty = BTreeMap::new();
    let result = compare_reingestion(
        &baseline(&corpus, &empty),
        &candidate(&recipe, &graph, &patches, &evaluations),
    );
    assert!(matches!(
        result,
        Err(DriftError::MissingPatchEvaluation {
            plane: "committed",
            ..
        })
    ));
}

/// One malformed candidate patch degrades to its own finding and
/// never suppresses later independent findings (TEST-194).
#[test]
fn malformed_patch_never_suppresses_later_findings() {
    let corpus = load_corpus(false);
    let evaluations = evaluate_all_patch_lifecycles(&corpus).expect("evaluations");
    let recipe = fixture_recipe();
    let mut malformed = committed_patches(&corpus);
    malformed[0].reviewed_content_digest = structural(&"1".repeat(64));
    // A second, independent candidate-only patch follows the
    // malformed one; the paragraph's text drifts too.
    let mut added = testkit::patch_record();
    added.uid = "patch_00000000-0000-4000-8000-0000000000b2".to_string();
    added.human_id = "fix-note".to_string();
    added.operations = vec![evidence_core::corpus::PatchOperation::ReplaceContent {
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
    added.reviewed_content_digest = reviewed_content_digest(&added);
    let mut candidate_evals = evaluations.clone();
    candidate_evals.extend(eval_in(&added, PatchLifecycle::Candidate));
    let patches = [malformed, vec![added]].concat();
    let mut drifted_nodes = base_nodes();
    for node in &mut drifted_nodes {
        if node.uid == PARA {
            node.canonical_text = "goodbye".to_string();
            node.content_sha256 =
                evidence_core::corpus::content_digest(SourceNodeKind::Paragraph, "goodbye");
        }
    }
    let drifted = graph_from(drifted_nodes);
    let report = compare_reingestion(
        &baseline(&corpus, &evaluations),
        &candidate(&recipe, &drifted, &patches, &candidate_evals),
    )
    .expect("malformed patch aggregates");
    let found = categories(&report.findings);
    assert!(
        found.contains(&DriftCategory::PatchUnappliable),
        "{found:?}"
    );
    assert!(found.contains(&DriftCategory::PatchAdded), "{found:?}");
    assert!(
        found.contains(&DriftCategory::NodeCanonicalTextChanged),
        "{found:?}"
    );
}

/// Stale and unapproved patches never alter effective output but
/// remain visible as patch and review drift (TEST-194).
#[test]
fn stale_unapproved_patches_never_alter_effective_output() {
    // A corpus without reviews: the committed patch is a
    // candidate, so neither effective graph applies it.
    let dir = tempfile::tempdir().expect("tempdir");
    let patch = testkit::patch_record();
    write(&dir.path().join("sources.toml"), &testkit::source_toml());
    write(
        &dir.path().join("graphs.toml"),
        &testkit::graphs_toml(&base_nodes()),
    );
    write(
        &dir.path().join("patches.toml"),
        &testkit::patch_toml(&patch),
    );
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources.toml\"]\n\
         source_graphs = [\"graphs.toml\"]\nsource_patches = [\"patches.toml\"]\n",
    );
    let corpus = evidence_core::corpus::CorpusIndex::load_graph(&dir.path().join("corpus.toml"))
        .expect("unreviewed corpus loads");
    let committed_evals = evaluate_all_patch_lifecycles(&corpus).expect("evaluations");
    assert_eq!(
        committed_evals[PATCH_A].state,
        PatchLifecycle::Candidate,
        "no reviews: the patch is a candidate"
    );
    let recipe = fixture_recipe();
    let graph = graph_from(base_nodes());
    let patches = committed_patches(&corpus);
    // The candidate plane evaluates the patch stale: visible as
    // patch and review drift, but neither effective graph changes,
    // so no effective-graph finding fires.
    let stale = eval_in(&patches[0], PatchLifecycle::Stale);
    let report = compare_reingestion(
        &baseline(&corpus, &committed_evals),
        &candidate(&recipe, &graph, &patches, &stale),
    )
    .expect("stale patch compares");
    let found = categories(&report.findings);
    assert!(found.contains(&DriftCategory::PatchStale), "{found:?}");
    assert!(
        found.contains(&DriftCategory::ReviewStateChanged),
        "{found:?}"
    );
    assert!(
        !found.contains(&DriftCategory::EffectiveGraphChanged),
        "a stale patch never alters effective output: {found:?}"
    );
}

/// Comparison leaves every input byte-identical (TEST-194).
#[test]
fn comparison_leaves_inputs_byte_identical() {
    let dir = tempfile::tempdir().expect("tempdir");
    let patch = testkit::patch_record();
    write(&dir.path().join("sources.toml"), &testkit::source_toml());
    write(
        &dir.path().join("graphs.toml"),
        &testkit::graphs_toml(&base_nodes()),
    );
    write(
        &dir.path().join("patches.toml"),
        &testkit::patch_toml(&patch),
    );
    write(
        &dir.path().join("reviews.toml"),
        &format!(
            "schema_version = 2\n{}",
            testkit::review_toml(
                testkit::REV_1,
                "REV-001",
                "alice@example.com",
                patch.reviewed_content_digest.as_str(),
            )
        ),
    );
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources.toml\"]\n\
         source_graphs = [\"graphs.toml\"]\nsource_patches = [\"patches.toml\"]\n\
         reviews = [\"reviews.toml\"]\n",
    );
    let snapshot = |root: &std::path::Path| {
        let mut hashes = BTreeMap::new();
        for entry in walkdir::WalkDir::new(root).follow_links(false) {
            let entry = entry.expect("walk");
            if entry.file_type().is_file() {
                hashes.insert(
                    entry.path().to_path_buf(),
                    evidence_core::hash::sha256_file(entry.path()).expect("hash"),
                );
            }
        }
        hashes
    };
    let before = snapshot(dir.path());
    let corpus = evidence_core::corpus::CorpusIndex::load_graph(&dir.path().join("corpus.toml"))
        .expect("corpus loads");
    let corpus_before = corpus.clone();
    let evaluations = evaluate_all_patch_lifecycles(&corpus).expect("evaluations");
    let recipe = fixture_recipe();
    let graph = graph_from(base_nodes());
    let graph_before = graph.clone();
    let patches = committed_patches(&corpus);
    let report = compare_reingestion(
        &baseline(&corpus, &evaluations),
        &candidate(&recipe, &graph, &patches, &evaluations),
    )
    .expect("comparison runs");
    assert!(report.is_equal());
    assert_eq!(
        snapshot(dir.path()),
        before,
        "every input file is byte-identical"
    );
    assert_eq!(corpus, corpus_before, "the committed corpus is untouched");
    assert_eq!(graph, graph_before, "the candidate graph is untouched");
    // A drifted candidate mutates nothing either.
    let mut drifted_nodes = base_nodes();
    drifted_nodes.truncate(2);
    let drifted = graph_from(drifted_nodes);
    let report = compare_reingestion(
        &baseline(&corpus, &evaluations),
        &candidate(&recipe, &drifted, &patches, &evaluations),
    )
    .expect("drifted comparison runs");
    assert!(!report.is_equal());
    assert_eq!(snapshot(dir.path()), before, "drift reporting never writes");
    assert_eq!(corpus, corpus_before);
}
