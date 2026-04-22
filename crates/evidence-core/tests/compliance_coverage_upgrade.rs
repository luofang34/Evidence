//! Integration tests pinning the A-7 Obj-5 / Obj-6 upgrade on a
//! coverage report present in the bundle. Carved out of
//! `compliance/status.rs` to keep that file under the workspace
//! 500-line limit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use evidence_core::{CrateEvidence, Dal, ObjectiveStatusKind, generate_compliance_report};

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
    }
}

/// A-7 Obj-5 (statement coverage) upgrades from NotMet /
/// Partial to Met when `coverage_statement_percent` meets the
/// DAL threshold.
#[test]
fn a7_8_met_when_statement_coverage_above_threshold() {
    let evidence = evidence_with_coverage(96.0, 88.0);
    let report = generate_compliance_report("cov-crate", Dal::B, &evidence);
    let a7_8 = report
        .objectives
        .iter()
        .find(|o| o.objective_id == "A7-8")
        .expect("A7-8 present at DAL-B");
    assert_eq!(a7_8.status, ObjectiveStatusKind::Met);
}

#[test]
fn a7_8_partial_when_statement_coverage_below_threshold() {
    let evidence = evidence_with_coverage(92.0, 88.0);
    let report = generate_compliance_report("cov-crate", Dal::B, &evidence);
    let a7_8 = report
        .objectives
        .iter()
        .find(|o| o.objective_id == "A7-8")
        .expect("A7-8 present at DAL-B");
    assert_eq!(a7_8.status, ObjectiveStatusKind::Partial);
}

/// A-7 Obj-6 (branch coverage) mirrors Obj-5's upgrade but
/// reads the `coverage_branch_percent` field and DAL-B's 85%
/// branch minimum.
#[test]
fn a7_9_met_when_branch_coverage_above_threshold() {
    let evidence = evidence_with_coverage(96.0, 88.0);
    let report = generate_compliance_report("cov-crate", Dal::B, &evidence);
    let a7_9 = report
        .objectives
        .iter()
        .find(|o| o.objective_id == "A7-9")
        .expect("A7-9 present at DAL-B");
    assert_eq!(a7_9.status, ObjectiveStatusKind::Met);
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
    };
    let report = generate_compliance_report("no-cov-crate", Dal::C, &evidence);
    let a7_8 = report
        .objectives
        .iter()
        .find(|o| o.objective_id == "A7-8")
        .expect("A7-8 present at DAL-C");
    assert_eq!(a7_8.status, ObjectiveStatusKind::NotMet);
}
