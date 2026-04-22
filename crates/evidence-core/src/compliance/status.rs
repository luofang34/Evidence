//! Map a (objective, crate-evidence) pair to a compliance verdict.
//!
//! `determine_objective_status` dispatches by DO-178C table (A-3 through
//! A-7); Table A-7 is large enough to live in its own helper,
//! `determine_a7_status`.
//!
//! Every Table A-7 helper treats `tests_passed: Option<bool>` as
//! three-valued: only `Some(true)` earns credit. A failing run
//! (`Some(false)`) and a missing run (`None`) both land in `NotMet`
//! — a certification argument must not rely on red or absent tests.
//!
//! Tables A-3, A-4, A-5, and A-6 mostly yield `ManualReviewRequired`:
//! the tool can only mechanically check traceability links (A3-6,
//! A4-6, A6-5). Every other objective in those tables requires a
//! human reviewer to judge design / implementation correctness.

use super::objective::Objective;
use super::report::{CrateEvidence, ObjectiveStatusKind};

/// Concise tuple to spare us typing the `String, Vec<String>,
/// Option<String>` triad on every return.
type Verdict = (ObjectiveStatusKind, Vec<String>, Option<String>);

fn manual_review(note: &str) -> Verdict {
    (
        ObjectiveStatusKind::ManualReviewRequired,
        vec![],
        Some(note.to_string()),
    )
}

/// Determine status for a single objective based on available evidence.
pub(super) fn determine_objective_status(obj: &Objective, evidence: &CrateEvidence) -> Verdict {
    match obj.table {
        "Table A-3" => {
            if obj.id == "A3-6" {
                // Traceability objective: can be checked by tool
                if evidence.has_trace_data && evidence.trace_validation_passed {
                    (
                        ObjectiveStatusKind::Met,
                        vec!["trace/hlr.toml".to_string(), "trace/matrix.md".to_string()],
                        None,
                    )
                } else if evidence.has_trace_data {
                    (
                        ObjectiveStatusKind::Partial,
                        vec!["trace/hlr.toml".to_string()],
                        Some("trace validation did not pass".to_string()),
                    )
                } else {
                    (
                        ObjectiveStatusKind::NotMet,
                        vec![],
                        Some("no trace data available".to_string()),
                    )
                }
            } else {
                manual_review("reviewer must confirm HLR correctness / completeness")
            }
        }
        "Table A-4" => {
            if obj.id == "A4-6" {
                // LLR-to-HLR traceability
                if evidence.has_trace_data && evidence.trace_validation_passed {
                    (
                        ObjectiveStatusKind::Met,
                        vec!["trace/llr.toml".to_string(), "trace/matrix.md".to_string()],
                        None,
                    )
                } else if evidence.has_trace_data {
                    (
                        ObjectiveStatusKind::Partial,
                        vec!["trace/llr.toml".to_string()],
                        Some("trace validation did not pass".to_string()),
                    )
                } else {
                    (
                        ObjectiveStatusKind::NotMet,
                        vec![],
                        Some("no trace data available".to_string()),
                    )
                }
            } else {
                manual_review("reviewer must confirm LLR correctness / completeness")
            }
        }
        "Table A-5" => manual_review("reviewer must confirm architecture consistency"),
        "Table A-6" => {
            if obj.id == "A6-5" {
                // Source-to-LLR traceability
                if evidence.has_trace_data && evidence.trace_validation_passed {
                    (
                        ObjectiveStatusKind::Partial,
                        vec!["trace/llr.toml".to_string()],
                        Some(
                            "trace links exist but source-level mapping requires test_selector"
                                .to_string(),
                        ),
                    )
                } else {
                    (
                        ObjectiveStatusKind::NotMet,
                        vec![],
                        Some("no trace data available".to_string()),
                    )
                }
            } else {
                manual_review("reviewer or static-analysis tool must judge source compliance")
            }
        }
        "Table A-7" => determine_a7_status(obj, evidence),
        _ => (ObjectiveStatusKind::NotMet, vec![], None),
    }
}

