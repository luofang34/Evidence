//! Acceptance, distinct-diagnostic, determinism, and fail-closed
//! tests for the approval boundary (TEST-137).

use super::{
    ApprovalBoundaryError, ApprovalBoundaryViolation, LifecycleEnforcement, ReferringArtifact,
    validate_approval_boundary,
};
use crate::corpus::graph::{RequirementMetadata, TraceMetadata};
use crate::corpus::{
    CorpusGraph, EdgeKind, LifecycleError, Node, RequirementLayer, RequirementLifecycle,
    RequirementNode, RequirementReviewContentV1, ReviewContentDigest, ReviewDecision, ReviewNode,
    TestNode, review_content_digest_v1,
};

const REQ: &str = "req_a";
const REQ_B: &str = "req_b";
const REQ_C: &str = "req_c";
const REV_1: &str = "rev_1";
const TEST_A: &str = "test_a";
const TEST_B: &str = "test_b";

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

/// Insert a requirement carrying legacy-style trace metadata claims
/// (`modules` / `emits`), as the legacy adapter populates them.
fn insert_with_claims(
    graph: &mut CorpusGraph,
    node: RequirementNode,
    modules: &[&str],
    emits: &[&str],
) {
    graph
        .insert_with_trace_metadata(
            Node::Requirement(node),
            TraceMetadata::Requirement(RequirementMetadata {
                modules: modules.iter().map(|m| (*m).to_string()).collect(),
                emits: emits.iter().map(|c| (*c).to_string()).collect(),
                ..RequirementMetadata::default()
            }),
        )
        .expect("insert requirement with claims");
}

