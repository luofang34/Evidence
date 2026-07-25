//! Tests for the approval-gated effective source graph: only
//! currently approved patches contribute, application is
//! deterministic, and every degenerate case fails closed
//! (TEST-191).

use super::*;
use crate::corpus::graph::ReviewDecision;
use crate::corpus::patch_testkit as kit;
use crate::corpus::{PatchLifecycle, evaluate_patch_lifecycle};

fn bindings() -> PatchBindings {
    PatchBindings {
        recipe_digest: kit::structural(kit::RECIPE_HEX),
        input_digest: kit::structural(kit::INPUT_HEX),
    }
}

fn approve_current(patch: &crate::corpus::SourcePatchRecord) -> crate::corpus::ReviewNode {
    kit::patch_review(
        kit::REV_1,
        "REV-001",
        &patch.uid,
        patch.reviewed_content_digest.as_str(),
        ReviewDecision::Approve,
        "alice@example.com",
        None,
    )
}

/// An approved patch contributes to the effective graph;
/// candidate, rejected, and stale patches never do; wrong
/// presented bindings and unknown revisions fail closed
/// (TEST-191).
#[test]
fn only_approved_patches_contribute_and_fail_closed() {
    let patch = kit::patch_record(kit::PATCH_A, "PATCH-001");

    // Candidate: no reviews — the effective graph is the parser
    // graph, no patch contributes.
    let graph = kit::graph_with(patch.clone(), vec![]);
    let effective = effective_source_graph(&graph, kit::REVISION, &bindings(), kit::MEDIA)
        .expect("candidate corpus computes");
    assert!(effective.applied_patch_uids.is_empty());
    assert!(effective.graph.get(kit::INSERTED).is_none());
    assert_eq!(
        effective.source_revision_uid, kit::REVISION,
        "the result names its revision"
    );

    // Rejected: a current-digest rejection never contributes.
    let rejected = kit::patch_review(
        kit::REV_1,
        "REV-001",
        kit::PATCH_A,
        patch.reviewed_content_digest.as_str(),
        ReviewDecision::Reject,
        "alice@example.com",
        None,
    );
    let graph = kit::graph_with(patch.clone(), vec![rejected]);
    let effective = effective_source_graph(&graph, kit::REVISION, &bindings(), kit::MEDIA)
        .expect("rejected corpus computes");
    assert!(effective.applied_patch_uids.is_empty());
    assert!(effective.graph.get(kit::INSERTED).is_none());

    // Stale: an older-digest approval never contributes.
    let stale = kit::patch_review(
        kit::REV_1,
        "REV-001",
        kit::PATCH_A,
        &"a".repeat(64),
        ReviewDecision::Approve,
        "alice@example.com",
        None,
    );
    let graph = kit::graph_with(patch.clone(), vec![stale]);
    let effective = effective_source_graph(&graph, kit::REVISION, &bindings(), kit::MEDIA)
        .expect("stale corpus computes");
    assert!(effective.applied_patch_uids.is_empty());
    assert!(effective.graph.get(kit::INSERTED).is_none());

    // Approved: the patch contributes — the inserted node is
    // present, the parser graph is untouched, and the applied
    // patch uid records the approval proof.
    let graph = kit::graph_with(patch.clone(), vec![approve_current(&patch)]);
    let effective = effective_source_graph(&graph, kit::REVISION, &bindings(), kit::MEDIA)
        .expect("approved corpus computes");
    assert_eq!(
        effective.applied_patch_uids,
        vec![kit::PATCH_A.to_string()]
    );
    assert!(
        effective.graph.get(kit::INSERTED).is_some(),
        "the approved patch's inserted node is effective"
    );
    assert!(
        graph
            .source_graph(kit::REVISION)
            .is_none_or(|committed| committed.get(kit::INSERTED).is_none()),
        "the committed parser graph is never mutated"
    );

    // Determinism: review insertion order never affects output.
    let mut correction = kit::patch_review(
        kit::REV_2,
        "REV-002",
        kit::PATCH_A,
        patch.reviewed_content_digest.as_str(),
        ReviewDecision::Approve,
        "bob@example.com",
        None,
    );
    correction.reviewer = "bob@example.com".to_string();
    let first = approve_current(&patch);
    let forward = kit::graph_with(patch.clone(), vec![first.clone(), correction.clone()]);
    let reverse = kit::graph_with(patch.clone(), vec![correction, first]);
    let forward_effective =
        effective_source_graph(&forward, kit::REVISION, &bindings(), kit::MEDIA).expect("forward");
    let reverse_effective =
        effective_source_graph(&reverse, kit::REVISION, &bindings(), kit::MEDIA).expect("reverse");
    assert_eq!(forward_effective, reverse_effective);

    // Fail closed: the presented recipe binding no longer matches
    // the approved patch.
    let wrong = PatchBindings {
        recipe_digest: kit::structural(&"1".repeat(64)),
        input_digest: kit::structural(kit::INPUT_HEX),
    };
    let graph = kit::graph_with(patch.clone(), vec![approve_current(&patch)]);
    let err = effective_source_graph(&graph, kit::REVISION, &wrong, kit::MEDIA)
        .expect_err("a stale recipe binding fails closed");
    assert!(
        matches!(
            err,
            EffectiveGraphError::ApprovedPatchApplication { ref patch_uid, .. }
                if patch_uid == kit::PATCH_A
        ),
        "stale binding, got: {err:?}"
    );

    // Fail closed: unknown revision.
    let err = effective_source_graph(
        &graph,
        "src_00000000-0000-4000-8000-0000000000ff",
        &bindings(),
        kit::MEDIA,
    )
    .expect_err("an unknown revision fails closed");
    assert!(matches!(
        err,
        EffectiveGraphError::UnknownSourceRevision { .. }
    ));

    // The committed patch plane is unchanged by evaluation: the
    // approval is still derived, never stored.
    let evaluation =
        evaluate_patch_lifecycle(&graph, kit::PATCH_A).expect("lifecycle evaluates");
    assert_eq!(evaluation.state, PatchLifecycle::Approved);
    assert!(
        graph.get(kit::PATCH_A).is_none(),
        "patches live in their own plane, never as nodes"
    );
}
