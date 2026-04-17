//! Map a (objective, crate-evidence) pair to a compliance verdict.
//!
//! `determine_objective_status` dispatches by DO-178C table (A-3 through
//! A-7); Table A-7 is large enough to live in its own helper,
//! `determine_a7_status`.
//!
//! Every Table A-7 helper treats `tests_passed: Option<bool>` as
//! three-valued: only `Some(true)` earns credit. A failing run
//! (`Some(false)`) and a missing run (`None`) both land in `not_met`
//! — a certification argument must not rely on red or absent tests.

use super::objective::Objective;
use super::report::CrateEvidence;

/// Determine status for a single objective based on available evidence.
pub(super) fn determine_objective_status(
    obj: &Objective,
    evidence: &CrateEvidence,
) -> (String, Vec<String>, Option<String>) {
    match obj.table {
        // Table A-3: HLR verification -- depends on trace data
        "Table A-3" => {
            if obj.id == "A3-6" {
                // Traceability objective: can be checked by tool
                if evidence.has_trace_data && evidence.trace_validation_passed {
                    (
                        "met".to_string(),
                        vec!["trace/hlr.toml".to_string(), "trace/matrix.md".to_string()],
                        None,
                    )
                } else if evidence.has_trace_data {
                    (
                        "partial".to_string(),
                        vec!["trace/hlr.toml".to_string()],
                        Some("trace validation did not pass".to_string()),
                    )
                } else {
                    (
                        "not_met".to_string(),
                        vec![],
                        Some("no trace data available".to_string()),
                    )
                }
            } else {
                // Other A-3 objectives require manual review
                (
                    "not_met".to_string(),
                    vec![],
                    Some("requires manual review".to_string()),
                )
            }
        }
        // Table A-4: LLR verification -- depends on trace data
        "Table A-4" => {
            if obj.id == "A4-6" {
                // LLR-to-HLR traceability
                if evidence.has_trace_data && evidence.trace_validation_passed {
                    (
                        "met".to_string(),
                        vec!["trace/llr.toml".to_string(), "trace/matrix.md".to_string()],
                        None,
                    )
                } else if evidence.has_trace_data {
                    (
                        "partial".to_string(),
                        vec!["trace/llr.toml".to_string()],
                        Some("trace validation did not pass".to_string()),
                    )
                } else {
                    (
                        "not_met".to_string(),
                        vec![],
                        Some("no trace data available".to_string()),
                    )
                }
            } else {
                (
                    "not_met".to_string(),
                    vec![],
                    Some("requires manual review".to_string()),
                )
            }
        }
        // Table A-5: Architecture -- all require manual review
        "Table A-5" => (
            "not_met".to_string(),
            vec![],
            Some("requires manual review".to_string()),
        ),
        // Table A-6: Source code verification
        "Table A-6" => {
            if obj.id == "A6-5" {
                // Source-to-LLR traceability
                if evidence.has_trace_data && evidence.trace_validation_passed {
                    (
                        "partial".to_string(),
                        vec!["trace/llr.toml".to_string()],
                        Some(
                            "trace links exist but source-level mapping requires test_selector"
                                .to_string(),
                        ),
                    )
                } else {
                    (
                        "not_met".to_string(),
                        vec![],
                        Some("no trace data available".to_string()),
                    )
                }
            } else {
                (
                    "not_met".to_string(),
                    vec![],
                    Some("requires manual review or static analysis".to_string()),
                )
            }
        }
        // Table A-7: Testing
        "Table A-7" => determine_a7_status(obj, evidence),
        _ => ("not_met".to_string(), vec![], None),
    }
}

