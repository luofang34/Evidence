//! Truth-table, determinism, and fail-closed tests for lifecycle
//! evaluation (TEST-135, TEST-136).

use super::{
    LifecycleError, LifecycleEvaluation, RequirementLifecycle, evaluate_all_lifecycles,
    evaluate_lifecycle,
};
use crate::corpus::{
    CorpusGraph, EdgeKind, Node, RequirementLayer, RequirementNode, RequirementReviewContentV1,
    ReviewContentDigest, ReviewDecision, ReviewNode, review_content_digest_v1,
};

const REQ: &str = "req_a";
const REQ_B: &str = "req_b";
const REV_1: &str = "rev_1";
const REV_2: &str = "rev_2";

/// A requirement whose `description` populates the review-content
/// projection, so editing it moves the digest.
fn requirement(uid: &str, description: &str) -> RequirementNode {
    let mut node = RequirementNode::new(
        uid.to_string(),
        uid.to_uppercase().replace('_', "-"),
        format!("title of {uid}"),
        RequirementLayer::Hlr,
        Vec::new(),
    );
    node.description = Some(description.to_string());
    node
}

/// The digest a review of `node`'s current content binds.
fn digest_of(node: &RequirementNode) -> ReviewContentDigest {
    review_content_digest_v1(&RequirementReviewContentV1::from_node(node))
}

fn review(
    uid: &str,
    requirement_uid: &str,
    digest: &ReviewContentDigest,
    decision: ReviewDecision,
) -> ReviewNode {
    ReviewNode {
        uid: uid.to_string(),
        id: uid.to_string(),
        requirement_uid: requirement_uid.to_string(),
        content_schema: 1,
        reviewed_content_sha256: digest.clone(),
        decision,
        reviewer: format!("{uid}@example.com"),
        reviewed_at: "2026-07-01T10:00:00Z".to_string(),
        rationale: match decision {
            ReviewDecision::Approve => None,
            ReviewDecision::Reject => Some("reviewed and found wanting".to_string()),
        },
        edges: vec![(EdgeKind::Reviews, requirement_uid.to_string())],
    }
}

fn approve(uid: &str, requirement_uid: &str, digest: &ReviewContentDigest) -> ReviewNode {
    review(uid, requirement_uid, digest, ReviewDecision::Approve)
}

fn reject(uid: &str, requirement_uid: &str, digest: &ReviewContentDigest) -> ReviewNode {
    review(uid, requirement_uid, digest, ReviewDecision::Reject)
}

/// Override the reviewer — a supersession chain names one reviewer.
fn by(mut node: ReviewNode, reviewer: &str) -> ReviewNode {
    node.reviewer = reviewer.to_string();
    node
}

fn supersedes(node: &mut ReviewNode, predecessor: &str) {
    node.edges
        .push((EdgeKind::Supersedes, predecessor.to_string()));
}

fn graph_with(req: RequirementNode, reviews: Vec<ReviewNode>) -> CorpusGraph {
    let mut graph = CorpusGraph::new();
    graph
        .insert(Node::Requirement(req))
        .expect("insert requirement");
    for review in reviews {
        graph.insert(Node::Review(review)).expect("insert review");
    }
    graph
}

fn evaluate(graph: &CorpusGraph, uid: &str) -> LifecycleEvaluation {
    evaluate_lifecycle(graph, uid).expect("evaluation succeeds")
}

/// No reviews at all: the requirement is a Candidate (TEST-135).
#[test]
fn no_reviews_yield_candidate() {
    let graph = graph_with(requirement(REQ, "prose v1"), Vec::new());
    let evaluation = evaluate(&graph, REQ);
    assert_eq!(evaluation.state, RequirementLifecycle::Candidate);
    assert_eq!(evaluation.state.as_str(), "candidate");
    assert_eq!(evaluation.requirement_uid, REQ);
    assert_eq!(
        evaluation.current_digest,
        digest_of(&requirement(REQ, "prose v1"))
    );
    assert!(evaluation.effective_review_uids.is_empty());
}

