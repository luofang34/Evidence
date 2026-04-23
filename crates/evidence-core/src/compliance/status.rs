//! Map a (objective, crate-evidence) pair to a compliance verdict.
//!
//! `determine_objective_status` dispatches by DO-178C table (A-3 — A-7);
//! Table A-7 lives in a separate helper `determine_a7_status`.
//! `tests_passed: Option<bool>` is three-valued: only `Some(true)`
//! earns credit — `Some(false)` + `None` both map to `NotMet` so a
//! cert argument can't rely on red or absent tests. Tables A-3/A-4/
//! A-5/A-6 mostly yield `ManualReviewRequired`; the tool only
//! mechanically checks traceability links (A3-6, A4-6, A6-5).

use super::coverage_verdict::coverage_verdict;
use super::objective::Objective;
use super::report::{CrateEvidence, ObjectiveStatusKind};
use crate::policy::Dal;

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
pub(super) fn determine_objective_status(
    obj: &Objective,
    dal: Dal,
    evidence: &CrateEvidence,
) -> Verdict {
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
        "Table A-7" => determine_a7_status(obj, dal, evidence),
        _ => (ObjectiveStatusKind::NotMet, vec![], None),
    }
}

/// Determine status for Table A-7 (testing) objectives. Dispatches
/// to per-objective helpers so each arm is independently testable
/// (DO-178C single-responsibility for verification); pre-split,
/// a 137-line match body bundled eight distinct state machines
/// into one function.
fn determine_a7_status(obj: &Objective, dal: Dal, evidence: &CrateEvidence) -> Verdict {
    match obj.id {
        "A7-1" | "A7-2" => a7_1_2_hlr_testing(evidence),
        "A7-3" | "A7-4" => a7_3_4_llr_testing(evidence),
        "A7-5" => a7_5_target_compatibility(evidence),
        "A7-6" => a7_6_hlr_test_coverage(evidence),
        "A7-7" => a7_7_llr_test_coverage(evidence),
        "A7-8" => a7_8_statement_coverage(dal, evidence),
        "A7-9" => a7_9_decision_coverage(dal, evidence),
        "A7-10" => a7_10_mcdc_coverage(),
        _ => (ObjectiveStatusKind::NotMet, vec![], None),
    }
}

/// A7-1, A7-2: HLR-level testing. Only reports any credit when
/// tests actually passed; `Some(false)` / `None` both fall into
/// NotMet so a red test run can't prop up the objective.
fn a7_1_2_hlr_testing(evidence: &CrateEvidence) -> Verdict {
    match evidence.tests_passed {
        Some(true) if evidence.has_test_results => (
            ObjectiveStatusKind::Partial,
            vec!["tests/cargo_test_stdout.txt".to_string()],
            Some(
                "aggregate test results available; per-requirement mapping requires test_selector"
                    .to_string(),
            ),
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

/// A7-3, A7-4: LLR-level testing. Per-test outcome atoms
/// (`tests/test_outcomes.jsonl`) upgrade this from Partial → Met
/// because an auditor asking "show me the result of TEST-046"
/// resolves to a specific row instead of the workspace-aggregate
/// boolean.
fn a7_3_4_llr_testing(evidence: &CrateEvidence) -> Verdict {
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

/// A7-5: target compatibility — only "Met" when tests actually
/// passed on the recorded target.
fn a7_5_target_compatibility(evidence: &CrateEvidence) -> Verdict {
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

/// A7-6: HLR test coverage — requires trace, tests, and trace
/// validation. Caps at Partial; full coverage claim needs manual
/// review of the matrix.
fn a7_6_hlr_test_coverage(evidence: &CrateEvidence) -> Verdict {
    if evidence.has_trace_data && evidence.has_test_results && evidence.trace_validation_passed {
        (
            ObjectiveStatusKind::Partial,
            vec![
                "trace/matrix.md".to_string(),
                "tests/cargo_test_stdout.txt".to_string(),
            ],
            Some(
                "traceability matrix shows HLR-to-test links; completeness requires manual review"
                    .to_string(),
            ),
        )
    } else {
        (ObjectiveStatusKind::NotMet, vec![], None)
    }
}

/// A7-7: LLR test coverage — same gates as A7-6 but the evidence
/// set is the LLR half of the matrix.
fn a7_7_llr_test_coverage(evidence: &CrateEvidence) -> Verdict {
    if evidence.has_trace_data && evidence.has_test_results && evidence.trace_validation_passed {
        (
            ObjectiveStatusKind::Partial,
            vec!["trace/matrix.md".to_string()],
            Some(
                "traceability matrix shows LLR-to-test links; completeness requires manual review"
                    .to_string(),
            ),
        )
    } else {
        (ObjectiveStatusKind::NotMet, vec![], None)
    }
}

/// A7-8: statement coverage. Upgrades to Met when the aggregate
/// percent from `coverage/coverage_summary.json` meets the
/// DO-178C threshold for this DAL.
fn a7_8_statement_coverage(dal: Dal, evidence: &CrateEvidence) -> Verdict {
    coverage_verdict(
        evidence.coverage_statement_percent,
        dal.coverage_thresholds().statement_percent,
        "statement",
    )
}

/// A7-9: decision coverage — LLVM branch coverage is our
/// approximation (see `cert/QUALIFICATION.md` for the semantic
/// gap statement).
fn a7_9_decision_coverage(dal: Dal, evidence: &CrateEvidence) -> Verdict {
    coverage_verdict(
        evidence.coverage_branch_percent,
        dal.coverage_thresholds().branch_percent,
        "branch",
    )
}

/// A7-10: MC/DC coverage — tool capability gap, not a review
/// item. NotMet is the honest verdict; see `cert/QUALIFICATION.md`
/// for the rustc stabilization caveat.
fn a7_10_mcdc_coverage() -> Verdict {
    (
        ObjectiveStatusKind::NotMet,
        vec![],
        Some("MC/DC analysis not yet supported".to_string()),
    )
}

// Tests live in the sibling `status/tests.rs` via `#[path]` so
// this file stays under the 500-line workspace limit. Each A7-*
// helper gets a normal/robustness/BVA triplet (single-
// responsibility verification per DO-178C DAL-A/B expectations).
#[cfg(test)]
#[path = "status/tests.rs"]
mod tests;
