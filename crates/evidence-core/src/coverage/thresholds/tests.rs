//! Unit tests for `evidence_core::coverage::thresholds`. Lives in a
//! sibling file pulled in via `#[path]` so the parent stays under
//! the workspace 500-line limit.
//!
//! Test structure follows DO-178C DAL-A/B verification expectations
//! per LLR: normal range + robustness + boundary value analysis for
//! every aggregator and the threshold dispatcher.
//!
//! These tests live in `evidence-core` (not `cargo-evidence`'s
//! binary unit-test target) so a reviewer can run them via
//! `cargo test --workspace <pattern>` without `--all-targets` or
//! `--bins` — the names below are stable execution entry points.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use super::*;
use crate::coverage::report::{
    BranchCoverage, CoverageLevel, FileMeasurement, LineCoverage, Measurement,
};
use crate::schema_versions;

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

// ---- LLR-057: aggregate_lines_percent ----

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
    assert!((aggregate_lines_percent(&m) - 60.0).abs() < 1e-9);
}

#[test]
fn aggregate_lines_robustness_empty_per_file_returns_zero() {
    let m = mk_measurement(CoverageLevel::Statement, vec![]);
    assert_eq!(aggregate_lines_percent(&m), 0.0);
}

#[test]
fn aggregate_lines_robustness_single_file_zero_total_returns_zero() {
    let m = mk_measurement(CoverageLevel::Statement, vec![mk_file_lines_only((0, 0))]);
    assert_eq!(aggregate_lines_percent(&m), 0.0);
}

#[test]
fn aggregate_lines_bva_one_over_one_is_hundred() {
    let m = mk_measurement(CoverageLevel::Statement, vec![mk_file_lines_only((1, 1))]);
    assert_eq!(aggregate_lines_percent(&m), 100.0);
}

#[test]
fn aggregate_lines_bva_zero_over_one_is_zero() {
    let m = mk_measurement(CoverageLevel::Statement, vec![mk_file_lines_only((0, 1))]);
    assert_eq!(aggregate_lines_percent(&m), 0.0);
}

#[test]
fn aggregate_lines_bva_forty_two_over_hundred_is_exact() {
    let m = mk_measurement(
        CoverageLevel::Statement,
        vec![mk_file_lines_only((42, 100))],
    );
    assert_eq!(aggregate_lines_percent(&m), 42.0);
}

// ---- LLR-058: aggregate_branches_percent ----

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

#[test]
fn aggregate_branches_robustness_empty_per_file_returns_zero() {
    let m = mk_measurement(CoverageLevel::Branch, vec![]);
    assert_eq!(aggregate_branches_percent(&m), 0.0);
}

#[test]
fn aggregate_branches_bva_zero_zero_single_file_returns_zero() {
    let m = mk_measurement(
        CoverageLevel::Branch,
        vec![mk_file_with_branches((0, 0), Some((0, 0)))],
    );
    assert_eq!(aggregate_branches_percent(&m), 0.0);
}

#[test]
fn aggregate_branches_bva_all_covered_is_hundred() {
    let m = mk_measurement(
        CoverageLevel::Branch,
        vec![mk_file_with_branches((0, 0), Some((4, 4)))],
    );
    assert_eq!(aggregate_branches_percent(&m), 100.0);
}

/// **The bug 0.1.1's #82 fixed.** Pre-fix: the branch-threshold
/// aggregator summed `lines.*`, so a file at 95% lines / 50%
/// branches reported branch coverage as 95% — DAL-B compliance
/// gate passed spuriously. Post-fix: the branch aggregator
/// ignores line counts entirely.
#[test]
fn aggregate_branches_ignores_line_counts_at_branch_level() {
    let m = mk_measurement(
        CoverageLevel::Branch,
        vec![mk_file_with_branches((95, 100), Some((5, 10)))],
    );
    assert!((aggregate_branches_percent(&m) - 50.0).abs() < 1e-9);
}

// ---- LLR-059: evaluate_thresholds dispatcher ----

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
    let v = evaluate_thresholds(&report, thresholds);
    assert_eq!(v.len(), 1, "only branch should fire");
    assert_eq!(v[0].dimension, "branch");
    assert!((v[0].current_percent - 60.0).abs() < 1e-9);
    assert_eq!(v[0].threshold_percent, 85);
}

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
    assert!(evaluate_thresholds(&report, thresholds).is_empty());
}

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
    let v = evaluate_thresholds(&report, thresholds);
    assert!(v.is_empty());
}

/// BVA: `current == threshold` passes. DO-178C A-7 minima are
/// stated inclusively ("shall be ≥ 85%"), so equality is
/// compliant. Strict `<` at the call-site.
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
        evaluate_thresholds(&report, thresholds).is_empty(),
        "85.0 == 85 must pass (strict <, not <=)"
    );
}

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
    let v = evaluate_thresholds(&report, thresholds);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].dimension, "branch");
}

/// Integration regression for the original 0.1.1 #82 bug.
/// 95% lines / 60% branches at DAL-B {stmt 90, branch 85}.
/// Pre-fix: zero violations (branch check read lines → 95%
/// passed 85%). Post-fix: exactly one branch violation.
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
    let v = evaluate_thresholds(&report, thresholds);
    assert_eq!(v.len(), 1, "branch alone fires; statement passes");
    assert_eq!(v[0].dimension, "branch");
}
