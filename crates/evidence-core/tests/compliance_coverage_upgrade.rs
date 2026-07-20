//! Integration tests pinning the A-7 Obj-5 / Obj-6 verdict truth
//! table (LLR-108 / HLR-088): a coverage percentage is an
//! engineering metric, never a compliance verdict. `Met` requires
//! the metric to meet the engineering gate AND a recorded
//! analysis/disposition of uncovered structure — and only for
//! statement coverage; the LLVM branch approximation of decision
//! coverage caps at `ManualReviewRequired` in every data-present
//! case. Carved out of `compliance/status.rs` to keep that file
//! under the workspace 500-line limit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use evidence_core::{
    AssuranceLevel, CrateEvidence, ObjectiveStatusKind, generate_compliance_report,
};

fn evidence_with_coverage(statement: f64, branch: f64) -> CrateEvidence {
    CrateEvidence {
        has_trace_data: true,
        trace_validation_passed: true,
        has_test_results: true,
        tests_passed: Some(true),
        has_coverage_data: true,
        has_per_test_outcomes: true,
        coverage_statement_percent: Some(statement),
        coverage_branch_percent: Some(branch),
        coverage_disposition: None,
    }
}

fn status_of(report: &evidence_core::ComplianceReport, objective_id: &str) -> ObjectiveStatusKind {
    report
        .objectives
        .iter()
        .find(|o| o.objective_id == objective_id)
        .unwrap_or_else(|| panic!("{objective_id} present in report"))
        .status
}

/// **Complete analyzed evidence.** A-7 Obj-5 (statement coverage)
/// reaches Met when the aggregate meets the engineering gate AND a
/// documented analysis/disposition of uncovered structure is
/// recorded (96% ≥ DAL-B's 95% gate + disposition).
#[test]
fn a7_8_met_when_statement_coverage_meets_gate_with_disposition() {
    let mut evidence = evidence_with_coverage(96.0, 88.0);
    evidence.coverage_disposition = Some("coverage-analysis record COV-2026-011".to_string());
    let report = generate_compliance_report("cov-crate", AssuranceLevel::DalB, &evidence);
    assert_eq!(status_of(&report, "A7-8"), ObjectiveStatusKind::Met);
}

/// The same metric without a disposition is engineering data, not a
/// verdict: the objective closes only with documented
/// analysis/disposition + tool qualification evidence, so the tool
/// reports `ManualReviewRequired` — never a silent Met.
#[test]
fn a7_8_manual_review_when_statement_coverage_meets_gate_without_disposition() {
    let evidence = evidence_with_coverage(96.0, 88.0);
    let report = generate_compliance_report("cov-crate", AssuranceLevel::DalB, &evidence);
    let a7_8 = report
        .objectives
        .iter()
        .find(|o| o.objective_id == "A7-8")
        .expect("A7-8 present at DAL-B");
    assert_eq!(a7_8.status, ObjectiveStatusKind::ManualReviewRequired);
    let note = a7_8.note.as_ref().expect("review note present");
    assert!(
        note.contains("engineering metric"),
        "note must frame the percentage as an engineering metric; got: {note}"
    );
}

/// **Incomplete coverage, rationale pending.** Below the gate the
/// verdict is Partial: uncovered items exist and the note requires
/// their analysis/disposition before the objective can close.
#[test]
fn a7_8_partial_when_statement_coverage_below_gate() {
    let evidence = evidence_with_coverage(92.0, 88.0);
    let report = generate_compliance_report("cov-crate", AssuranceLevel::DalB, &evidence);
    let a7_8 = report
        .objectives
        .iter()
        .find(|o| o.objective_id == "A7-8")
        .expect("A7-8 present at DAL-B");
    assert_eq!(a7_8.status, ObjectiveStatusKind::Partial);
    let note = a7_8.note.as_ref().expect("partial note present");
    assert!(
        note.contains("analysis/disposition"),
        "note must require analysis/disposition of uncovered structure; got: {note}"
    );
}

/// A-7 Obj-5 stays NotMet when no coverage report was produced
/// — absent coverage is not `ManualReviewRequired` at DAL ≥ C
/// since the tool has the means to capture it.
#[test]
fn a7_8_not_met_when_no_coverage_report() {
    let evidence = CrateEvidence {
        has_trace_data: true,
        trace_validation_passed: true,
        has_test_results: true,
        tests_passed: Some(true),
        has_coverage_data: false,
        has_per_test_outcomes: true,
        coverage_statement_percent: None,
        coverage_branch_percent: None,
        coverage_disposition: None,
    };
    let report = generate_compliance_report("no-cov-crate", AssuranceLevel::DalC, &evidence);
    assert_eq!(status_of(&report, "A7-8"), ObjectiveStatusKind::NotMet);
}

/// **Approximate branch evidence.** LLVM branch coverage
/// approximates decision coverage; even at/above the gate WITH a
/// recorded disposition the verdict caps at `ManualReviewRequired`
/// — approximate evidence cannot close the semantic objective.
#[test]
fn a7_9_manual_review_when_branch_coverage_meets_gate_with_disposition() {
    let mut evidence = evidence_with_coverage(96.0, 88.0);
    evidence.coverage_disposition = Some("coverage-analysis record COV-2026-011".to_string());
    let report = generate_compliance_report("cov-crate", AssuranceLevel::DalB, &evidence);
    let a7_9 = report
        .objectives
        .iter()
        .find(|o| o.objective_id == "A7-9")
        .expect("A7-9 present at DAL-B");
    assert_eq!(a7_9.status, ObjectiveStatusKind::ManualReviewRequired);
    let note = a7_9.note.as_ref().expect("review note present");
    assert!(
        note.contains("approximat"),
        "note must name the approximation gap; got: {note}"
    );
}

/// The approximation cap holds without a disposition too: the
/// percentage is an engineering metric, so A7-9 can never report
/// Met from LLVM branch data alone.
#[test]
fn a7_9_manual_review_when_branch_coverage_meets_gate_without_disposition() {
    let evidence = evidence_with_coverage(96.0, 88.0);
    let report = generate_compliance_report("cov-crate", AssuranceLevel::DalB, &evidence);
    assert_eq!(
        status_of(&report, "A7-9"),
        ObjectiveStatusKind::ManualReviewRequired
    );
}

/// **Incomplete branch coverage.** Below the gate the verdict is
/// Partial — uncovered decisions exist and require documented
/// analysis/disposition before the objective can close. (The
/// no-gate arm — a DAL whose table row carries no threshold — is
/// not reachable through the objectives table; it is pinned by the
/// `coverage_verdict` unit tests instead.)
#[test]
fn a7_9_partial_when_branch_coverage_below_gate() {
    let evidence = evidence_with_coverage(96.0, 84.0);
    let report = generate_compliance_report("cov-crate", AssuranceLevel::DalB, &evidence);
    let a7_9 = report
        .objectives
        .iter()
        .find(|o| o.objective_id == "A7-9")
        .expect("A7-9 present at DAL-B");
    assert_eq!(a7_9.status, ObjectiveStatusKind::Partial);
    let note = a7_9.note.as_ref().expect("partial note present");
    assert!(
        note.contains("analysis/disposition"),
        "note must require analysis/disposition of uncovered structure; got: {note}"
    );
}
