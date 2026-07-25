//! Tests for the approval boundary's curated patch plane: any
//! effective curated content is proven approved and producible,
//! and requirement claim behavior is identical (TEST-191).

use super::fixtures::{REQ, insert_with_claims, requirement};
use super::{
    ApprovalBoundaryError, ApprovalBoundaryViolation, LifecycleEnforcement,
    validate_approval_boundary,
};
use crate::corpus::{CorpusGraph, Node, RequirementLifecycle};

fn validate(graph: &CorpusGraph) -> Result<(), ApprovalBoundaryError> {
    validate_approval_boundary(graph, LifecycleEnforcement::Required)
}

fn expect_violations(result: Result<(), ApprovalBoundaryError>) -> Vec<ApprovalBoundaryViolation> {
    match result {
        Err(ApprovalBoundaryError::Violations { violations }) => violations,
        other => panic!("expected aggregated violations, got: {other:?}"),
    }
}

/// The patch plane under explicit enforcement: an approved patch
/// that applies cleanly passes, candidate/rejected/stale patches
/// are no violation, an approved patch that cannot become
/// effective fails closed, and requirement claim behavior is
/// identical with patches present (TEST-191).
#[test]
fn effective_curated_content_proven_approved() {
    use crate::corpus::patch_testkit as kit;
    use crate::corpus::{ReviewDecision, SourceMaterial, SourceRevisionNode};

    let patch = kit::patch_record(kit::PATCH_A, "PATCH-001");
    let review = |decision, digest: &str| {
        kit::patch_review(
            kit::REV_1,
            "REV-001",
            kit::PATCH_A,
            digest,
            decision,
            "alice@example.com",
            None,
        )
    };
    let current = patch.reviewed_content_digest.as_str().to_string();

    // Approved and producible: no violation.
    let graph = kit::graph_with(
        patch.clone(),
        vec![review(ReviewDecision::Approve, &current)],
    );
    validate(&graph).expect("an approved, cleanly applying patch passes the boundary");

    // Candidate, rejected, and stale patches contribute nothing
    // and are no violation.
    for (name, candidate) in [
        ("rejected", review(ReviewDecision::Reject, &current)),
        ("stale", review(ReviewDecision::Approve, &"a".repeat(64))),
    ] {
        let graph = kit::graph_with(patch.clone(), vec![candidate]);
        validate(&graph).unwrap_or_else(|err| panic!("a {name} patch is no violation: {err}"));
    }
    let graph = kit::graph_with(patch.clone(), vec![]);
    validate(&graph).expect("a candidate patch is no violation");

    // Approved but not producible — the revision's media type
    // disagrees with the inserted node's locator, so the post-patch
    // graph is invalid — fails closed with the typed source chain,
    // while the corpus itself still validates.
    let html_revision = Node::SourceRevision(SourceRevisionNode {
        uid: kit::REVISION.to_string(),
        id: "DOC-1".to_string(),
        document_key: "doc".to_string(),
        title: "fixture document".to_string(),
        media_type: "text/html".to_string(),
        canonical_location: "https://example.org/doc/rev-a".to_string(),
        material: SourceMaterial::Unavailable {
            reason: "fixture".to_string(),
        },
        edges: Vec::new(),
    });
    let mut graph = CorpusGraph::new();
    graph.insert(html_revision).unwrap();
    graph.insert_source_patch(patch.clone()).unwrap();
    graph
        .insert(Node::Review(review(ReviewDecision::Approve, &current)))
        .unwrap();
    graph.validate().expect("the corpus itself validates");
    let err = validate(&graph).expect_err("an approved patch that cannot apply must fail closed");
    assert!(
        matches!(
            err,
            ApprovalBoundaryError::EffectiveCuratedContent { ref patch_uid, .. }
                if patch_uid == kit::PATCH_A
        ),
        "unproducible approved content, got: {err:?}"
    );

    // Requirement claim behavior is identical with patches
    // present: an unapproved requirement claim still violates with
    // the unchanged violation shape.
    let mut graph = CorpusGraph::new();
    let req = requirement(REQ, "normative text");
    insert_with_claims(&mut graph, req, &["evidence_core::corpus::x"], &[]);
    graph.insert(kit::revision_node()).unwrap();
    graph.insert_source_patch(patch).unwrap();
    let err = validate(&graph).expect_err("an unapproved requirement claim still violates");
    let violations = expect_violations(Err(err));
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].requirement_uid, REQ);
    assert_eq!(violations[0].state, RequirementLifecycle::Candidate);
}
