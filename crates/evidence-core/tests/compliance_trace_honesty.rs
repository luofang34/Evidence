//! TEST-061: `generate_compliance_report` reflects the actual
//! `trace_validation_passed` flag through A3-6 / A4-6 / A6-5
//! objective status.
//!
//! Pre-fix, `write_compliance_reports` hardcoded
//! `trace_validation_passed: true` — every run claimed the
//! traceability objectives passed even when non-strict (dev)
//! trace validation warned-and-continued. This is the compliance-
//! honesty hole the plumbing fix closes; these tests anchor the
//! library-side state machine that the wire-up feeds.
//!
//! Normal / robustness / BVA per DO-178C DAL-A/B expectations:
//!
//! - **Normal**: `has_trace_data + passed` → A3-6 Met.
//! - **Robustness (warned-continued)**: `has_trace_data +
//!   !passed` → A3-6 Partial, with the reason string naming the
//!   missing validation so an auditor can trace the gap.
//! - **BVA (no trace)**: `!has_trace_data` → A3-6 NotMet,
//!   regardless of `passed`. "Passed" is vacuously true on zero
//!   input; `has_trace_data` is the first-cut gate.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use evidence_core::{CrateEvidence, Dal, ObjectiveStatusKind, generate_compliance_report};

fn evidence(trace_data: bool, trace_validated: bool) -> CrateEvidence {
    CrateEvidence {
        has_trace_data: trace_data,
        trace_validation_passed: trace_validated,
        has_test_results: true,
        tests_passed: Some(true),
        has_coverage_data: false,
        has_per_test_outcomes: false,
        coverage_statement_percent: None,
        coverage_branch_percent: None,
    }
}

fn status_for(evidence: &CrateEvidence, obj_id: &str) -> ObjectiveStatusKind {
    let report = generate_compliance_report("fixture-crate", Dal::B, evidence);
    report
        .objectives
        .iter()
        .find(|o| o.objective_id == obj_id)
        .unwrap_or_else(|| panic!("objective {obj_id} not found"))
        .status
}

// ---- LLR-061 normal: passed=true + has=true → Met ----

/// A3-6 (HLR traceability, Table A-3 Obj-6) is the cleanest
/// signal for the wire-up: Met when both flags line up.
#[test]
fn a3_6_normal_passed_and_present_is_met() {
    let e = evidence(/*trace_data=*/ true, /*validated=*/ true);
    assert_eq!(status_for(&e, "A3-6"), ObjectiveStatusKind::Met);
}

/// A4-6 (LLR-to-HLR traceability) mirrors A3-6's wire.
#[test]
fn a4_6_normal_passed_and_present_is_met() {
    let e = evidence(true, true);
    assert_eq!(status_for(&e, "A4-6"), ObjectiveStatusKind::Met);
}

// A6-5 is trace-driven but always caps at Partial (needs
// per-test source-to-LLR mapping beyond link validity). Not a
// clean observable for the honesty fix; A3-6 / A4-6 are the
// discriminators.

// ---- LLR-061 robustness: warned-continued → Partial ----

/// **The bug this PR closes.** `has_trace_data = true` but
/// `trace_validation_passed = false` must produce Partial, not
/// Met. Pre-fix, this state was unreachable because the CLI
/// hardcoded `true`; the library logic was already correct, but
/// the wire never fed a `false`.
#[test]
fn a3_6_robustness_warned_continuing_is_partial() {
    let e = evidence(/*trace_data=*/ true, /*validated=*/ false);
    assert_eq!(status_for(&e, "A3-6"), ObjectiveStatusKind::Partial);
}

#[test]
fn a4_6_robustness_warned_continuing_is_partial() {
    let e = evidence(true, false);
    assert_eq!(status_for(&e, "A4-6"), ObjectiveStatusKind::Partial);
}

/// Partial status must carry a `reason` string that names the
/// failing condition — without it, an auditor reading the
/// compliance JSON has no signal for what to follow up on.
#[test]
fn a3_6_partial_carries_reason_naming_the_gap() {
    let e = evidence(true, false);
    let report = generate_compliance_report("fixture-crate", Dal::B, &e);
    let a3_6 = report
        .objectives
        .iter()
        .find(|o| o.objective_id == "A3-6")
        .unwrap();
    let reason = a3_6.note.as_ref().expect(
        "Partial status must carry a reason; otherwise the \
         auditor sees a downgrade with no signal for the gap",
    );
    assert!(
        reason.contains("trace validation"),
        "reason must name the missing validation; got: {reason}"
    );
}

// ---- LLR-061 BVA: no trace data → NotMet regardless of passed ----

/// Boundary case at the first-cut gate: `has_trace_data =
/// false` shadows `trace_validation_passed` entirely. "Passed"
/// is vacuously true on zero input; what the compliance report
/// needs is a distinct NotMet status with its own reason.
#[test]
fn a3_6_bva_no_trace_data_is_notmet_even_when_passed_true() {
    let e = evidence(/*trace_data=*/ false, /*validated=*/ true);
    assert_eq!(status_for(&e, "A3-6"), ObjectiveStatusKind::NotMet);
}

#[test]
fn a3_6_bva_no_trace_data_is_notmet_even_when_passed_false() {
    let e = evidence(false, false);
    assert_eq!(status_for(&e, "A3-6"), ObjectiveStatusKind::NotMet);
}

/// NotMet reason must name the missing data, not the missing
/// validation — an auditor needs to know "add trace files",
/// not "fix your validation". Distinct signal from Partial.
#[test]
fn a3_6_notmet_reason_is_no_trace_data_not_validation_failure() {
    let e = evidence(false, true);
    let report = generate_compliance_report("fixture-crate", Dal::B, &e);
    let a3_6 = report
        .objectives
        .iter()
        .find(|o| o.objective_id == "A3-6")
        .unwrap();
    let reason = a3_6.note.as_ref().expect("NotMet carries note");
    assert!(
        reason.contains("no trace data") || reason.contains("trace data available"),
        "NotMet reason must name missing data, not missing \
         validation; got: {reason}"
    );
}
