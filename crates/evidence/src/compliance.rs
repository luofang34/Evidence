//! Per-crate DO-178C compliance reporting.
//!
//! This module generates compliance status reports for each in-scope crate
//! based on its declared DAL level and available evidence in the bundle.

use serde::{Deserialize, Serialize};

use crate::policy::Dal;

// ============================================================================
// Objective Applicability
// ============================================================================

/// DO-178C objective applicability level.
/// Three-level per standard: not applicable, required, or required with independence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Applicability {
    /// Objective does not apply at this DAL level
    NotApplicable,
    /// Objective is required
    Required,
    /// Objective is required with independent verification
    RequiredWithIndependence,
}

// ============================================================================
// Objective Definition
// ============================================================================

/// A single DO-178C objective from Annex A tables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Objective {
    /// Unique objective ID (e.g., "A3-1", "A7-3")
    pub id: &'static str,
    /// DO-178C table reference (e.g., "Table A-3")
    pub table: &'static str,
    /// Human-readable objective title
    pub title: &'static str,
    /// Applicability per DAL level: [A, B, C, D]
    pub applicability: [Applicability; 4],
}

impl Objective {
    /// Get applicability for a specific DAL level.
    pub fn applicability_for(&self, dal: Dal) -> Applicability {
        match dal {
            Dal::A => self.applicability[0],
            Dal::B => self.applicability[1],
            Dal::C => self.applicability[2],
            Dal::D => self.applicability[3],
        }
    }
}

// ============================================================================
// Objective Matrix (DO-178C Annex A subset)
// ============================================================================

// Abbreviations for readability
use Applicability::{NotApplicable as NA, Required as R, RequiredWithIndependence as RI};

/// DO-178C Annex A objectives relevant to tool-automatable verification.
/// This covers Tables A-3 through A-7 (the tables where cargo-evidence can
/// provide evidence). Tables A-1, A-2, A-8 through A-10 are process/management
/// objectives that require human documentation, not tool output.
pub static OBJECTIVES: &[Objective] = &[
    // Table A-3: Verification of HLR
    Objective {
        id: "A3-1",
        table: "Table A-3",
        title: "HLR comply with system requirements",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A3-2",
        table: "Table A-3",
        title: "HLR are accurate and consistent",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A3-3",
        table: "Table A-3",
        title: "HLR are compatible with target computer",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A3-4",
        table: "Table A-3",
        title: "HLR are verifiable",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A3-5",
        table: "Table A-3",
        title: "HLR conform to standards",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A3-6",
        table: "Table A-3",
        title: "HLR are traceable to system requirements",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A3-7",
        table: "Table A-3",
        title: "Algorithms are accurate",
        applicability: [RI, RI, R, NA],
    },
    // Table A-4: Verification of LLR
    Objective {
        id: "A4-1",
        table: "Table A-4",
        title: "LLR comply with HLR",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A4-2",
        table: "Table A-4",
        title: "LLR are accurate and consistent",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A4-3",
        table: "Table A-4",
        title: "LLR are compatible with target computer",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A4-4",
        table: "Table A-4",
        title: "LLR are verifiable",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A4-5",
        table: "Table A-4",
        title: "LLR conform to standards",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A4-6",
        table: "Table A-4",
        title: "LLR are traceable to HLR",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A4-7",
        table: "Table A-4",
        title: "LLR algorithms are accurate",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A4-8",
        table: "Table A-4",
        title: "LLR are compatible with target",
        applicability: [RI, RI, R, NA],
    },
    // Table A-5: Verification of Software Architecture
    Objective {
        id: "A5-1",
        table: "Table A-5",
        title: "Architecture is compatible with HLR",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A5-2",
        table: "Table A-5",
        title: "Architecture is consistent",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A5-3",
        table: "Table A-5",
        title: "Architecture is compatible with target",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A5-4",
        table: "Table A-5",
        title: "Architecture is verifiable",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A5-5",
        table: "Table A-5",
        title: "Architecture conforms to standards",
        applicability: [RI, RI, R, NA],
    },
    // Table A-6: Verification of Source Code
    Objective {
        id: "A6-1",
        table: "Table A-6",
        title: "Source code complies with LLR",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A6-2",
        table: "Table A-6",
        title: "Source code complies with architecture",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A6-3",
        table: "Table A-6",
        title: "Source code is verifiable",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A6-4",
        table: "Table A-6",
        title: "Source code conforms to standards",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A6-5",
        table: "Table A-6",
        title: "Source code is traceable to LLR",
        applicability: [RI, RI, R, R],
    },
    // Table A-7: Verification of Integration (Testing)
    Objective {
        id: "A7-1",
        table: "Table A-7",
        title: "Executable object code complies with HLR",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A7-2",
        table: "Table A-7",
        title: "Executable object code is robust with HLR",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A7-3",
        table: "Table A-7",
        title: "Executable object code complies with LLR",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A7-4",
        table: "Table A-7",
        title: "Executable object code is robust with LLR",
        applicability: [RI, RI, NA, NA],
    },
    Objective {
        id: "A7-5",
        table: "Table A-7",
        title: "Executable object code is compatible with target",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A7-6",
        table: "Table A-7",
        title: "Test coverage of HLR is achieved",
        applicability: [RI, RI, R, R],
    },
    Objective {
        id: "A7-7",
        table: "Table A-7",
        title: "Test coverage of LLR is achieved",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A7-8",
        table: "Table A-7",
        title: "Test coverage of software structure (statement) is achieved",
        applicability: [RI, RI, R, NA],
    },
    Objective {
        id: "A7-9",
        table: "Table A-7",
        title: "Test coverage of software structure (decision) is achieved",
        applicability: [RI, RI, NA, NA],
    },
    Objective {
        id: "A7-10",
        table: "Table A-7",
        title: "Test coverage of software structure (MC/DC) is achieved",
        applicability: [RI, NA, NA, NA],
    },
];