/// One approval binding the current digest: Approved (TEST-135).
#[test]
fn current_approval_yields_approved() {
    let req = requirement(REQ, "prose v1");
    let digest = digest_of(&req);
    let graph = graph_with(req, vec![approve(REV_1, REQ, &digest)]);
    let evaluation = evaluate(&graph, REQ);
    assert_eq!(evaluation.state, RequirementLifecycle::Approved);
    assert_eq!(evaluation.state.as_str(), "approved");
    assert_eq!(evaluation.current_digest, digest);
    assert_eq!(evaluation.effective_review_uids, vec![REV_1.to_string()]);
}

/// One rejection binding the current digest: Rejected (TEST-135).
#[test]
fn current_rejection_yields_rejected() {
    let req = requirement(REQ, "prose v1");
    let digest = digest_of(&req);
    let graph = graph_with(req, vec![reject(REV_1, REQ, &digest)]);
    let evaluation = evaluate(&graph, REQ);
    assert_eq!(evaluation.state, RequirementLifecycle::Rejected);
    assert_eq!(evaluation.state.as_str(), "rejected");
    assert_eq!(evaluation.effective_review_uids, vec![REV_1.to_string()]);
}

/// Approval and rejection both binding the current digest:
/// rejection is fail-closed and takes precedence (TEST-135).
#[test]
fn rejection_takes_precedence_over_approval() {
    let req = requirement(REQ, "prose v1");
    let digest = digest_of(&req);
    let graph = graph_with(
        req,
        vec![approve(REV_1, REQ, &digest), reject(REV_2, REQ, &digest)],
    );
    let evaluation = evaluate(&graph, REQ);
    assert_eq!(evaluation.state, RequirementLifecycle::Rejected);
    assert_eq!(
        evaluation.effective_review_uids,
        vec![REV_1.to_string(), REV_2.to_string()],
        "both heads are effective; precedence is in the truth table"
    );
}

/// An approval superseded by a rejection over the same digest
/// rejects; a rejection superseded by an approval approves. The
/// superseded review contributes nothing (TEST-135).
#[test]
fn superseded_reviews_contribute_nothing() {
    let req = requirement(REQ, "prose v1");
    let digest = digest_of(&req);

    let mut correction = by(reject(REV_2, REQ, &digest), "rev_1@example.com");
    supersedes(&mut correction, REV_1);
    let graph = graph_with(req.clone(), vec![approve(REV_1, REQ, &digest), correction]);
    graph.validate().expect("a well-formed chain validates");
    let evaluation = evaluate(&graph, REQ);
    assert_eq!(evaluation.state, RequirementLifecycle::Rejected);
    assert_eq!(
        evaluation.effective_review_uids,
        vec![REV_2.to_string()],
        "only the correcting head is effective"
    );

    let mut correction = by(approve(REV_2, REQ, &digest), "rev_1@example.com");
    supersedes(&mut correction, REV_1);
    let graph = graph_with(req, vec![reject(REV_1, REQ, &digest), correction]);
    graph.validate().expect("a well-formed chain validates");
    let evaluation = evaluate(&graph, REQ);
    assert_eq!(evaluation.state, RequirementLifecycle::Approved);
    assert_eq!(evaluation.effective_review_uids, vec![REV_2.to_string()]);
}

/// A content change after approval turns the state Stale
/// mechanically: the untouched approval re-evaluated against new
/// content degrades; no caller toggles a flag (TEST-135).
#[test]
fn content_change_after_approval_yields_stale() {
    let approved_digest = digest_of(&requirement(REQ, "prose v1"));
    let approval = approve(REV_1, REQ, &approved_digest);
    let approved_graph = graph_with(requirement(REQ, "prose v1"), vec![approval.clone()]);
    assert_eq!(
        evaluate(&approved_graph, REQ).state,
        RequirementLifecycle::Approved
    );

    // One projection field changes; the review record is untouched.
    let revised_graph = graph_with(requirement(REQ, "prose v2"), vec![approval]);
    let evaluation = evaluate(&revised_graph, REQ);
    assert_eq!(evaluation.state, RequirementLifecycle::Stale);
    assert_eq!(evaluation.state.as_str(), "stale");
    assert_ne!(
        evaluation.current_digest, approved_digest,
        "the digest moved with the content"
    );
    assert_eq!(
        evaluation.effective_review_uids,
        vec![REV_1.to_string()],
        "the approval is still the effective head, bound to the older digest"
    );
}

