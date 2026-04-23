//! Unit tests for per-objective Table A-7 helpers + the
//! cross-table dispatch invariants. Sibling file via `#[path]`
//! so the parent stays under the 500-line workspace limit.
//!
//! Each A7-* helper gets its own normal / robustness / BVA
//! triplet per DO-178C DAL-A/B verification expectations —
//! single-responsibility testing falls out of the
//! function-size split.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use super::super::generator::generate_compliance_report;
use super::super::report::{CrateEvidence, ObjectiveStatusKind};
use super::{
    a7_1_2_hlr_testing, a7_3_4_llr_testing, a7_5_target_compatibility, a7_6_hlr_test_coverage,
    a7_7_llr_test_coverage, a7_10_mcdc_coverage,
};
use crate::policy::Dal;

/// Baseline evidence: trace + tests + per-test outcomes all
/// present and green. Individual tests override fields to
/// exercise specific cells.
fn evidence_all_green() -> CrateEvidence {
    CrateEvidence {
        has_trace_data: true,
        trace_validation_passed: true,
        has_test_results: true,
        tests_passed: Some(true),
        has_coverage_data: false,
        has_per_test_outcomes: true,
        coverage_statement_percent: None,
        coverage_branch_percent: None,
    }
}

// ---- a7_1_2_hlr_testing: normal + robustness + BVA ----

/// Normal: tests passed + results present → Partial (workspace-
/// aggregate, no per-requirement mapping).
#[test]
fn a7_1_2_normal_passing_tests_is_partial() {
    let e = evidence_all_green();
    let (status, _refs, note) = a7_1_2_hlr_testing(&e);
    assert_eq!(status, ObjectiveStatusKind::Partial);
    assert!(
        note.as_ref()
            .is_some_and(|n| n.contains("per-requirement mapping")),
        "note must explain the Partial cap; got {note:?}"
    );
}

/// Robustness: Some(false) — a red test run cannot earn any
/// A-7 credit. NotMet, distinct note from the "no data" case.
#[test]
fn a7_1_2_robustness_failing_tests_is_notmet() {
    let e = CrateEvidence {
        tests_passed: Some(false),
        ..evidence_all_green()
    };
    let (status, _, note) = a7_1_2_hlr_testing(&e);
    assert_eq!(status, ObjectiveStatusKind::NotMet);
    assert!(note.unwrap().contains("at least one failed"));
}

/// BVA: `tests_passed = None` — no test run recorded at all.
/// NotMet with a distinct reason string so auditor sees "add
/// tests", not "fix a failing test".
#[test]
fn a7_1_2_bva_missing_data_is_notmet_with_no_results_note() {
    let e = CrateEvidence {
        tests_passed: None,
        has_test_results: false,
        ..evidence_all_green()
    };
    let (status, _, note) = a7_1_2_hlr_testing(&e);
    assert_eq!(status, ObjectiveStatusKind::NotMet);
    assert!(note.unwrap().contains("no test results"));
}

// ---- a7_3_4_llr_testing: normal + robustness + BVA ----

/// Normal: per-test outcomes + tests green → Met. The upgrade
/// from Partial → Met is the core claim of the #73 per-test
/// wire format; pin it at the dispatcher level.
#[test]
fn a7_3_4_normal_per_test_outcomes_present_is_met() {
    let e = evidence_all_green();
    let (status, refs, _) = a7_3_4_llr_testing(&e);
    assert_eq!(status, ObjectiveStatusKind::Met);
    assert!(
        refs.iter().any(|r| r.contains("test_outcomes.jsonl")),
        "Met verdict must reference test_outcomes.jsonl; got {refs:?}"
    );
}

