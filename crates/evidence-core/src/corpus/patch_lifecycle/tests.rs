//! Tests for deterministic per-patch lifecycle evaluation: the
//! requirement truth table over digest-bound review heads, semantic
//! mutation staleness, and layout independence (TEST-190).

use super::*;
use crate::corpus::graph::{EdgeKind, Node, ReviewDecision, ReviewNode};
use crate::corpus::patch_testkit as kit;
use crate::corpus::source_patch::digest::reviewed_content_digest;
use crate::corpus::source_patch::records::SourcePatchRecord;
use crate::corpus::{CorpusGraph, ReviewTarget};

fn approve(uid: &str, id: &str, digest: &str) -> ReviewNode {
    kit::patch_review(
        uid,
        id,
        kit::PATCH_A,
        digest,
        ReviewDecision::Approve,
        "alice@example.com",
        None,
    )
}

fn reject(uid: &str, id: &str, digest: &str, reviewer: &str) -> ReviewNode {
    kit::patch_review(
        uid,
        id,
        kit::PATCH_A,
        digest,
        ReviewDecision::Reject,
        reviewer,
        None,
    )
}

fn fixture() -> (SourcePatchRecord, String) {
    let patch = kit::patch_record(kit::PATCH_A, "PATCH-001");
    let digest = patch.reviewed_content_digest.as_str().to_string();
    (patch, digest)
}

/// The patch truth table matches requirements: current-digest
/// rejection beats current-digest approval beats older-digest
/// approval beats nothing; superseded heads decide nothing;
/// conflicting current decisions are never approved (TEST-190).
#[test]
fn truth_table_and_supersession_match_requirements() {
    let (patch, digest) = fixture();
    let older = "a".repeat(64);

    // Candidate: no reviews at all.
    let evaluation =
        evaluate_patch_lifecycle(&kit::graph_with(patch.clone(), vec![]), kit::PATCH_A)
            .expect("no-review graph evaluates");
    assert_eq!(evaluation.state, PatchLifecycle::Candidate);
    assert!(evaluation.effective_review_uids.is_empty());
    assert_eq!(evaluation.current_digest.as_str(), digest);

    // Approved: one current-digest approval.
    let graph = kit::graph_with(patch.clone(), vec![approve(kit::REV_1, "REV-001", &digest)]);
    let evaluation = evaluate_patch_lifecycle(&graph, kit::PATCH_A).expect("approved evaluates");
    assert_eq!(evaluation.state, PatchLifecycle::Approved);
    assert_eq!(
        evaluation.effective_review_uids,
        vec![kit::REV_1.to_string()]
    );

    // Rejected: a current-digest rejection, with precedence over a
    // current-digest approval by another reviewer — conflicting
    // current decisions are never approved.
    let graph = kit::graph_with(
        patch.clone(),
        vec![
            approve(kit::REV_1, "REV-001", &digest),
            reject(kit::REV_2, "REV-002", &digest, "bob@example.com"),
        ],
    );
    let evaluation = evaluate_patch_lifecycle(&graph, kit::PATCH_A).expect("rejected evaluates");
    assert_eq!(evaluation.state, PatchLifecycle::Rejected);

    // Stale: an approval of an older digest only.
    let graph = kit::graph_with(patch.clone(), vec![approve(kit::REV_1, "REV-001", &older)]);
    let evaluation = evaluate_patch_lifecycle(&graph, kit::PATCH_A).expect("stale evaluates");
    assert_eq!(evaluation.state, PatchLifecycle::Stale);

    // Candidate again: revised content whose only older decisions
    // were rejections — an older-digest rejection never stigmatizes
    // new content.
    let graph = kit::graph_with(
        patch.clone(),
        vec![reject(kit::REV_1, "REV-001", &older, "alice@example.com")],
    );
    let evaluation = evaluate_patch_lifecycle(&graph, kit::PATCH_A).expect("revised evaluates");
    assert_eq!(evaluation.state, PatchLifecycle::Candidate);

    // Supersession: one reviewer's correction replaces their
    // approval with a rejection of the same digest; the superseded
    // approval decides nothing.
    let mut correction = reject(kit::REV_2, "REV-002", &digest, "alice@example.com");
    correction
        .edges
        .push((EdgeKind::Supersedes, kit::REV_1.to_string()));
    let graph = kit::graph_with(
        patch.clone(),
        vec![approve(kit::REV_1, "REV-001", &digest), correction],
    );
    let evaluation = evaluate_patch_lifecycle(&graph, kit::PATCH_A).expect("corrected evaluates");
    assert_eq!(evaluation.state, PatchLifecycle::Rejected);
    assert_eq!(
        evaluation.effective_review_uids,
        vec![kit::REV_2.to_string()],
        "the superseded approval is no longer a head"
    );

    // evaluate_all covers every committed patch, keyed in uid order.
    let evaluations = evaluate_all_patch_lifecycles(&graph).expect("evaluate_all evaluates");
    assert_eq!(evaluations.len(), 1);
    assert_eq!(
        evaluations.get(kit::PATCH_A).map(|e| e.state),
        Some(PatchLifecycle::Rejected)
    );

    // Missing patch, no reviews: PatchMissing. A review targeting a
    // missing patch fails validation first (defense in depth).
    let err = evaluate_patch_lifecycle(&CorpusGraph::new(), kit::PATCH_A)
        .expect_err("a missing patch fails closed");
    assert!(matches!(err, PatchLifecycleError::PatchMissing { .. }));
}