/// Revised content whose only older decision was a rejection is a
/// Candidate: an older-digest rejection never stigmatizes new
/// content and can never produce Stale (TEST-135).
#[test]
fn content_change_after_rejection_yields_candidate() {
    let old_digest = digest_of(&requirement(REQ, "prose v1"));
    let graph = graph_with(
        requirement(REQ, "prose v2"),
        vec![reject(REV_1, REQ, &old_digest)],
    );
    let evaluation = evaluate(&graph, REQ);
    assert_eq!(
        evaluation.state,
        RequirementLifecycle::Candidate,
        "rejection of older content must yield Candidate, not Stale"
    );
}

/// Returning content to the previously approved exact digest
/// yields Approved again: the digest binds exact content
/// (TEST-135).
#[test]
fn reverting_to_approved_digest_yields_approved_again() {
    let approved_digest = digest_of(&requirement(REQ, "prose v1"));
    let approved_graph = graph_with(
        requirement(REQ, "prose v1"),
        vec![approve(REV_1, REQ, &approved_digest)],
    );
    assert_eq!(
        evaluate(&approved_graph, REQ).state,
        RequirementLifecycle::Approved
    );

    let revised_graph = graph_with(
        requirement(REQ, "prose v2"),
        vec![approve(REV_1, REQ, &approved_digest)],
    );
    assert_eq!(
        evaluate(&revised_graph, REQ).state,
        RequirementLifecycle::Stale
    );

    let reverted_graph = graph_with(
        requirement(REQ, "prose v1"),
        vec![approve(REV_1, REQ, &approved_digest)],
    );
    let evaluation = evaluate(&reverted_graph, REQ);
    assert_eq!(
        evaluation.state,
        RequirementLifecycle::Approved,
        "reverting to the approved digest restores Approved"
    );
    assert_eq!(evaluation.current_digest, approved_digest);
}

/// Independent reviewers compose deterministically: two current
/// approvals approve; a current approval plus an older-digest
/// rejection still approves (TEST-135).
#[test]
fn multiple_reviewers_compose_deterministically() {
    let digest = digest_of(&requirement(REQ, "prose v1"));
    let older_digest = digest_of(&requirement(REQ, "prose v0"));

    let graph = graph_with(
        requirement(REQ, "prose v1"),
        vec![approve(REV_1, REQ, &digest), approve(REV_2, REQ, &digest)],
    );
    let evaluation = evaluate(&graph, REQ);
    assert_eq!(evaluation.state, RequirementLifecycle::Approved);
    assert_eq!(
        evaluation.effective_review_uids,
        vec![REV_1.to_string(), REV_2.to_string()]
    );

    let graph = graph_with(
        requirement(REQ, "prose v1"),
        vec![
            approve(REV_1, REQ, &digest),
            reject(REV_2, REQ, &older_digest),
        ],
    );
    let evaluation = evaluate(&graph, REQ);
    assert_eq!(
        evaluation.state,
        RequirementLifecycle::Approved,
        "an older-digest rejection cannot veto a current-digest approval"
    );
}

