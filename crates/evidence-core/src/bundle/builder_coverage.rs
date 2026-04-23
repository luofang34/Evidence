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
        let report = self.coverage_report_ref()?;
        let m = measurement_at_level(report, CoverageLevel::Statement)?;
        Some(aggregate_lines(m))
    }

    /// Aggregate branch-coverage percentage, same semantics as
    /// [`Self::coverage_statement_percent`]. Sums the structural
    /// `branches` field, not the line field — compliance A-7
    /// Obj-6 must reflect actual branch coverage, not a
    /// line-coverage proxy.
    pub fn coverage_branch_percent(&self) -> Option<f64> {
        let report = self.coverage_report_ref()?;
        let m = measurement_at_level(report, CoverageLevel::Branch)?;
        Some(aggregate_branches(m))
    }
}

fn measurement_at_level(
    report: &CoverageReport,
    level: CoverageLevel,
) -> Option<&crate::Measurement> {
    report.measurements.iter().find(|m| m.level == level)
}

fn aggregate_lines(m: &crate::Measurement) -> f64 {
    let total: u64 = m.per_file.iter().map(|f| f.lines.total).sum();
    if total == 0 {
        return 0.0;
    }
    let covered: u64 = m.per_file.iter().map(|f| f.lines.covered).sum();
    (covered as f64 / total as f64) * 100.0
}

fn aggregate_branches(m: &crate::Measurement) -> f64 {
    let total: u64 = m
        .per_file
        .iter()
        .map(|f| f.branches.as_ref().map_or(0, |b| b.total))
        .sum();
    if total == 0 {
        return 0.0;
    }
    let covered: u64 = m
        .per_file
        .iter()
        .map(|f| f.branches.as_ref().map_or(0, |b| b.covered))
        .sum();
    (covered as f64 / total as f64) * 100.0
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
    use crate::{BranchCoverage, FileMeasurement, LineCoverage, Measurement};

    fn mk_file(lines: (u64, u64), branches: Option<(u64, u64)>) -> FileMeasurement {
        FileMeasurement {
            path: "x.rs".into(),
            lines: LineCoverage {
                covered: lines.0,
                total: lines.1,
            },
            branches: branches.map(|(c, t)| BranchCoverage {
                covered: c,
                total: t,
            }),
            decisions: vec![],
            conditions: vec![],
        }
    }

    fn mk_m(level: CoverageLevel, files: Vec<FileMeasurement>) -> Measurement {
        Measurement {
            level,
            engine: "llvm-cov".into(),
            engine_version: "test".into(),
            per_file: files,
        }
    }

    /// Normal: `aggregate_lines` sums `.lines.*` regardless of
    /// what `.branches` carries. Lines path is the Statement-
    /// level compliance-report source.
    #[test]
    fn aggregate_lines_sums_line_field_only() {
        let m = mk_m(
            CoverageLevel::Statement,
            vec![mk_file((95, 100), Some((1, 10)))],
        );
        assert!((aggregate_lines(&m) - 95.0).abs() < 1e-9);
    }

    /// **The bug this PR fixes**, compliance-report side.
    /// `aggregate_branches` must read the `branches` field, not
    /// the `lines` field. Pre-fix, `coverage_aggregate` at
    /// `Branch` level read `lines` → A-7 Obj-6 (decision
    /// coverage) compliance status was computed from line
    /// coverage. Post-fix, the two paths are separate
    /// functions.
    #[test]
    fn aggregate_branches_reads_branches_not_lines() {
        let m = mk_m(
            CoverageLevel::Branch,
            vec![mk_file((95, 100), Some((5, 10)))],
        );
        assert!((aggregate_branches(&m) - 50.0).abs() < 1e-9);
    }

    /// Robustness: `coverage_branch_percent` on a Branch-level
    /// measurement where every file has `branches: None` → 0.0.
    /// No division-by-zero; no NaN leakage into the compliance
    /// JSON.
    #[test]
    fn aggregate_branches_robustness_all_none_returns_zero() {
        let m = mk_m(
            CoverageLevel::Branch,
            vec![mk_file((5, 10), None), mk_file((5, 10), None)],
        );
        assert_eq!(aggregate_branches(&m), 0.0);
    }

    /// BVA: zero-total file is a legitimate `Some(0, 0)` and
    /// does not crash the aggregator. Returns `0.0` as the
    /// aggregate percent.
    #[test]
    fn aggregate_branches_bva_zero_zero_single_file_returns_zero() {
        let m = mk_m(CoverageLevel::Branch, vec![mk_file((0, 0), Some((0, 0)))]);
        assert_eq!(aggregate_branches(&m), 0.0);
    }
}