/// Every semantic patch mutation — recipe, input, operation
/// content, precondition, and ordinal — moves the reviewed-content
/// digest and makes an approval stale; audit metadata mutations do
/// not; source and pre-graph binding mutations fail the corpus
/// itself closed (TEST-190).
#[test]
fn every_semantic_mutation_stales_approval() {
    let (patch, digest) = fixture();
    let approved_graph = |patch: &SourcePatchRecord| {
        kit::graph_with(
            patch.clone(),
            vec![approve(
                kit::REV_1,
                "REV-001",
                patch.reviewed_content_digest.as_str(),
            )],
        )
    };
    let base = approved_graph(&patch);
    assert_eq!(
        evaluate_patch_lifecycle(&base, kit::PATCH_A)
            .expect("base evaluates")
            .state,
        PatchLifecycle::Approved
    );

    // The approval binds the pre-mutation digest; a mutated record
    // (as the loader would re-derive it) no longer matches.
    let approved_review = || approve(kit::REV_1, "REV-001", &digest);
    let state_after = |mutated: SourcePatchRecord| {
        let graph = kit::graph_with(mutated, vec![approved_review()]);
        evaluate_patch_lifecycle(&graph, kit::PATCH_A)
            .expect("mutated graph evaluates")
            .state
    };

    type Mutation = (&'static str, fn(&mut SourcePatchRecord));
    let semantic: [Mutation; 6] = [
        ("recipe binding", |p| {
            p.recipe_digest = kit::structural(&"1".repeat(64))
        }),
        ("input binding", |p| {
            p.input_digest = kit::structural(&"2".repeat(64))
        }),
        ("operation content", |p| {
            let crate::corpus::PatchOperation::Insert { node, .. } = &mut p.operations[0] else {
                panic!("fixture patch inserts");
            };
            node.canonical_text.push_str(" edited");
        }),
        ("inserted node kind", |p| {
            let crate::corpus::PatchOperation::Insert { node, .. } = &mut p.operations[0] else {
                panic!("fixture patch inserts");
            };
            node.kind = crate::corpus::SourceNodeKind::Paragraph;
        }),
        ("operation precondition", |p| {
            let crate::corpus::PatchOperation::Insert {
                expected_parent_uid,
                ..
            } = &mut p.operations[0]
            else {
                panic!("fixture patch inserts");
            };
            *expected_parent_uid = Some("snode_00000000-0000-4000-8000-0000000000ff".to_string());
        }),
        ("operation ordinal", |p| {
            let crate::corpus::PatchOperation::Insert { ordinal, .. } = &mut p.operations[0] else {
                panic!("fixture patch inserts");
            };
            *ordinal = 7;
        }),
    ];
    for (name, mutate) in semantic {
        let mut mutated = patch.clone();
        mutate(&mut mutated);
        mutated.reviewed_content_digest = reviewed_content_digest(&mutated);
        assert_ne!(
            mutated.reviewed_content_digest, patch.reviewed_content_digest,
            "{name}: the projection must cover the mutation"
        );
        if name == "operation precondition" {
            // A self-parented insert dangles at graph validation;
            // the corpus fails closed even before staleness.
            let graph = kit::graph_with(mutated, vec![approved_review()]);
            assert!(
                evaluate_patch_lifecycle(&graph, kit::PATCH_A).is_err(),
                "{name}: an unresolvable precondition context fails closed"
            );
            continue;
        }
        assert_eq!(
            state_after(mutated),
            PatchLifecycle::Stale,
            "{name}: the approval must stale"
        );
    }

    let audit: [Mutation; 4] = [
        ("author", |p| p.author = "other@example.com".to_string()),
        ("rationale", |p| {
            p.rationale = "different rationale".to_string()
        }),
        ("created_at", |p| {
            p.created_at = "2026-07-02T00:00:00Z".to_string()
        }),
        ("human_id", |p| p.human_id = "PATCH-RENAMED".to_string()),
    ];
    for (name, mutate) in audit {
        let mut mutated = patch.clone();
        mutate(&mut mutated);
        mutated.reviewed_content_digest = reviewed_content_digest(&mutated);
        assert_eq!(
            state_after(mutated),
            PatchLifecycle::Approved,
            "{name}: audit metadata stays outside semantic identity"
        );
    }

    // Source and pre-graph binding mutations fail the corpus itself
    // closed — stronger than staleness.
    let mut rebound = patch.clone();
    rebound.source_revision_uid = "src_00000000-0000-4000-8000-0000000000ff".to_string();
    rebound.reviewed_content_digest = reviewed_content_digest(&rebound);
    let graph = kit::graph_with(rebound, vec![approved_review()]);
    let err = evaluate_patch_lifecycle(&graph, kit::PATCH_A)
        .expect_err("an unbound revision fails the graph closed");
    assert!(matches!(err, PatchLifecycleError::InvalidGraph(_)));

    let mut re_graphed = patch.clone();
    re_graphed.pre_patch_graph_digest = kit::structural(&"3".repeat(64));
    re_graphed.reviewed_content_digest = reviewed_content_digest(&re_graphed);
    let graph = kit::graph_with(re_graphed, vec![approved_review()]);
    let err = evaluate_patch_lifecycle(&graph, kit::PATCH_A)
        .expect_err("a stale pre-graph binding fails the graph closed");
    assert!(matches!(err, PatchLifecycleError::InvalidGraph(_)));
}

/// Review insertion order, supersession chain declaration order,
/// and reviewed_at timestamps never affect the derived state or
/// the evaluation output (TEST-190).
#[test]
fn layout_and_record_order_never_affect_evaluation() {
    let (patch, digest) = fixture();
    let first = approve(kit::REV_1, "REV-001", &digest);
    let mut correction = reject(kit::REV_2, "REV-002", &digest, "alice@example.com");
    correction
        .edges
        .push((EdgeKind::Supersedes, kit::REV_1.to_string()));
    let mut independent = approve(kit::REV_3, "REV-003", &digest);
    independent.reviewer = "carol@example.com".to_string();

    let forward = kit::graph_with(
        patch.clone(),
        vec![first.clone(), correction.clone(), independent.clone()],
    );
    let reverse = kit::graph_with(patch.clone(), vec![independent, correction, first]);
    assert_eq!(
        forward, reverse,
        "review insertion order must not affect the loaded graph"
    );
    let forward_eval = evaluate_all_patch_lifecycles(&forward).expect("forward evaluates");
    let reverse_eval = evaluate_all_patch_lifecycles(&reverse).expect("reverse evaluates");
    assert_eq!(forward_eval, reverse_eval);

    // reviewed_at never picks a winner.
    let mut late = kit::graph_with(patch.clone(), vec![]);
    let mut timed = approve(kit::REV_1, "REV-001", &digest);
    timed.reviewed_at = "2030-01-01T00:00:00Z".to_string();
    late.insert(Node::Review(timed)).unwrap();
    let evaluation = evaluate_patch_lifecycle(&late, kit::PATCH_A).expect("timed evaluates");
    assert_eq!(evaluation.state, PatchLifecycle::Approved);

    // Patch-targeted reviews never leak into requirement lifecycle
    // and vice versa.
    assert!(forward.reviews_for_patch(kit::PATCH_A).len() == 3);
    assert!(forward.reviews_for_requirement(kit::PATCH_A).is_empty());
    assert!(matches!(
        forward.get(kit::REV_1),
        Some(Node::Review(review))
            if matches!(&review.target, ReviewTarget::CuratedPatch(uid) if uid == kit::PATCH_A)
    ));
}