/// Permuting `reviewed_at` values and insertion order cannot change
/// any evaluation (TEST-136).
#[test]
fn timestamps_and_insertion_order_never_affect_evaluation() {
    let req = requirement(REQ, "prose v1");
    let digest = digest_of(&req);
    let first = approve(REV_1, REQ, &digest);
    let mut second = reject(REV_2, REQ, &digest);
    second.reviewed_at = "2026-07-03T18:30:00+02:00".to_string();
    let graph_a = graph_with(req.clone(), vec![first.clone(), second.clone()]);

    // Same graph, swapped timestamps and reversed insertion order.
    let mut first_later = first;
    first_later.reviewed_at = second.reviewed_at.clone();
    let mut second_earlier = second;
    second_earlier.reviewed_at = "2025-01-01T00:00:00Z".to_string();
    let graph_b = graph_with(req, vec![second_earlier, first_later]);

    let eval_a = evaluate(&graph_a, REQ);
    assert_eq!(
        eval_a,
        evaluate(&graph_b, REQ),
        "timestamps and insertion order are non-semantic"
    );
    assert_eq!(
        eval_a.state,
        RequirementLifecycle::Rejected,
        "the current-digest rejection still takes precedence"
    );
    assert_eq!(
        eval_a.effective_review_uids,
        vec![REV_1.to_string(), REV_2.to_string()]
    );
    assert_eq!(
        evaluate_all_lifecycles(&graph_a).expect("bulk evaluation succeeds"),
        evaluate_all_lifecycles(&graph_b).expect("bulk evaluation succeeds")
    );
}

/// A review targeting a missing requirement is invalid graph data:
/// both entry points fail closed with a typed error carrying the
/// requirement uid and the review uid (TEST-136).
#[test]
fn review_of_missing_requirement_fails_closed() {
    let req = requirement(REQ, "prose v1");
    let stray = approve(REV_1, "req_gone", &digest_of(&req));
    let graph = graph_with(req, vec![stray]);

    let err = evaluate_lifecycle(&graph, "req_gone").expect_err("must fail closed");
    assert!(
        matches!(
            err,
            LifecycleError::ApprovalTargetsMissingRequirement {
                ref requirement_uid,
                ref review_uid,
            } if requirement_uid == "req_gone" && review_uid == REV_1
        ),
        "error names the requirement uid and the review uid: {err}"
    );
    let err = evaluate_all_lifecycles(&graph).expect_err("bulk evaluation must fail closed too");
    assert!(
        matches!(
            err,
            LifecycleError::ApprovalTargetsMissingRequirement { .. }
        ),
        "bulk evaluation reports the same invalid graph data: {err}"
    );
    let err = evaluate_lifecycle(&graph, "req_absent").expect_err("absent uid must fail");
    assert!(
        matches!(
            err,
            LifecycleError::RequirementMissing { ref requirement_uid } if requirement_uid == "req_absent"
        ),
        "an absent uid with no reviews is RequirementMissing: {err}"
    );
}

/// Bulk evaluation reports every requirement keyed by uid in
/// deterministic order, matching per-requirement evaluation
/// (TEST-136).
#[test]
fn evaluate_all_reports_requirements_in_uid_order() {
    let digest_b = digest_of(&requirement(REQ_B, "second requirement"));
    let older_digest = digest_of(&requirement(REQ, "first requirement v0"));
    let mut graph = CorpusGraph::new();
    // Insert requirements out of uid order on purpose.
    graph
        .insert(Node::Requirement(requirement(REQ_B, "second requirement")))
        .expect("insert requirement");
    graph
        .insert(Node::Requirement(requirement(REQ, "first requirement")))
        .expect("insert requirement");
    graph
        .insert(Node::Review(approve(REV_2, REQ_B, &digest_b)))
        .expect("insert review");
    graph
        .insert(Node::Review(approve(REV_1, REQ, &older_digest)))
        .expect("insert review");

    let evaluations = evaluate_all_lifecycles(&graph).expect("bulk evaluation succeeds");
    let keys: Vec<&str> = evaluations.keys().map(String::as_str).collect();
    assert_eq!(keys, vec![REQ, REQ_B], "reporting iterates in uid order");
    assert_eq!(evaluations[REQ].state, RequirementLifecycle::Stale);
    assert_eq!(evaluations[REQ_B].state, RequirementLifecycle::Approved);
    assert_eq!(evaluations[REQ], evaluate(&graph, REQ));
    assert_eq!(evaluations[REQ_B], evaluate(&graph, REQ_B));
}