/// Robustness: aggregate tests only (no per-test outcomes) →
/// Partial. Proves the upgrade gate actually gates on the
/// outcome file, not on any other aggregate signal.
#[test]
fn a7_3_4_robustness_aggregate_only_is_partial() {
    let e = CrateEvidence {
        has_per_test_outcomes: false,
        ..evidence_all_green()
    };
    let (status, _, _) = a7_3_4_llr_testing(&e);
    assert_eq!(status, ObjectiveStatusKind::Partial);
}

/// BVA: failed tests with per-test outcomes present → NotMet.
/// The outcome file's presence cannot upgrade a red run;
/// `tests_passed = Some(false)` short-circuits.
#[test]
fn a7_3_4_bva_failing_tests_override_outcomes_is_notmet() {
    let e = CrateEvidence {
        tests_passed: Some(false),
        ..evidence_all_green()
    };
    let (status, _, _) = a7_3_4_llr_testing(&e);
    assert_eq!(status, ObjectiveStatusKind::NotMet);
}

// ---- a7_5_target_compatibility ----

/// Normal: green tests on the recorded target → Met.
#[test]
fn a7_5_normal_passing_tests_is_met() {
    let e = evidence_all_green();
    let (status, refs, _) = a7_5_target_compatibility(&e);
    assert_eq!(status, ObjectiveStatusKind::Met);
    assert!(
        refs.iter().any(|r| r == "env.json"),
        "Met verdict must reference env.json so auditor can \
         confirm target triple; got {refs:?}"
    );
}

/// Robustness: failing tests → NotMet. Contrast with the
/// Partial verdict A7-1/2 earns; A7-5's target-compat claim
/// requires green.
#[test]
fn a7_5_robustness_failing_tests_is_notmet() {
    let e = CrateEvidence {
        tests_passed: Some(false),
        ..evidence_all_green()
    };
    let (status, _, _) = a7_5_target_compatibility(&e);
    assert_eq!(status, ObjectiveStatusKind::NotMet);
}

// ---- a7_6 / a7_7 coverage helpers ----

/// Normal: trace + tests + validation → Partial (cap until
/// matrix manual review).
#[test]
fn a7_6_normal_full_evidence_is_partial() {
    let e = evidence_all_green();
    let (status, _, _) = a7_6_hlr_test_coverage(&e);
    assert_eq!(status, ObjectiveStatusKind::Partial);
}

/// Robustness: trace validation failed → NotMet. A6's gate is
/// the same three-way conjunction; if trace validation drops
/// the claim is unfounded.
#[test]
fn a7_6_robustness_trace_validation_failed_is_notmet() {
    let e = CrateEvidence {
        trace_validation_passed: false,
        ..evidence_all_green()
    };
    let (status, _, _) = a7_6_hlr_test_coverage(&e);
    assert_eq!(status, ObjectiveStatusKind::NotMet);
}

/// BVA: A7-7's references list is the LLR half (no
/// cargo_test_stdout reference). Distinguishes its evidence
/// set from A7-6's.
#[test]
fn a7_7_bva_llr_only_references_no_stdout() {
    let e = evidence_all_green();
    let (_, refs, _) = a7_7_llr_test_coverage(&e);
    assert!(
        refs.iter().any(|r| r.contains("matrix.md")),
        "A7-7 refs must include matrix.md; got {refs:?}"
    );
    assert!(
        !refs.iter().any(|r| r.contains("cargo_test_stdout")),
        "A7-7 refs must not include cargo_test_stdout (A7-6's evidence); got {refs:?}"
    );
}

// ---- a7_10_mcdc_coverage: constant NotMet ----

/// Normal = Robustness = BVA: MC/DC is always NotMet until
/// rustc ships stable MC/DC. The objective is observable only
/// as the constant verdict + a reason string pointing at
/// QUALIFICATION.md's gap statement.
#[test]
fn a7_10_is_always_notmet_with_rationale() {
    let (status, refs, note) = a7_10_mcdc_coverage();
    assert_eq!(status, ObjectiveStatusKind::NotMet);
    assert!(refs.is_empty());
    assert!(note.unwrap().contains("MC/DC"));
}

