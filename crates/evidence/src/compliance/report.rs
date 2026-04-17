//! Compliance report types: per-objective status, summary, per-crate report, crate evidence.

use serde::{Deserialize, Serialize};

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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::super::generator::generate_compliance_report;
    use super::*;
    use crate::policy::Dal;

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
