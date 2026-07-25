//! Adversarial malformed-graph tests: the public lifecycle entry
//! points validate once per call and fail closed with the exact
//! validation error preserved as the typed source — no malformed
//! review graph can produce a lifecycle state (TEST-136).

use super::fixtures::{
    REQ, REQ_B, REV_1, REV_2, approve, by, digest_of, graph_with, requirement, supersedes,
};
use super::{LifecycleError, evaluate_all_lifecycles, evaluate_lifecycle};
use crate::corpus::{CorpusError, CorpusGraph, EdgeKind, Node, ReviewError};

/// Mismatched requirement field/edge: a review whose
/// `requirement_uid` field names one requirement while its
/// `Reviews` edge targets another must never produce a lifecycle
/// state. Both entry points fail closed in
/// validation, and the source preserves the review-invariant error
/// (TEST-136).
#[test]
fn mismatched_requirement_field_and_edge_fails_closed() {
    let digest = digest_of(&requirement(REQ, "prose v1"));
    let mut stray = approve(REV_1, REQ, &digest);
    // The field names REQ; retarget the Reviews edge at REQ_B.
    stray.edges = vec![(EdgeKind::Reviews, REQ_B.to_string())];
    let mut graph = CorpusGraph::new();
    graph
        .insert(Node::Requirement(requirement(REQ, "prose v1")))
        .expect("insert requirement");
    graph
        .insert(Node::Requirement(requirement(REQ_B, "other prose")))
        .expect("insert requirement");
    graph.insert(Node::Review(stray)).expect("insert review");

    let err = evaluate_lifecycle(&graph, REQ).expect_err("no Approved state may be produced");
    assert!(
        matches!(
            err,
            LifecycleError::InvalidGraph(ref inner)
                if matches!(
                    inner.as_ref(),
                    CorpusError::Review(review)
                        if matches!(
                            review.as_ref(),
                            ReviewError::ReviewTargetEdgeMismatch {
                                field_target_uid,
                                edge_target_uid,
                                ..
                            } if field_target_uid == REQ && edge_target_uid == REQ_B
                        )
                )
        ),
        "the source preserves the review-invariant error: {err}"
    );
    let err = evaluate_all_lifecycles(&graph).expect_err("no Candidate state may be produced");
    assert!(
        matches!(
            err,
            LifecycleError::InvalidGraph(ref inner) if matches!(
                inner.as_ref(),
                CorpusError::Review(review)
                    if matches!(review.as_ref(), ReviewError::ReviewTargetEdgeMismatch { .. })
            )
        ),
        "bulk evaluation fails the same way: {err}"
    );
}

/// Unsupported content schema: `content_schema = 99` on a
/// programmatically built review must never produce a lifecycle
/// state. Both entry points fail closed in validation, and the source
/// preserves the schema error (TEST-136).
#[test]
fn unsupported_content_schema_fails_closed() {
    let req = requirement(REQ, "prose v1");
    let digest = digest_of(&req);
    let mut foreign = approve(REV_1, REQ, &digest);
    foreign.content_schema = 99;
    let graph = graph_with(req, vec![foreign]);

    let err = evaluate_lifecycle(&graph, REQ).expect_err("no Approved state may be produced");
    assert!(
        matches!(
            err,
            LifecycleError::InvalidGraph(ref inner) if matches!(
                inner.as_ref(),
                CorpusError::Review(review)
                    if matches!(review.as_ref(), ReviewError::ReviewContentSchema { found: 99, .. })
            )
        ),
        "the source preserves the schema error: {err}"
    );
    let err = evaluate_all_lifecycles(&graph).expect_err("no Candidate state may be produced");
    assert!(
        matches!(
            err,
            LifecycleError::InvalidGraph(ref inner) if matches!(
                inner.as_ref(),
                CorpusError::Review(review)
                    if matches!(review.as_ref(), ReviewError::ReviewContentSchema { found: 99, .. })
            )
        ),
        "bulk evaluation fails the same way: {err}"
    );
}

/// Supersession cycle a→b→a: graph validation already rejected the
/// chain, yet `evaluate_lifecycle` still returned `Ok(Candidate)`.
/// Both entry points now fail closed in validation, and the source
/// preserves the cycle error (TEST-136).
#[test]
fn supersession_cycle_fails_closed() {
    let req = requirement(REQ, "prose v1");
    let digest = digest_of(&req);
    let mut first = approve(REV_1, REQ, &digest);
    supersedes(&mut first, REV_2);
    let mut second = by(approve(REV_2, REQ, &digest), "rev_1@example.com");
    supersedes(&mut second, REV_1);
    let graph = graph_with(req, vec![first, second]);
    graph
        .validate()
        .expect_err("the cycle already fails graph validation");

    let err =
        evaluate_lifecycle(&graph, REQ).expect_err("the former Ok(Candidate) is now an error");
    assert!(
        matches!(
            err,
            LifecycleError::InvalidGraph(ref inner) if matches!(
                inner.as_ref(),
                CorpusError::Review(review)
                    if matches!(review.as_ref(), ReviewError::ReviewSupersessionCycle { .. })
            )
        ),
        "the source preserves the cycle error: {err}"
    );
    let err = evaluate_all_lifecycles(&graph).expect_err("no Candidate state may be produced");
    assert!(
        matches!(
            err,
            LifecycleError::InvalidGraph(ref inner) if matches!(
                inner.as_ref(),
                CorpusError::Review(review)
                    if matches!(review.as_ref(), ReviewError::ReviewSupersessionCycle { .. })
            )
        ),
        "bulk evaluation fails the same way: {err}"
    );
}