// ---- Cross-table regression tests (pre-existing) ----

/// Core regression: a Some(false) verdict must NOT earn any
/// Table A-7 credit. The previous API treated `tests_passed:
/// bool` as "did the bundle record a test run", which let a
/// failing run still register as partial/met for A7-1..A7-5.
/// With Option<bool>, Some(false) and None both land in NotMet.
#[test]
fn test_failing_tests_mark_all_a7_objectives_not_met() {
    let evidence = CrateEvidence {
        has_trace_data: true,
        trace_validation_passed: true,
        has_test_results: true,
        tests_passed: Some(false),
        has_coverage_data: false,
        has_per_test_outcomes: false,
        ..CrateEvidence::default()
    };
    let report = generate_compliance_report("failing-crate", Dal::A, &evidence);

    for id in ["A7-1", "A7-2", "A7-3", "A7-4", "A7-5"] {
        let obj = report
            .objectives
            .iter()
            .find(|o| o.objective_id == id)
            .unwrap_or_else(|| panic!("objective {} missing from report", id));
        assert_eq!(
            obj.status,
            ObjectiveStatusKind::NotMet,
            "{} must be NotMet when tests_passed = Some(false), got {:?}",
            id,
            obj.status
        );
    }
}

/// No data at all (tests never ran or output couldn't be parsed)
/// also lands in NotMet for every A7-* with a "no test results"
/// note. Distinguishes from Some(false) in wording only; the
/// compliance verdict is the same.
#[test]
fn test_missing_tests_passed_data_marks_a7_not_met() {
    let evidence = CrateEvidence {
        has_trace_data: true,
        trace_validation_passed: true,
        has_test_results: false,
        tests_passed: None,
        has_coverage_data: false,
        has_per_test_outcomes: false,
        ..CrateEvidence::default()
    };
    let report = generate_compliance_report("no-tests-crate", Dal::A, &evidence);

    for id in ["A7-1", "A7-2", "A7-3", "A7-4", "A7-5"] {
        let obj = report
            .objectives
            .iter()
            .find(|o| o.objective_id == id)
            .unwrap_or_else(|| panic!("objective {} missing from report", id));
        assert_eq!(
            obj.status,
            ObjectiveStatusKind::NotMet,
            "{} must be NotMet when tests_passed = None, got {:?}",
            id,
            obj.status
        );
    }
}

/// Positive control: Some(true) earns the partial/met verdicts
/// Table A-7 was designed for, proving the gating logic hasn't
/// accidentally blocked the happy path.
#[test]
fn test_passing_tests_earn_a7_partial_or_met() {
    let evidence = CrateEvidence {
        has_trace_data: true,
        trace_validation_passed: true,
        has_test_results: true,
        tests_passed: Some(true),
        has_coverage_data: false,
        has_per_test_outcomes: false,
        ..CrateEvidence::default()
    };
    let report = generate_compliance_report("passing-crate", Dal::A, &evidence);

    for id in ["A7-1", "A7-2", "A7-3", "A7-4"] {
        let obj = report
            .objectives
            .iter()
            .find(|o| o.objective_id == id)
            .unwrap_or_else(|| panic!("objective {} missing from report", id));
        assert_eq!(
            obj.status,
            ObjectiveStatusKind::Partial,
            "{} must be Partial when tests pass, got {:?}",
            id,
            obj.status
        );
    }

    let a7_5 = report
        .objectives
        .iter()
        .find(|o| o.objective_id == "A7-5")
        .expect("A7-5 missing");
    assert_eq!(a7_5.status, ObjectiveStatusKind::Met);
}