// ============================================================================
// Compliance Report Types
// ============================================================================

/// Status of a single objective for a specific crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectiveStatus {
    /// Objective ID (e.g., "A3-1")
    pub objective_id: String,
    /// DO-178C table reference
    pub table: String,
    /// Human-readable title
    pub title: String,
    /// Whether this objective is applicable at the crate's declared DAL
    pub applicable: bool,
    /// Applicability detail (required, required_with_independence, not_applicable)
    pub applicability_detail: String,
    /// Status: "met", "not_met", "partial", "not_applicable"
    pub status: String,
    /// Evidence reference(s) (file paths within the bundle)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
    /// Human-readable note
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Summary counts for a compliance report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceSummary {
    pub total_objectives: u32,
    pub applicable: u32,
    pub met: u32,
    pub not_met: u32,
    pub partial: u32,
}

/// Per-crate compliance report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReport {
    /// Crate name
    pub crate_name: String,
    /// Declared DAL level
    pub dal: String,
    /// Schema version of this compliance report
    pub schema_version: String,
    /// List of objectives with their status
    pub objectives: Vec<ObjectiveStatus>,
    /// Summary counts
    pub summary: ComplianceSummary,
}

// ============================================================================
// Report Generation
// ============================================================================

/// Evidence available for a crate (passed into report generation).
#[derive(Debug, Default)]
pub struct CrateEvidence {
    /// Whether trace data (HLR/LLR/test TOML files) exists for this crate
    pub has_trace_data: bool,
    /// Whether trace validation passed
    pub trace_validation_passed: bool,
    /// Whether test results exist
    pub has_test_results: bool,
    /// Did all tests pass?
    ///
    /// - `None` — no data, either because tests were never run or
    ///   the test output couldn't be parsed. Treated as "no evidence"
    ///   for objective satisfaction.
    /// - `Some(false)` — at least one test failed. Treated the same
    ///   way as "no evidence" for Table A-7 objectives; a
    ///   certification argument cannot rely on a red test run.
    /// - `Some(true)` — all recorded tests passed.
    pub tests_passed: Option<bool>,
    /// Whether structural coverage data exists
    pub has_coverage_data: bool,
}