fn test_verifies(uid: &str, target: &str) -> TestNode {
    TestNode {
        uid: uid.to_string(),
        id: uid.to_uppercase().replace('_', "-"),
        title: format!("title of {uid}"),
        selectors: Vec::new(),
        edges: vec![(EdgeKind::Verifies, target.to_string())],
    }
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

fn validate(graph: &CorpusGraph) -> Result<(), ApprovalBoundaryError> {
    validate_approval_boundary(graph, LifecycleEnforcement::Required)
}

fn expect_violations(result: Result<(), ApprovalBoundaryError>) -> Vec<ApprovalBoundaryViolation> {
    match result {
        Err(ApprovalBoundaryError::Violations { violations }) => violations,
        other => panic!("expected aggregated violations, got: {other:?}"),
    }
}

/// Approved requirements accept test and implementation references:
/// a verifying test plus module and emitted-diagnostic claims all
/// pass silently (TEST-137).
#[test]
fn approved_requirement_accepts_test_and_implementation_claims() {
    let req = requirement(REQ, "prose v1");
    let digest = digest_of(&req);
    let mut graph = CorpusGraph::new();
    insert_with_claims(
        &mut graph,
        req,
        &["evidence_core::corpus::x"],
        &["SOME_CODE"],
    );
    graph
        .insert(Node::Review(approve(REV_1, REQ, &digest)))
        .expect("insert review");
    graph
        .insert(Node::Test(test_verifies(TEST_A, REQ)))
        .expect("insert test");

    validate(&graph).expect("an approved target passes silently");
}

/// A test cannot verify an unapproved requirement: a candidate
/// target yields one violation naming the requirement uid, human id,
/// `candidate` state, and the test's uid and id (TEST-137).
#[test]
fn candidate_requirement_rejects_verifying_test() {
    let mut graph = CorpusGraph::new();
    graph
        .insert(Node::Requirement(requirement(REQ, "prose v1")))
        .expect("insert requirement");
    graph
        .insert(Node::Test(test_verifies(TEST_A, REQ)))
        .expect("insert test");

    let violations = expect_violations(validate(&graph));
    assert_eq!(violations.len(), 1);
    let violation = &violations[0];
    assert_eq!(violation.requirement_uid, REQ);
    assert_eq!(violation.requirement_id, "REQ-A");
    assert_eq!(violation.state, RequirementLifecycle::Candidate);
    assert_eq!(
        violation.referring,
        ReferringArtifact::Test {
            test_uid: TEST_A.to_string(),
            test_id: "TEST-A".to_string(),
        }
    );
    let rendered = violation.to_string();
    assert!(
        rendered.contains("candidate"),
        "Display names the state: {rendered}"
    );
    assert!(rendered.contains(REQ), "Display names the uid: {rendered}");
    assert!(
        rendered.contains("REQ-A"),
        "Display names the human id: {rendered}"
    );
    assert!(
        rendered.contains(TEST_A),
        "Display names the test: {rendered}"
    );
}

/// A module claim cannot attach to an unapproved requirement: a
/// rejected target yields one `ImplementationModules` violation
/// naming `rejected` (TEST-137).
#[test]
fn rejected_requirement_rejects_modules_claim() {
    let req = requirement(REQ, "prose v1");
    let digest = digest_of(&req);
    let mut graph = CorpusGraph::new();
    insert_with_claims(&mut graph, req, &["evidence_core::corpus::x"], &[]);
    graph
        .insert(Node::Review(reject(REV_1, REQ, &digest)))
        .expect("insert review");

    let violations = expect_violations(validate(&graph));
    assert_eq!(violations.len(), 1);
    let violation = &violations[0];
    assert_eq!(violation.state, RequirementLifecycle::Rejected);
    assert_eq!(
        violation.referring,
        ReferringArtifact::ImplementationModules {
            modules: vec!["evidence_core::corpus::x".to_string()],
        }
    );
    let rendered = violation.to_string();
    assert!(
        rendered.contains("rejected"),
        "Display names the state: {rendered}"
    );
}

/// An emitted-code claim cannot attach to an unapproved requirement:
/// a stale target yields one `EmittedDiagnostics` violation naming
/// `stale` (TEST-137).
#[test]
fn stale_requirement_rejects_emits_claim() {
    let approved_digest = digest_of(&requirement(REQ, "prose v1"));
    let mut graph = CorpusGraph::new();
    insert_with_claims(
        &mut graph,
        requirement(REQ, "prose v2"),
        &[],
        &["SOME_CODE"],
    );
    graph
        .insert(Node::Review(approve(REV_1, REQ, &approved_digest)))
        .expect("insert review");

    let violations = expect_violations(validate(&graph));
    assert_eq!(violations.len(), 1);
    let violation = &violations[0];
    assert_eq!(violation.state, RequirementLifecycle::Stale);
    assert_eq!(
        violation.referring,
        ReferringArtifact::EmittedDiagnostics {
            codes: vec!["SOME_CODE".to_string()],
        }
    );
    let rendered = violation.to_string();
    assert!(
        rendered.contains("stale"),
        "Display names the state: {rendered}"
    );
}

/// Candidate, rejected, and stale targets produce DISTINCT typed
/// diagnostics: the state payloads differ pairwise and each Display
/// line names its own state (TEST-137).
#[test]
fn unapproved_states_produce_distinct_diagnostics() {
    let rejected_digest = digest_of(&requirement(REQ_B, "rejected prose"));
    let stale_digest = digest_of(&requirement(REQ_C, "stale prose v1"));
    let mut graph = CorpusGraph::new();
    graph
        .insert(Node::Requirement(requirement(REQ, "candidate prose")))
        .expect("insert candidate");
    graph
        .insert(Node::Requirement(requirement(REQ_B, "rejected prose")))
        .expect("insert rejected");
    graph
        .insert(Node::Requirement(requirement(REQ_C, "stale prose v2")))
        .expect("insert stale");
    graph
        .insert(Node::Review(reject(REV_1, REQ_B, &rejected_digest)))
        .expect("insert rejection");
    graph
        .insert(Node::Review(approve("rev_2", REQ_C, &stale_digest)))
        .expect("insert older approval");
    for (test_uid, target) in [(TEST_A, REQ), (TEST_B, REQ_B), ("test_c", REQ_C)] {
        graph
            .insert(Node::Test(test_verifies(test_uid, target)))
            .expect("insert test");
    }

    let violations = expect_violations(validate(&graph));
    let states: Vec<RequirementLifecycle> = violations.iter().map(|v| v.state).collect();
    assert_eq!(
        states,
        vec![
            RequirementLifecycle::Candidate,
            RequirementLifecycle::Rejected,
            RequirementLifecycle::Stale,
        ],
        "violations iterate in uid order and carry pairwise-distinct states"
    );
    let rendered = validate(&graph).expect_err("still failing").to_string();
    for state in ["candidate", "rejected", "stale"] {
        assert!(
            rendered.contains(state),
            "error Display names {state}: {rendered}"
        );
    }
}

/// Candidate requirement decomposition remains usable before
/// approval: two candidates linked by `DerivesFrom`, with no tests
/// or implementation claims, are structurally valid and ungated
/// (TEST-137).
#[test]
fn candidate_decomposition_remains_usable() {
    let mut child = requirement(REQ_B, "child prose");
    child.edges.push((EdgeKind::DerivesFrom, REQ.to_string()));
    let mut graph = CorpusGraph::new();
    graph
        .insert(Node::Requirement(requirement(REQ, "parent prose")))
        .expect("insert parent");
    graph
        .insert(Node::Requirement(child))
        .expect("insert child");

    graph
        .validate()
        .expect("candidate decomposition is structurally valid");
    validate(&graph).expect("DerivesFrom edges are never gated");
}

/// Explicit enforcement over zero reviews fails closed: an
/// explicitly requested approval claim can never succeed over an
/// empty review set, because missing reviews are `Candidate`, never
/// implicitly approved (TEST-137).
#[test]
fn zero_review_graph_fails_closed() {
    let mut graph = CorpusGraph::new();
    insert_with_claims(
        &mut graph,
        requirement(REQ, "prose v1"),
        &["evidence_core::corpus::x"],
        &[],
    );
    graph
        .insert(Node::Test(test_verifies(TEST_A, REQ)))
        .expect("insert test");

    let err = validate(&graph).expect_err("zero reviews must fail closed");
    let ApprovalBoundaryError::Violations { violations } = &err else {
        panic!("expected aggregated violations, got: {err:?}");
    };
    assert_eq!(violations.len(), 2, "every would-be-gated claim violates");
    assert!(
        violations
            .iter()
            .all(|v| v.state == RequirementLifecycle::Candidate),
        "every target is Candidate over zero reviews"
    );
    assert!(
        err.to_string().contains("candidate"),
        "Display names the state"
    );
}

/// The policy choice is explicit and cannot silently default to a
/// weaker assurance level: `LifecycleEnforcement` has exactly one
/// variant and no `Default`, so naming `Required` in code is the
/// only way to request enforcement. This exhaustive match pins the
/// variant set — adding a second (weaker) policy fails compilation
/// here until the test names it. The "not requested" case is the
/// absence of `validate_approval_boundary` from the call graph
/// (TEST-137).
#[test]
fn enforcement_is_explicit_single_variant() {
    let enforcement = LifecycleEnforcement::Required;
    match enforcement {
        LifecycleEnforcement::Required => {}
    }
    assert_eq!(enforcement, LifecycleEnforcement::Required);
}

/// A lifecycle-evaluator failure (a review of a missing requirement)
/// fails closed as the `Lifecycle` variant, wrapping the typed
/// evaluator error (TEST-137).
#[test]
fn lifecycle_evaluation_error_fails_closed() {
    let req = requirement(REQ, "prose v1");
    let stray = approve(REV_1, "req_gone", &digest_of(&req));
    let mut graph = CorpusGraph::new();
    graph
        .insert(Node::Requirement(req))
        .expect("insert requirement");
    graph
        .insert(Node::Review(stray))
        .expect("insert stray review");

    let err = validate(&graph).expect_err("invalid review data must fail closed");
    assert!(
        matches!(
            err,
            ApprovalBoundaryError::Lifecycle(
                LifecycleError::ApprovalTargetsMissingRequirement { .. }
            )
        ),
        "the evaluator error is wrapped, not flattened: {err}"
    );
}

/// Violations iterate in deterministic order — requirement uid, then
/// referring artifact (`Test` before `ImplementationModules`) —
/// regardless of node insertion order (TEST-137).
#[test]
fn violations_are_deterministically_ordered() {
    let build = |reverse: bool| {
        let mut graph = CorpusGraph::new();
        let insert = |graph: &mut CorpusGraph, uid: &str, test_uid: &str| {
            insert_with_claims(
                graph,
                requirement(uid, "prose"),
                &["evidence_core::corpus::x"],
                &[],
            );
            graph
                .insert(Node::Test(test_verifies(test_uid, uid)))
                .expect("insert test");
        };
        if reverse {
            insert(&mut graph, REQ_B, TEST_B);
            insert(&mut graph, REQ, TEST_A);
        } else {
            insert(&mut graph, REQ, TEST_A);
            insert(&mut graph, REQ_B, TEST_B);
        }
        expect_violations(validate(&graph))
    };
    let forward = build(false);
    let reversed = build(true);
    assert_eq!(forward, reversed, "insertion order is non-semantic");
    let order: Vec<(&str, &ReferringArtifact)> = forward
        .iter()
        .map(|v| (v.requirement_uid.as_str(), &v.referring))
        .collect();
    assert!(
        matches!(
            order.as_slice(),
            [
                (REQ, ReferringArtifact::Test { .. }),
                (REQ, ReferringArtifact::ImplementationModules { .. }),
                (REQ_B, ReferringArtifact::Test { .. }),
                (REQ_B, ReferringArtifact::ImplementationModules { .. }),
            ]
        ),
        "sorted by requirement uid, then referring artifact: {order:?}"
    );
}

/// Multiple violations aggregate in one error rather than failing on
/// the first: a candidate requirement with a verifying test plus
/// module and emitted-diagnostic claims yields all three distinct
/// violation kinds in one error (TEST-137).
#[test]
fn multiple_violations_aggregate_in_one_error() {
    let mut graph = CorpusGraph::new();
    insert_with_claims(
        &mut graph,
        requirement(REQ, "prose v1"),
        &["evidence_core::corpus::x"],
        &["SOME_CODE"],
    );
    graph
        .insert(Node::Test(test_verifies(TEST_A, REQ)))
        .expect("insert test");

    let err = validate(&graph).expect_err("claims must fail");
    let ApprovalBoundaryError::Violations { violations } = &err else {
        panic!("expected aggregated violations, got: {err:?}");
    };
    assert_eq!(violations.len(), 3, "aggregation, not first-fail");
    assert!(
        matches!(
            violations.as_slice(),
            [
                ApprovalBoundaryViolation {
                    referring: ReferringArtifact::Test { .. },
                    ..
                },
                ApprovalBoundaryViolation {
                    referring: ReferringArtifact::ImplementationModules { .. },
                    ..
                },
                ApprovalBoundaryViolation {
                    referring: ReferringArtifact::EmittedDiagnostics { .. },
                    ..
                },
            ]
        ),
        "all three distinct violation kinds in sort order: {violations:?}"
    );
    let rendered = err.to_string();
    assert_eq!(
        rendered.lines().skip(1).count(),
        3,
        "one Display line per violation"
    );
    assert!(rendered.contains(TEST_A), "names the test: {rendered}");
    assert!(
        rendered.contains("evidence_core::corpus::x"),
        "names the module: {rendered}"
    );
    assert!(rendered.contains("SOME_CODE"), "names the code: {rendered}");
}