/// Determine status for Table A-7 (testing) objectives.
fn determine_a7_status(obj: &Objective, evidence: &CrateEvidence) -> Verdict {
    match obj.id {
        "A7-1" | "A7-2" => {
            // HLR-level testing. Only reports any credit when tests
            // actually passed; Some(false)/None both fall into
            // NotMet so a red test run can't prop up the objective.
            match evidence.tests_passed {
                Some(true) if evidence.has_test_results => (
                    ObjectiveStatusKind::Partial,
                    vec!["tests/cargo_test_stdout.txt".to_string()],
                    Some("aggregate test results available; per-requirement mapping requires test_selector".to_string()),
                ),
                Some(false) => (
                    ObjectiveStatusKind::NotMet,
                    vec!["tests/cargo_test_stdout.txt".to_string()],
                    Some("tests ran but at least one failed".to_string()),
                ),
                _ => (
                    ObjectiveStatusKind::NotMet,
                    vec![],
                    Some("no test results in bundle".to_string()),
                ),
            }
        }
        "A7-3" | "A7-4" => {
            // LLR-level testing. Per-test outcome atoms
            // (`tests/test_outcomes.jsonl`) upgrade this from
            // Partial → Met because an auditor asking "show me
            // the result of TEST-046" resolves to a specific row
            // instead of the workspace-aggregate boolean.
            match (evidence.tests_passed, evidence.has_per_test_outcomes) {
                (Some(true), true) if evidence.has_test_results => (
                    ObjectiveStatusKind::Met,
                    vec![
                        "tests/test_outcomes.jsonl".to_string(),
                        "tests/cargo_test_stdout.txt".to_string(),
                    ],
                    Some("per-test outcome atoms captured in test_outcomes.jsonl".to_string()),
                ),
                (Some(true), false) if evidence.has_test_results => (
                    ObjectiveStatusKind::Partial,
                    vec!["tests/cargo_test_stdout.txt".to_string()],
                    Some(
                        "aggregate test results available; per-LLR mapping requires test_selector"
                            .to_string(),
                    ),
                ),
                (Some(false), _) => (
                    ObjectiveStatusKind::NotMet,
                    vec!["tests/cargo_test_stdout.txt".to_string()],
                    Some("tests ran but at least one failed".to_string()),
                ),
                _ => (
                    ObjectiveStatusKind::NotMet,
                    vec![],
                    Some("no test results in bundle".to_string()),
                ),
            }
        }
        "A7-5" => {
            // Target compatibility — only "Met" when tests actually
            // passed on the recorded target.
            match evidence.tests_passed {
                Some(true) if evidence.has_test_results => (
                    ObjectiveStatusKind::Met,
                    vec![
                        "tests/cargo_test_stdout.txt".to_string(),
                        "env.json".to_string(),
                    ],
                    None,
                ),
                Some(false) => (
                    ObjectiveStatusKind::NotMet,
                    vec!["tests/cargo_test_stdout.txt".to_string()],
                    Some("tests ran but at least one failed".to_string()),
                ),
                _ => (
                    ObjectiveStatusKind::NotMet,
                    vec![],
                    Some("no passing test results".to_string()),
                ),
            }
        }
        "A7-6" => {
            // HLR test coverage
            if evidence.has_trace_data
                && evidence.has_test_results
                && evidence.trace_validation_passed
            {
                (ObjectiveStatusKind::Partial, vec!["trace/matrix.md".to_string(), "tests/cargo_test_stdout.txt".to_string()], Some("traceability matrix shows HLR-to-test links; completeness requires manual review".to_string()))
            } else {
                (ObjectiveStatusKind::NotMet, vec![], None)
            }
        }
        "A7-7" => {
            // LLR test coverage
            if evidence.has_trace_data
                && evidence.has_test_results
                && evidence.trace_validation_passed
            {
                (ObjectiveStatusKind::Partial, vec!["trace/matrix.md".to_string()], Some("traceability matrix shows LLR-to-test links; completeness requires manual review".to_string()))
            } else {
                (ObjectiveStatusKind::NotMet, vec![], None)
            }
        }
        "A7-8" => {
            // Statement coverage
            if evidence.has_coverage_data {
                (
                    ObjectiveStatusKind::Partial,
                    vec![],
                    Some("coverage data exists; threshold check not yet implemented".to_string()),
                )
            } else {
                (
                    ObjectiveStatusKind::NotMet,
                    vec![],
                    Some("no structural coverage data".to_string()),
                )
            }
        }
        "A7-9" => {
            // Decision coverage
            if evidence.has_coverage_data {
                (
                    ObjectiveStatusKind::Partial,
                    vec![],
                    Some(
                        "coverage data exists; decision coverage extraction not yet implemented"
                            .to_string(),
                    ),
                )
            } else {
                (
                    ObjectiveStatusKind::NotMet,
                    vec![],
                    Some("no structural coverage data".to_string()),
                )
            }
        }
        "A7-10" => {
            // MC/DC coverage — tool capability gap, not a review
            // item. NotMet is the honest verdict.
            (
                ObjectiveStatusKind::NotMet,
                vec![],
                Some("MC/DC analysis not yet supported".to_string()),
            )
        }
        _ => (ObjectiveStatusKind::NotMet, vec![], None),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::super::generator::generate_compliance_report;
    use super::super::report::{CrateEvidence, ObjectiveStatusKind};
    use crate::policy::Dal;

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
        };
        let report = generate_compliance_report("review-crate", Dal::A, &evidence);

        // Every objective in A-3/A-4/A-5/A-6 except A3-6, A4-6,
        // A6-5 should be ManualReviewRequired. (A6-5 becomes
        // Partial when trace validation passes.)
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
        // And the summary should reflect a non-zero review load.
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
}
