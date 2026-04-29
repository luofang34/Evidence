//! Coverage-threshold gating: per-DAL `(statement_min, branch_min)`
//! evaluated against a [`CoverageReport`]'s aggregate percentages.
//!
//! Lives in `evidence-core` (rather than the cargo-evidence binary
//! it's called from) so the same logic is available to integration
//! tests via `cargo test --workspace <pattern>` without needing
//! `--all-targets` or `--bins`. The CLI's `coverage_phase` thinly
//! wraps [`evaluate_thresholds`] and emits the resulting
//! [`CoverageThresholdViolation`]s as JSONL diagnostics.
//!
//! Bug-history note: the pre-0.1.1 `aggregate_branches_percent`
//! summed `lines.{covered,total}` even at the Branch measurement
//! level, so a project at 95% lines / 50% branches passed the
//! DAL-B `branch ≥ 85%` gate spuriously. The fix split the
//! aggregator into [`aggregate_lines_percent`] and
//! [`aggregate_branches_percent`]; the dispatcher in
//! [`evaluate_thresholds`] picks the right one per dimension.
//! Tests below pin both sides plus the BVA cases.

use crate::coverage::report::{CoverageLevel, CoverageReport, Measurement};
use crate::policy::DalCoverageThresholds;

/// One violation: a dimension's observed aggregate is below the
/// DAL's required minimum. CLI emits one `COVERAGE_BELOW_THRESHOLD`
/// diagnostic per entry.
#[derive(Debug, Clone, PartialEq)]
pub struct CoverageThresholdViolation {
    /// `"statement"` or `"branch"`. Static-string so JSONL agents
    /// pattern-match on a stable token.
    pub dimension: &'static str,
    /// Aggregate percentage the report observed (0.0–100.0).
    pub current_percent: f64,
    /// Minimum required by the DAL (1–100). `u8` matches the
    /// type carried by [`DalCoverageThresholds`].
    pub threshold_percent: u8,
}

/// Evaluate every dimension carried by `thresholds` against
/// `report` and return one [`CoverageThresholdViolation`] per
/// dimension that falls short. `Some` thresholds with a missing
/// matching measurement (`measurements[]` lacks a Statement or
/// Branch level) are silently skipped — that's a generate-time
/// invariant the caller has already enforced.
///
/// Comparison is strict `<`, not `<=`. DO-178C A-7 minima are
/// stated inclusively ("statement coverage shall be ≥ 90% at
/// DAL-C"), so equality is compliant. The corresponding
/// `threshold_bva_exact_equality_passes` test pins this.
pub fn evaluate_thresholds(
    report: &CoverageReport,
    thresholds: DalCoverageThresholds,
) -> Vec<CoverageThresholdViolation> {
    let mut violations = Vec::new();
    if let Some(min) = thresholds.statement_percent
        && let Some(m) = measurement_for(report, CoverageLevel::Statement)
    {
        let pct = aggregate_lines_percent(m);
        if pct < f64::from(min) {
            violations.push(CoverageThresholdViolation {
                dimension: "statement",
                current_percent: pct,
                threshold_percent: min,
            });
        }
    }
    if let Some(min) = thresholds.branch_percent
        && let Some(m) = measurement_for(report, CoverageLevel::Branch)
    {
        let pct = aggregate_branches_percent(m);
        if pct < f64::from(min) {
            violations.push(CoverageThresholdViolation {
                dimension: "branch",
                current_percent: pct,
                threshold_percent: min,
            });
        }
    }
    violations
}

fn measurement_for(report: &CoverageReport, level: CoverageLevel) -> Option<&Measurement> {
    report.measurements.iter().find(|m| m.level == level)
}

/// Aggregate percentage for `CoverageLevel::Statement`. Sums
/// per-file line counts. Zero denominator (no executable lines
/// across the measurement) returns `0.0`, never NaN —
/// divide-by-zero on a u64 total would otherwise break f64
/// comparison at every downstream call-site.
pub fn aggregate_lines_percent(m: &Measurement) -> f64 {
    let total: u64 = m.per_file.iter().map(|f| f.lines.total).sum();
    if total == 0 {
        return 0.0;
    }
    let covered: u64 = m.per_file.iter().map(|f| f.lines.covered).sum();
    (covered as f64 / total as f64) * 100.0
}

/// Aggregate percentage for `CoverageLevel::Branch`. Sums
/// per-file `branches.{covered,total}`; files whose `branches`
/// field is `None` contribute `0/0` (no contribution). Zero
/// denominator → `0.0` (no NaN). Separation from
/// [`aggregate_lines_percent`] is load-bearing: summing
/// `lines.*` at branch level was the pre-fix bug — tool
/// reported branch thresholds as met using line-coverage data.
pub fn aggregate_branches_percent(m: &Measurement) -> f64 {
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
#[path = "thresholds/tests.rs"]
mod tests;