/// Determine status for Table A-7 (testing) objectives.
fn determine_a7_status(
    obj: &Objective,
    evidence: &CrateEvidence,
) -> (String, Vec<String>, Option<String>) {
    match obj.id {
        "A7-1" | "A7-2" => {
            // HLR-level testing. Only reports any credit when tests
            // actually passed; Some(false)/None both fall into
            // not_met so a red test run can't prop up the objective.
            match evidence.tests_passed {
                Some(true) if evidence.has_test_results => (
                    "partial".to_string(),
                    vec!["tests/cargo_test_stdout.txt".to_string()],
                    Some("aggregate test results available; per-requirement mapping requires test_selector".to_string()),
                ),
                Some(false) => (
                    "not_met".to_string(),
                    vec!["tests/cargo_test_stdout.txt".to_string()],
                    Some("tests ran but at least one failed".to_string()),
                ),
                _ => (
                    "not_met".to_string(),
                    vec![],
                    Some("no test results in bundle".to_string()),
                ),
            }
        }
        "A7-3" | "A7-4" => {
            // LLR-level testing
            match evidence.tests_passed {
                Some(true) if evidence.has_test_results => (
                    "partial".to_string(),
                    vec!["tests/cargo_test_stdout.txt".to_string()],
                    Some(
                        "aggregate test results available; per-LLR mapping requires test_selector"
                            .to_string(),
                    ),
                ),
                Some(false) => (
                    "not_met".to_string(),
                    vec!["tests/cargo_test_stdout.txt".to_string()],
                    Some("tests ran but at least one failed".to_string()),
                ),
                _ => (
                    "not_met".to_string(),
                    vec![],
                    Some("no test results in bundle".to_string()),
                ),
            }
        }
        "A7-5" => {
            // Target compatibility — only "met" when tests actually
            // passed on the recorded target.
            match evidence.tests_passed {
                Some(true) if evidence.has_test_results => (
                    "met".to_string(),
                    vec![
                        "tests/cargo_test_stdout.txt".to_string(),
                        "env.json".to_string(),
                    ],
                    None,
                ),
                Some(false) => (
                    "not_met".to_string(),
                    vec!["tests/cargo_test_stdout.txt".to_string()],
                    Some("tests ran but at least one failed".to_string()),
                ),
                _ => (
                    "not_met".to_string(),
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
                ("partial".to_string(), vec!["trace/matrix.md".to_string(), "tests/cargo_test_stdout.txt".to_string()], Some("traceability matrix shows HLR-to-test links; completeness requires manual review".to_string()))
            } else {
                ("not_met".to_string(), vec![], None)
            }
        }
        "A7-7" => {
            // LLR test coverage
            if evidence.has_trace_data
                && evidence.has_test_results
                && evidence.trace_validation_passed
            {
                ("partial".to_string(), vec!["trace/matrix.md".to_string()], Some("traceability matrix shows LLR-to-test links; completeness requires manual review".to_string()))
            } else {
                ("not_met".to_string(), vec![], None)
            }
        }
        "A7-8" => {
            // Statement coverage
            if evidence.has_coverage_data {
                (
                    "partial".to_string(),
                    vec![],
                    Some("coverage data exists; threshold check not yet implemented".to_string()),
                )
            } else {
                (
                    "not_met".to_string(),
                    vec![],
                    Some("no structural coverage data".to_string()),
                )
            }
        }
        "A7-9" => {
            // Decision coverage
            if evidence.has_coverage_data {
                (
                    "partial".to_string(),
                    vec![],
                    Some(
                        "coverage data exists; decision coverage extraction not yet implemented"
                            .to_string(),
                    ),
                )
            } else {
                (
                    "not_met".to_string(),
                    vec![],
                    Some("no structural coverage data".to_string()),
                )
            }
        }
        "A7-10" => {
            // MC/DC coverage
            (
                "not_met".to_string(),
                vec![],
                Some("MC/DC analysis not yet supported".to_string()),
            )
        }
        _ => ("not_met".to_string(), vec![], None),
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
    use super::super::report::CrateEvidence;
    use crate::policy::Dal;

    /// Core regression: a Some(false) verdict must NOT earn any
    /// Table A-7 credit. The previous API treated `tests_passed:
    /// bool` as "did the bundle record a test run", which let a
    /// failing run still register as partial/met for A7-1..A7-5.
    /// With Option<bool>, Some(false) and None both land in not_met.
    #[test]
    fn test_failing_tests_mark_all_a7_objectives_not_met() {
        let evidence = CrateEvidence {
            has_trace_data: true,
            trace_validation_passed: true,
            has_test_results: true,
            tests_passed: Some(false),
            has_coverage_data: false,
        };
        let report = generate_compliance_report("failing-crate", Dal::A, &evidence);

        for id in ["A7-1", "A7-2", "A7-3", "A7-4", "A7-5"] {
            let obj = report
                .objectives
                .iter()
                .find(|o| o.objective_id == id)
                .unwrap_or_else(|| panic!("objective {} missing from report", id));
            assert_eq!(
                obj.status, "not_met",
                "{} must be not_met when tests_passed = Some(false), got {}",
                id, obj.status
            );
        }
    }

    /// No data at all (tests never ran or output couldn't be parsed)
    /// also lands in not_met for every A7-* with a "no test results"
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
        };
        let report = generate_compliance_report("no-tests-crate", Dal::A, &evidence);

        for id in ["A7-1", "A7-2", "A7-3", "A7-4", "A7-5"] {
            let obj = report
                .objectives
                .iter()
                .find(|o| o.objective_id == id)
                .unwrap_or_else(|| panic!("objective {} missing from report", id));
            assert_eq!(
                obj.status, "not_met",
                "{} must be not_met when tests_passed = None, got {}",
                id, obj.status
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
        };
        let report = generate_compliance_report("passing-crate", Dal::A, &evidence);

        for id in ["A7-1", "A7-2", "A7-3", "A7-4"] {
            let obj = report
                .objectives
                .iter()
                .find(|o| o.objective_id == id)
                .unwrap_or_else(|| panic!("objective {} missing from report", id));
            assert_eq!(
                obj.status, "partial",
                "{} must be partial when tests pass, got {}",
                id, obj.status
            );
        }

        let a7_5 = report
            .objectives
            .iter()
            .find(|o| o.objective_id == "A7-5")
            .expect("A7-5 missing");
        assert_eq!(a7_5.status, "met");
    }
}
