//! `coverage_verdict` — the A-7 Obj-5 / Obj-6 evaluator.
//! Carved out of `compliance/status.rs` to keep that file
//! under the workspace 500-line limit.

use super::report::ObjectiveStatusKind;

type Verdict = (ObjectiveStatusKind, Vec<String>, Option<String>);

/// Structural-coverage verdict for A-7 Obj-5 (statement) /
/// Obj-6 (branch). Upgrades to [`ObjectiveStatusKind::Met`] when
/// the aggregate percent meets the DAL-policy threshold;
/// otherwise `Partial` when data exists but falls short, or
/// `NotMet` when no report was produced.
pub(super) fn coverage_verdict(
    observed_percent: Option<f64>,
    threshold_percent: Option<u8>,
    dimension: &str,
) -> Verdict {
    let evidence_files = vec![
        "coverage/coverage_summary.json".to_string(),
        "coverage/lcov.info".to_string(),
    ];
    match (observed_percent, threshold_percent) {
        (Some(obs), Some(min)) if obs >= f64::from(min) => (
            ObjectiveStatusKind::Met,
            evidence_files,
            Some(format!("{dimension} coverage {obs:.2}% ≥ {min}% threshold")),
        ),
        (Some(obs), Some(min)) => (
            ObjectiveStatusKind::Partial,
            evidence_files,
            Some(format!(
                "{dimension} coverage {obs:.2}% below {min}% threshold"
            )),
        ),
        (Some(obs), None) => (
            // Report exists but this DAL has no threshold — Met
            // with an informational note. (Shouldn't normally
            // fire: if DAL has no threshold the objective should
            // be NotApplicable.)
            ObjectiveStatusKind::Met,
            evidence_files,
            Some(format!("{dimension} coverage {obs:.2}% (no DAL threshold)")),
        ),
        (None, _) => (
            ObjectiveStatusKind::NotMet,
            vec![],
            Some(format!("no {dimension}-coverage data in bundle")),
        ),
    }
}
