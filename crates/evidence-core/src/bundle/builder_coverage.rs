//! Coverage-facing extras on [`EvidenceBuilder`]. Split from
//! `builder.rs` to keep the parent under the 500-line workspace
//! limit. See LLR-053.

use super::builder::EvidenceBuilder;
use crate::{CoverageLevel, CoverageReport};

impl EvidenceBuilder {
    /// Store the coverage report from the generate coverage
    /// phase. Feeds A-7 Obj-5 / Obj-6 compliance evaluation via
    /// [`Self::coverage_statement_percent`] +
    /// [`Self::coverage_branch_percent`].
    pub fn set_coverage_report(&mut self, report: CoverageReport) {
        self.coverage_report_mut().replace(report);
    }

    /// Aggregate statement-coverage percentage from the stored
    /// report, or `None` if no report was captured / the report
    /// has no statement-level measurement.
    pub fn coverage_statement_percent(&self) -> Option<f64> {
        coverage_aggregate(self.coverage_report_ref(), CoverageLevel::Statement)
    }

    /// Aggregate branch-coverage percentage, same semantics as
    /// [`Self::coverage_statement_percent`].
    pub fn coverage_branch_percent(&self) -> Option<f64> {
        coverage_aggregate(self.coverage_report_ref(), CoverageLevel::Branch)
    }
}

fn coverage_aggregate(report: Option<&CoverageReport>, level: CoverageLevel) -> Option<f64> {
    let report = report?;
    let m = report.measurements.iter().find(|m| m.level == level)?;
    let total: u64 = m.per_file.iter().map(|f| f.lines.total).sum();
    if total == 0 {
        return Some(0.0);
    }
    let covered: u64 = m.per_file.iter().map(|f| f.lines.covered).sum();
    Some((covered as f64 / total as f64) * 100.0)
}
