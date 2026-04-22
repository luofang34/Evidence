//! Compliance report types: per-objective status, summary, per-crate report, crate evidence.

use serde::{Deserialize, Serialize};

/// Verdict for a single DO-178C objective.
///
/// Distinct variants for `NotMet` vs `ManualReviewRequired` matter on
/// an audit surface: `NotMet` means the tool asserts the objective is
/// *not* satisfied (evidence against it, or a negative capability
/// gap like "tests failed"); `ManualReviewRequired` means the tool
/// *cannot judge* and a human reviewer must look. Conflating the two
/// — as the previous string-typed status did — inflated the `not_met`
/// counter with ~20 objectives that were actually "needs review" and
/// hid the real "tool says objective fails" cases.
///
/// Serialized as snake_case strings so the on-disk JSON stays
/// ergonomic for reviewers: `"met"`, `"not_met"`, `"partial"`,
/// `"not_applicable"`, `"manual_review_required"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveStatusKind {
    /// Evidence fully satisfies the objective.
    Met,
    /// The tool asserts the objective is not satisfied — evidence is
    /// negative (e.g. tests failed) or a required capability (e.g.
    /// MC/DC analysis) is explicitly unsupported. Reviewer should
    /// treat this as a red flag.
    NotMet,
    /// Evidence partially supports the objective but completion
    /// requires additional data or a mapping step the tool can't
    /// generate automatically (e.g. aggregate tests exist but no
    /// per-requirement mapping).
    Partial,
    /// Not required at this crate's DAL — carries no evidence and is
    /// excluded from every summary denominator.
    NotApplicable,
    /// The tool has no automated way to judge satisfaction; a human
    /// reviewer must inspect source / design docs / review records.
    /// Most Table A-3 / A-4 / A-5 / A-6 objectives land here by
    /// design — the tool can check traceability links but cannot
    /// certify "the HLRs are complete" or "the architecture is
    /// consistent".
    ManualReviewRequired,
}

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
    /// Objective verdict.
    pub status: ObjectiveStatusKind,
    /// Evidence reference(s) (file paths within the bundle)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
    /// Human-readable note
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Summary counts for a compliance report.
///
/// Every applicable objective lands in exactly one of
/// {met, not_met, partial, manual_review_required}, so:
/// `applicable == met + not_met + partial + manual_review_required`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceSummary {
    /// Total count of DO-178C objectives considered (always equals
    /// `OBJECTIVES.len()`).
    pub total_objectives: u32,
    /// Count of objectives applicable at this crate's DAL.
    pub applicable: u32,
    /// Count of applicable objectives whose status is `Met`.
    pub met: u32,
    /// Count of applicable objectives whose status is `NotMet`.
    pub not_met: u32,
    /// Count of applicable objectives whose status is `Partial`.
    pub partial: u32,
    /// Count of applicable objectives whose status is
    /// `ManualReviewRequired` — audit / review load that remains for
    /// humans after the tool has finished.
    pub manual_review_required: u32,
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
    /// Whether the bundle contains `tests/test_outcomes.jsonl`
    /// with per-test atoms (name, status, failure_message). The
    /// presence of this artifact — not just aggregate pass/fail
    /// — is what upgrades DO-178C A-7 Obj-3 / Obj-4 from
    /// `Partial` to `Met`: an auditor asking "show me the
    /// result of TEST-046" can resolve to a specific pass/fail
    /// row instead of the workspace-level boolean. `false` when
    /// the bundle doesn't carry the artifact (older bundles that
    /// predate the per-test capture, dev-profile runs that skip
    /// tests, libtest output that couldn't be parsed).
    pub has_per_test_outcomes: bool,
    /// Aggregate statement-coverage percentage from the
    /// `coverage/coverage_summary.json` artifact (when present).
    /// `None` means coverage was not captured on this run; some
    /// value means the report is available for the A-7 Obj-5
    /// evaluator to compare against the DAL threshold.
    pub coverage_statement_percent: Option<f64>,
    /// Aggregate branch-coverage percentage. Same semantics as
    /// [`Self::coverage_statement_percent`] for A-7 Obj-6.
    pub coverage_branch_percent: Option<f64>,
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

    /// Pin the wire format: each variant must serialize to the
    /// exact snake_case string documented in the struct-level rustdoc.
    /// A breaking rename here is a wire-format break and needs a
    /// COMPLIANCE schema version bump + golden fixture regen.
    #[test]
    fn objective_status_kind_serializes_to_expected_strings() {
        let cases = [
            (ObjectiveStatusKind::Met, r#""met""#),
            (ObjectiveStatusKind::NotMet, r#""not_met""#),
            (ObjectiveStatusKind::Partial, r#""partial""#),
            (ObjectiveStatusKind::NotApplicable, r#""not_applicable""#),
            (
                ObjectiveStatusKind::ManualReviewRequired,
                r#""manual_review_required""#,
            ),
        ];
        for (variant, expected) in cases {
            let got = serde_json::to_string(&variant).unwrap();
            assert_eq!(got, expected, "serialization drift for {:?}", variant);
            let round: ObjectiveStatusKind = serde_json::from_str(&got).unwrap();
            assert_eq!(round, variant, "round-trip drift for {:?}", variant);
        }
    }
}