/// Tables A-3, A-4, A-5, A-6 objectives (except the three the
/// tool can mechanically check — A3-6, A4-6, A6-5) must land in
/// ManualReviewRequired, not NotMet. Pins the semantic split
/// introduced alongside the enum.
#[test]
fn test_a3_a4_a5_a6_default_to_manual_review_at_dal_a() {
    let evidence = CrateEvidence {
        has_trace_data: true,
        trace_validation_passed: true,
        has_test_results: true,
        tests_passed: Some(true),
        has_coverage_data: false,
        has_per_test_outcomes: false,
        ..CrateEvidence::default()
    };
    let report = generate_compliance_report("review-crate", Dal::A, &evidence);

    let expected_manual = |id: &str, table: &str| {
        matches!(table, "Table A-3" | "Table A-4" | "Table A-5" | "Table A-6")
            && !matches!(id, "A3-6" | "A4-6" | "A6-5")
    };

    for obj in &report.objectives {
        if expected_manual(&obj.objective_id, &obj.table) {
            assert_eq!(
                obj.status,
                ObjectiveStatusKind::ManualReviewRequired,
                "{} in {} should be ManualReviewRequired, got {:?}",
                obj.objective_id,
                obj.table,
                obj.status
            );
        }
    }
    assert!(
        report.summary.manual_review_required > 0,
        "summary.manual_review_required should be non-zero at DAL-A"
    );
}

/// The four applicable buckets
/// (met + not_met + partial + manual_review_required) must sum
/// to exactly `applicable`. Catches off-by-one bugs in the
/// generator's match-over-enum.
#[test]
fn summary_buckets_cover_every_applicable_objective() {
    for dal in [Dal::A, Dal::B, Dal::C, Dal::D] {
        let evidence = CrateEvidence {
            has_trace_data: true,
            trace_validation_passed: true,
            has_test_results: true,
            tests_passed: Some(true),
            has_coverage_data: false,
            has_per_test_outcomes: false,
            ..CrateEvidence::default()
        };
        let report = generate_compliance_report("exhaustive", dal, &evidence);
        let s = &report.summary;
        assert_eq!(
            s.met + s.not_met + s.partial + s.manual_review_required,
            s.applicable,
            "DAL-{dal}: buckets must sum to applicable"
        );
    }
}

/// A-7 Obj-3/Obj-4 upgrade from `Partial` to `Met` when
/// `has_per_test_outcomes == true` alongside
/// `tests_passed == Some(true)`. Catches silent degradation
/// of the SVR credit when the per-test wire artifact lands.
#[test]
fn a7_3_upgrades_to_met_with_per_test_outcomes() {
    let without = CrateEvidence {
        has_trace_data: true,
        trace_validation_passed: true,
        has_test_results: true,
        tests_passed: Some(true),
        has_coverage_data: false,
        has_per_test_outcomes: false,
        ..CrateEvidence::default()
    };
    let report_without = generate_compliance_report("u", Dal::C, &without);
    let a7_3_without = report_without
        .objectives
        .iter()
        .find(|o| o.objective_id == "A7-3")
        .expect("A7-3 present");
    assert_eq!(
        a7_3_without.status,
        ObjectiveStatusKind::Partial,
        "aggregate-only evidence must stay Partial"
    );

    let with = CrateEvidence {
        has_per_test_outcomes: true,
        ..without
    };
    let report_with = generate_compliance_report("u", Dal::C, &with);
    let a7_3_with = report_with
        .objectives
        .iter()
        .find(|o| o.objective_id == "A7-3")
        .expect("A7-3 present");
    assert_eq!(
        a7_3_with.status,
        ObjectiveStatusKind::Met,
        "per-test outcomes must upgrade A7-3 to Met"
    );
    let report_b = generate_compliance_report("u", Dal::B, &with);
    let a7_4_b = report_b
        .objectives
        .iter()
        .find(|o| o.objective_id == "A7-4")
        .expect("A7-4 present at DAL-B");
    assert_eq!(a7_4_b.status, ObjectiveStatusKind::Met);
}
// A-7 Obj-5/6 upgrade tests live in
// `tests/compliance_coverage_upgrade.rs` (500-line limit).
