//! `generate_compliance_report` — the top-level entry point that
//! walks the objectives table for a given crate/DAL pair and collates
//! per-objective verdicts into a `ComplianceReport`.

use crate::policy::Dal;

use super::applicability::Applicability;
use super::objectives_table::OBJECTIVES;
use super::report::{
    ComplianceReport, ComplianceSummary, CrateEvidence, ObjectiveStatus, ObjectiveStatusKind,
};
use super::status::determine_objective_status;

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
    let mut manual_review_required = 0u32;
    let mut applicable_count = 0u32;

    for obj in OBJECTIVES {
        let app = obj.applicability_for(dal);
        let is_applicable = app != Applicability::NotApplicable;

        let (status, evidence_refs, note) = if !is_applicable {
            (ObjectiveStatusKind::NotApplicable, vec![], None)
        } else {
            applicable_count += 1;
            determine_objective_status(obj, evidence)
        };

        // Exhaustive match — a new variant becomes a compile error
        // here rather than silently falling into a `_ => {}` bucket
        // and under-counting the summary.
        match status {
            ObjectiveStatusKind::Met => met += 1,
            ObjectiveStatusKind::NotMet => not_met += 1,
            ObjectiveStatusKind::Partial => partial += 1,
            ObjectiveStatusKind::ManualReviewRequired => manual_review_required += 1,
            ObjectiveStatusKind::NotApplicable => {}
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
        schema_version: crate::schema_versions::COMPLIANCE.to_string(),
        objectives,
        summary: ComplianceSummary {
            total_objectives: OBJECTIVES.len() as u32,
            applicable: applicable_count,
            met,
            not_met,
            partial,
            manual_review_required,
        },
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
    use super::*;

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
}
