//! `coverage_verdict` — the A-7 Obj-5 / Obj-6 evaluator (LLR-108).
//! Carved out of `compliance/status.rs` to keep that file
//! under the workspace 500-line limit.
//!
//! Truth table (HLR-088): a coverage percentage is an engineering
//! metric, never a compliance verdict. `Met` requires the metric to
//! meet the engineering gate AND a recorded analysis/disposition of
//! uncovered structure — and only for statement coverage. LLVM
//! branch coverage approximates decision coverage, so it caps at
//! `ManualReviewRequired` even with a disposition: approximate
//! evidence cannot close the semantic objective.

use super::report::ObjectiveStatusKind;

type Verdict = (ObjectiveStatusKind, Vec<String>, Option<String>);

/// Coverage dimension under evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CoverageDimension {
    /// Statement coverage (A-7 Obj-5).
    Statement,
    /// LLVM branch coverage — an approximation of DO-178C decision
    /// coverage (A-7 Obj-6). The approximation can never close the
    /// semantic objective on its own.
    Branch,
}

impl CoverageDimension {
    fn as_str(self) -> &'static str {
        match self {
            CoverageDimension::Statement => "statement",
            CoverageDimension::Branch => "branch",
        }
    }
}

/// Structural-coverage verdict for A-7 Obj-5 (statement) /
/// Obj-6 (branch approximation):
///
/// - no coverage data → `NotMet`;
/// - observed below the engineering gate → `Partial` (uncovered
///   items exist; the note requires analysis/disposition);
/// - observed at/above the gate, no disposition →
///   `ManualReviewRequired` (a percentage alone cannot close the
///   objective);
/// - observed at/above the gate with a recorded disposition →
///   `Met` for `Statement` only; `Branch` stays
///   `ManualReviewRequired` (approximation cap);
/// - observed with no gate applying at this level →
///   `ManualReviewRequired`.
pub(super) fn coverage_verdict(
    observed_percent: Option<f64>,
    threshold_percent: Option<u8>,
    dimension: CoverageDimension,
    disposition: Option<&str>,
) -> Verdict {
    let evidence_files = vec![
        "coverage/coverage_summary.json".to_string(),
        "coverage/lcov.info".to_string(),
    ];
    let dim = dimension.as_str();
    match (observed_percent, threshold_percent) {
        (None, _) => (
            ObjectiveStatusKind::NotMet,
            vec![],
            Some(format!("no {dim}-coverage data in bundle")),
        ),
        (Some(obs), Some(min)) if obs < f64::from(min) => (
            ObjectiveStatusKind::Partial,
            evidence_files,
            Some(format!(
                "{dim} coverage {obs:.2}% below {min}% engineering gate; uncovered \
                 structure requires documented analysis/disposition before the \
                 objective can close"
            )),
        ),
        (Some(obs), Some(min)) => match (dimension, disposition) {
            (CoverageDimension::Statement, Some(_)) => (
                ObjectiveStatusKind::Met,
                evidence_files,
                Some(format!(
                    "{dim} coverage {obs:.2}% ≥ {min}% engineering gate; \
                     uncovered-structure analysis/disposition recorded"
                )),
            ),
            (CoverageDimension::Statement, None) => (
                ObjectiveStatusKind::ManualReviewRequired,
                evidence_files,
                Some(format!(
                    "{dim} coverage {obs:.2}% ≥ {min}% engineering gate; the \
                     percentage is an engineering metric — the objective closes \
                     only with documented analysis/disposition of uncovered \
                     structure and tool qualification evidence"
                )),
            ),
            (CoverageDimension::Branch, disp) => (
                ObjectiveStatusKind::ManualReviewRequired,
                evidence_files,
                Some(format!(
                    "LLVM branch coverage {obs:.2}% ≥ {min}% engineering gate \
                     approximates decision coverage; approximate evidence cannot \
                     close the objective{} — independent review of the \
                     approximation plus tool qualification evidence required",
                    if disp.is_some() {
                        " (disposition recorded)"
                    } else {
                        ""
                    }
                )),
            ),
        },
        (Some(obs), None) => (
            ObjectiveStatusKind::ManualReviewRequired,
            evidence_files,
            Some(format!(
                "{dim} coverage {obs:.2}% (no engineering gate applies at this \
                 level); a percentage alone cannot close the objective — \
                 analysis/disposition of uncovered structure and tool \
                 qualification review required"
            )),
        ),
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

    /// Direct truth-table pins (LLR-108). The objectives table
    /// makes the no-gate arm unreachable from
    /// `generate_compliance_report` (every applicable row carries
    /// a gate), so it is exercised here.
    #[test]
    fn truth_table_full_sweep() {
        // No data → NotMet.
        let (s, _, _) = coverage_verdict(None, Some(90), CoverageDimension::Statement, None);
        assert_eq!(s, ObjectiveStatusKind::NotMet);
        // Below gate → Partial.
        let (s, _, _) = coverage_verdict(Some(89.0), Some(90), CoverageDimension::Statement, None);
        assert_eq!(s, ObjectiveStatusKind::Partial);
        // At gate, no disposition → ManualReviewRequired.
        let (s, _, _) = coverage_verdict(Some(90.0), Some(90), CoverageDimension::Statement, None);
        assert_eq!(s, ObjectiveStatusKind::ManualReviewRequired);
        // At gate + disposition → Met for statement only.
        let (s, _, _) = coverage_verdict(
            Some(90.0),
            Some(90),
            CoverageDimension::Statement,
            Some("record"),
        );
        assert_eq!(s, ObjectiveStatusKind::Met);
        // Branch caps at ManualReviewRequired even with disposition.
        let (s, _, note) = coverage_verdict(
            Some(95.0),
            Some(85),
            CoverageDimension::Branch,
            Some("record"),
        );
        assert_eq!(s, ObjectiveStatusKind::ManualReviewRequired);
        assert!(note.unwrap().contains("approximat"));
        // No gate at this level → ManualReviewRequired, never Met.
        for dim in [CoverageDimension::Statement, CoverageDimension::Branch] {
            let (s, _, note) = coverage_verdict(Some(100.0), None, dim, Some("record"));
            assert_eq!(s, ObjectiveStatusKind::ManualReviewRequired);
            assert!(
                note.unwrap().contains("no engineering gate"),
                "no-gate note must name the absent gate"
            );
        }
    }
}