/// Generate a compliance report for a single crate.
pub fn generate_compliance_report(
    crate_name: &str,
    dal: Dal,
    evidence: &CrateEvidence,
) -> ComplianceReport {
    let mut objectives = Vec::new();
    let mut met = 0u32;
    let mut not_met = 0u32;
    let mut partial = 0u32;
    let mut applicable_count = 0u32;

    for obj in OBJECTIVES {
        let app = obj.applicability_for(dal);
        let is_applicable = app != Applicability::NotApplicable;

        let (status, evidence_refs, note) = if !is_applicable {
            ("not_applicable".to_string(), vec![], None)
        } else {
            applicable_count += 1;
            determine_objective_status(obj, evidence)
        };

        match status.as_str() {
            "met" => met += 1,
            "not_met" => not_met += 1,
            "partial" => partial += 1,
            _ => {}
        }

        let applicability_detail = match app {
            Applicability::NotApplicable => "not_applicable",
            Applicability::Required => "required",
            Applicability::RequiredWithIndependence => "required_with_independence",
        };

        objectives.push(ObjectiveStatus {
            objective_id: obj.id.to_string(),
            table: obj.table.to_string(),
            title: obj.title.to_string(),
            applicable: is_applicable,
            applicability_detail: applicability_detail.to_string(),
            status,
            evidence_refs,
            note,
        });
    }

    ComplianceReport {
        crate_name: crate_name.to_string(),
        dal: dal.to_string(),
        schema_version: "0.0.1".to_string(),
        objectives,
        summary: ComplianceSummary {
            total_objectives: OBJECTIVES.len() as u32,
            applicable: applicable_count,
            met,
            not_met,
            partial,
        },
    }
}

/// Determine status for a single objective based on available evidence.
fn determine_objective_status(
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;

    #[test]
    fn test_objective_count() {
        // We encode Tables A-3 through A-7 (automatable subset)
        // A-3: 7, A-4: 8, A-5: 5, A-6: 5, A-7: 10 = 35
        assert_eq!(OBJECTIVES.len(), 35);
    }

    #[test]
    fn test_applicability_dal_a() {
        // DAL-A: all objectives should be applicable
        for obj in OBJECTIVES {
            let app = obj.applicability_for(Dal::A);
            assert_ne!(
                app,
                Applicability::NotApplicable,
                "objective {} should be applicable at DAL-A",
                obj.id
            );
        }
    }

    #[test]
    fn test_applicability_dal_d_relaxed() {
        // DAL-D: several objectives should be not applicable
        let na_count = OBJECTIVES
            .iter()
            .filter(|obj| obj.applicability_for(Dal::D) == Applicability::NotApplicable)
            .count();
        assert!(
            na_count > 0,
            "DAL-D should have some non-applicable objectives"
        );
    }

    #[test]
    fn test_mcdc_only_dal_a() {
        // A7-10 (MC/DC) is only required at DAL-A
        let mcdc = OBJECTIVES.iter().find(|o| o.id == "A7-10").unwrap();
        assert_eq!(
            mcdc.applicability_for(Dal::A),
            Applicability::RequiredWithIndependence
        );
        assert_eq!(mcdc.applicability_for(Dal::B), Applicability::NotApplicable);
        assert_eq!(mcdc.applicability_for(Dal::C), Applicability::NotApplicable);
        assert_eq!(mcdc.applicability_for(Dal::D), Applicability::NotApplicable);
    }

    #[test]
    fn test_compliance_report_dal_a_no_evidence() {
        let evidence = CrateEvidence::default();
        let report = generate_compliance_report("my-crate", Dal::A, &evidence);
        assert_eq!(report.crate_name, "my-crate");
        assert_eq!(report.dal, "A");
        assert_eq!(report.summary.total_objectives, 35);
        assert_eq!(report.summary.applicable, 35); // All applicable at DAL-A
        assert_eq!(report.summary.met, 0);
        assert!(report.summary.not_met > 0);
    }

    #[test]
    fn test_compliance_report_dal_d_with_tests() {
        let evidence = CrateEvidence {
            has_trace_data: true,
            trace_validation_passed: true,
            has_test_results: true,
            tests_passed: Some(true),
            has_coverage_data: false,
        };
        let report = generate_compliance_report("util-crate", Dal::D, &evidence);
        assert_eq!(report.dal, "D");
        // DAL-D has fewer applicable objectives
        assert!(report.summary.applicable < report.summary.total_objectives);
        // With passing tests and trace, some should be met or partial
        assert!(report.summary.met + report.summary.partial > 0);
    }

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

    #[test]
    fn test_compliance_report_serialization() {
        let evidence = CrateEvidence::default();
        let report = generate_compliance_report("test-crate", Dal::C, &evidence);
        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("\"crate_name\": \"test-crate\""));
        assert!(json.contains("\"dal\": \"C\""));
        // Should deserialize back
        let _: ComplianceReport = serde_json::from_str(&json).unwrap();
    }
}
