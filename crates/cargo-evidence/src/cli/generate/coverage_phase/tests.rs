//! Unit tests for `coverage_phase`. Sibling file via `#[path]` so
//! the parent stays under the 500-line workspace limit.
//!
//! Test structure follows DO-178C DAL-A/B verification
//! expectations per LLR: normal range + robustness + boundary
//! value analysis for every aggregator and the threshold
//! dispatcher.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use evidence_core::{
    BranchCoverage, CoverageReport, FileMeasurement, LineCoverage, Measurement, schema_versions,
};

use super::*;

fn mk_file_lines_only(lines: (u64, u64)) -> FileMeasurement {
    FileMeasurement {
        path: "x.rs".into(),
        lines: LineCoverage {
            covered: lines.0,
            total: lines.1,
        },
        branches: None,
        decisions: vec![],
        conditions: vec![],
    }
}

fn mk_file_with_branches(lines: (u64, u64), branches: Option<(u64, u64)>) -> FileMeasurement {
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

fn mk_measurement(level: CoverageLevel, per_file: Vec<FileMeasurement>) -> Measurement {
    Measurement {
        level,
        engine: "llvm-cov".into(),
        engine_version: "test".into(),
        per_file,
    }
}

fn mk_report(measurements: Vec<Measurement>) -> CoverageReport {
    CoverageReport {
        schema_version: schema_versions::COVERAGE.into(),
        measurements,
    }
}

// ---- Pre-existing: resolve_choice + levels_for_choice ----

#[test]
fn resolve_choice_explicit_wins_over_profile_default() {
    assert_eq!(
        resolve_choice(Some(CoverageChoice::None), Profile::Cert),
        CoverageChoice::None
    );
    assert_eq!(
        resolve_choice(Some(CoverageChoice::Branch), Profile::Dev),
        CoverageChoice::Branch
    );
}

#[test]
fn resolve_choice_defaults_by_profile_when_unset() {
    assert_eq!(resolve_choice(None, Profile::Dev), CoverageChoice::None);
    assert_eq!(resolve_choice(None, Profile::Cert), CoverageChoice::Branch);
    assert_eq!(
        resolve_choice(None, Profile::Record),
        CoverageChoice::Branch
    );
}

#[test]
fn levels_for_choice_maps_as_expected() {
    assert!(levels_for_choice(CoverageChoice::None).is_empty());
    assert_eq!(
        levels_for_choice(CoverageChoice::Line),
        vec![CoverageLevel::Statement]
    );
    assert_eq!(
        levels_for_choice(CoverageChoice::Branch),
        vec![CoverageLevel::Branch]
    );
    assert_eq!(
        levels_for_choice(CoverageChoice::All),
        vec![CoverageLevel::Statement, CoverageLevel::Branch]
    );
}

// ---- LLR-057: aggregate_lines_percent ----
//
// Normal range + robustness + BVA per DO-178C DAL-A/B
// verification expectations.

/// Normal: three files with known line counts sum to 60%.
#[test]
fn aggregate_lines_normal_three_files_weighted_average() {
    let m = mk_measurement(
        CoverageLevel::Statement,
        vec![
            mk_file_lines_only((10, 20)),
            mk_file_lines_only((30, 50)),
            mk_file_lines_only((20, 30)),
        ],
    );
    // (10+30+20) / (20+50+30) = 60/100 = 60.0
    assert!((aggregate_lines_percent(&m) - 60.0).abs() < 1e-9);
}

/// Robustness: empty per_file → 0.0, not NaN. Divide-by-zero
/// on u64 `total` would otherwise break the downstream `<`
/// comparison in `threshold_violations`.
#[test]
fn aggregate_lines_robustness_empty_per_file_returns_zero() {
    let m = mk_measurement(CoverageLevel::Statement, vec![]);
    assert_eq!(aggregate_lines_percent(&m), 0.0);
}

/// Robustness: single file with total=0 (no executable
/// lines) → 0.0 contribution.
#[test]
fn aggregate_lines_robustness_single_file_zero_total_returns_zero() {
    let m = mk_measurement(CoverageLevel::Statement, vec![mk_file_lines_only((0, 0))]);
    assert_eq!(aggregate_lines_percent(&m), 0.0);
}

/// BVA: smallest non-trivial fully-covered file → 100%.
#[test]
fn aggregate_lines_bva_one_over_one_is_hundred() {
    let m = mk_measurement(CoverageLevel::Statement, vec![mk_file_lines_only((1, 1))]);
    assert_eq!(aggregate_lines_percent(&m), 100.0);
}

/// BVA: smallest non-trivial uncovered file → 0%.
#[test]
fn aggregate_lines_bva_zero_over_one_is_zero() {
    let m = mk_measurement(CoverageLevel::Statement, vec![mk_file_lines_only((0, 1))]);
    assert_eq!(aggregate_lines_percent(&m), 0.0);
}

/// BVA: fractional value with integer-representable percent
/// roundtrips exactly.
#[test]
fn aggregate_lines_bva_forty_two_over_hundred_is_exact() {
    let m = mk_measurement(
        CoverageLevel::Statement,
        vec![mk_file_lines_only((42, 100))],
    );
    assert_eq!(aggregate_lines_percent(&m), 42.0);
}

// ---- LLR-058: aggregate_branches_percent ----
//
// The split from `aggregate_lines_percent` is the bugfix —
// branch threshold must read branch counts, never lines.

/// Normal: two files with known branch counts sum to 50%.
#[test]
fn aggregate_branches_normal_two_files_sum_to_half() {
    let m = mk_measurement(
        CoverageLevel::Branch,
        vec![
            mk_file_with_branches((0, 0), Some((20, 50))),
            mk_file_with_branches((0, 0), Some((30, 50))),
        ],
    );
    assert!((aggregate_branches_percent(&m) - 50.0).abs() < 1e-9);
}

/// Robustness: file with `branches: None` contributes 0/0 —
/// does not pull the aggregate toward zero.
#[test]
fn aggregate_branches_robustness_none_file_contributes_nothing() {
    let m = mk_measurement(
        CoverageLevel::Branch,
        vec![
            mk_file_with_branches((5, 10), Some((20, 50))),
            mk_file_with_branches((5, 10), None),
            mk_file_with_branches((5, 10), Some((30, 50))),
        ],
    );
    assert!((aggregate_branches_percent(&m) - 50.0).abs() < 1e-9);
}

/// Robustness: all files lack branch data → 0.0, not NaN.
#[test]
fn aggregate_branches_robustness_all_none_returns_zero() {
    let m = mk_measurement(
        CoverageLevel::Branch,
        vec![
            mk_file_with_branches((0, 0), None),
            mk_file_with_branches((0, 0), None),
        ],
    );
    assert_eq!(aggregate_branches_percent(&m), 0.0);
}

/// Robustness: empty per_file → 0.0.
#[test]
fn aggregate_branches_robustness_empty_per_file_returns_zero() {
    let m = mk_measurement(CoverageLevel::Branch, vec![]);
    assert_eq!(aggregate_branches_percent(&m), 0.0);
}

/// BVA: single file with `branches: Some(0,0)` → 0.0 (not
/// NaN). Pure straight-line code: the file contributes
/// structurally but has nothing to cover.
#[test]
fn aggregate_branches_bva_zero_zero_single_file_returns_zero() {
    let m = mk_measurement(
        CoverageLevel::Branch,
        vec![mk_file_with_branches((0, 0), Some((0, 0)))],
    );
    assert_eq!(aggregate_branches_percent(&m), 0.0);
}

/// BVA: all branches covered → 100.0 exactly, no off-by-one.
#[test]
fn aggregate_branches_bva_all_covered_is_hundred() {
    let m = mk_measurement(
        CoverageLevel::Branch,
        vec![mk_file_with_branches((0, 0), Some((4, 4)))],
    );
    assert_eq!(aggregate_branches_percent(&m), 100.0);
}

/// **The bug this PR fixes.** Pre-fix: branch threshold
/// aggregator summed `lines.*`, so a file with 95% lines /
/// 50% branches reported branch coverage as 95% — DAL-B
/// compliance gate passed spuriously. Post-fix: the branches
/// aggregator ignores line counts entirely.
#[test]
fn aggregate_branches_ignores_line_counts_at_branch_level() {
    let m = mk_measurement(
        CoverageLevel::Branch,
        vec![mk_file_with_branches((95, 100), Some((5, 10)))],
    );
    assert!((aggregate_branches_percent(&m) - 50.0).abs() < 1e-9);
}

// ---- LLR-059: threshold_violations dispatcher ----

/// Normal: statement 95% passes, branch 60% falls below
/// DAL-B's 85% minimum → one violation on branch only.
#[test]
fn threshold_normal_branch_below_dalb_statement_above() {
    let report = mk_report(vec![
        mk_measurement(
            CoverageLevel::Statement,
            vec![mk_file_lines_only((95, 100))],
        ),
        mk_measurement(
            CoverageLevel::Branch,
            vec![mk_file_with_branches((95, 100), Some((6, 10)))],
        ),
    ]);
    let thresholds = DalCoverageThresholds {
        statement_percent: Some(90),
        branch_percent: Some(85),
    };
    let v = threshold_violations(&report, thresholds);
    assert_eq!(v.len(), 1, "only branch should fire");
    assert_eq!(v[0].dimension, "branch");
    assert!((v[0].current_percent - 60.0).abs() < 1e-9);
    assert_eq!(v[0].threshold_percent, 85);
}

/// Robustness: threshold.branch_percent = None → branch
/// check doesn't run even if a Branch measurement exists
/// that would otherwise fail.
#[test]
fn threshold_robustness_none_threshold_skips_check() {
    let report = mk_report(vec![mk_measurement(
        CoverageLevel::Branch,
        vec![mk_file_with_branches((0, 0), Some((1, 100)))],
    )]);
    let thresholds = DalCoverageThresholds {
        statement_percent: None,
        branch_percent: None,
    };
    assert!(threshold_violations(&report, thresholds).is_empty());
}

/// Robustness: threshold set for a level, but no measurement
/// of that level → no violation. "Measurement absent" is a
/// config-layer issue, not a coverage-layer failure. Treating
/// absence as 0% would mask the true problem.
#[test]
fn threshold_robustness_measurement_absent_no_violation() {
    let report = mk_report(vec![mk_measurement(
        CoverageLevel::Statement,
        vec![mk_file_lines_only((95, 100))],
    )]);
    let thresholds = DalCoverageThresholds {
        statement_percent: Some(90),
        branch_percent: Some(85),
    };
    let v = threshold_violations(&report, thresholds);
    assert!(v.is_empty());
}

/// BVA: `current == threshold` passes. DO-178C A-7 minima
/// are stated inclusively ("shall be ≥ 85%"), so equality
/// is compliant. Strict `<` at the call-site.
#[test]
fn threshold_bva_exact_equality_passes() {
    let report = mk_report(vec![mk_measurement(
        CoverageLevel::Branch,
        vec![mk_file_with_branches((0, 0), Some((85, 100)))],
    )]);
    let thresholds = DalCoverageThresholds {
        statement_percent: None,
        branch_percent: Some(85),
    };
    assert!(
        threshold_violations(&report, thresholds).is_empty(),
        "85.0 == 85 must pass (strict <, not <=)"
    );
}

/// BVA: one full branch below threshold → violation.
#[test]
fn threshold_bva_just_below_fires() {
    let report = mk_report(vec![mk_measurement(
        CoverageLevel::Branch,
        vec![mk_file_with_branches((0, 0), Some((84, 100)))],
    )]);
    let thresholds = DalCoverageThresholds {
        statement_percent: None,
        branch_percent: Some(85),
    };
    let v = threshold_violations(&report, thresholds);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].dimension, "branch");
}

/// Integration: the bug scenario end-to-end. 95% lines /
/// 60% branches at DAL-B {stmt 90, branch 85}. Pre-fix: zero
/// violations (branch check read lines → 95% passed 85%).
/// Post-fix: exactly one branch violation. This test would
/// have caught the #82 bug in review.
#[test]
fn threshold_integration_high_lines_low_branches_fires_branch_only() {
    let report = mk_report(vec![
        mk_measurement(
            CoverageLevel::Statement,
            vec![mk_file_lines_only((95, 100))],
        ),
        mk_measurement(
            CoverageLevel::Branch,
            vec![mk_file_with_branches((95, 100), Some((6, 10)))],
        ),
    ]);
    let thresholds = DalCoverageThresholds {
        statement_percent: Some(90),
        branch_percent: Some(85),
    };
    let v = threshold_violations(&report, thresholds);
    assert_eq!(v.len(), 1, "branch alone fires; statement passes");
    assert_eq!(v[0].dimension, "branch");
}
