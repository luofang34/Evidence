//! N:M selector aggregation for `build_test_diag`.
//!
//! When a TEST entry carries more than one selector through
//! `test_selectors: Vec<String>` and `test_selector: Option<String>`,
//! the pass/fail/skip aggregation rule is strict — the TEST passes
//! iff every selector matches a run function and every matched
//! function passed.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::bundle::TestOutcome;
use crate::diagnostic::{Diagnostic, FixHint, Location};

use super::super::view::ReportTest;
use super::super::{RequirementStatus, TestStatus};
use super::{ends_with_fn, make_diag};

/// Aggregate outcome across every selector in the TEST entry.
/// Caller guarantees `selectors.len() >= 2`; a single selector uses
/// the dedicated diagnostic path.
pub(super) fn status(
    t: &ReportTest,
    uid: &str,
    selectors: &[String],
    outcomes: &BTreeMap<String, TestOutcome>,
) -> (TestStatus, Diagnostic) {
    let summary = collect_summary(selectors, outcomes);
    if !summary.failed.is_empty() {
        return failed_status(t, uid, &summary.failed);
    }
    if !summary.ambiguous.is_empty() {
        return ambiguous_status(t, uid, &summary.ambiguous);
    }
    if !summary.unmatched.is_empty() {
        return unmatched_status(t, uid, &summary.unmatched);
    }
    if !summary.ignored.is_empty() {
        return ignored_status(t, uid, &summary.ignored);
    }
    passed_status(t, uid, selectors.len())
}

#[derive(Default)]
struct SelectorSummary {
    unmatched: Vec<String>,
    failed: Vec<String>,
    ignored: Vec<String>,
    ambiguous: Vec<String>,
}

fn collect_summary(
    selectors: &[String],
    outcomes: &BTreeMap<String, TestOutcome>,
) -> SelectorSummary {
    let mut summary = SelectorSummary::default();
    for sel in selectors {
        let matches: Vec<&String> = outcomes
            .keys()
            .filter(|k| k.as_str() == sel.as_str() || ends_with_fn(k, sel))
            .collect();
        match matches.as_slice() {
            [] => summary.unmatched.push(sel.clone()),
            [only] => match outcomes[*only] {
                TestOutcome::Passed => {}
                TestOutcome::Failed => summary.failed.push(sel.clone()),
                TestOutcome::Ignored => summary.ignored.push(sel.clone()),
            },
            many => summary.ambiguous.push(format!(
                "{} matches [{}]",
                sel,
                many.iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
    summary
}

fn failed_status(t: &ReportTest, uid: &str, failed: &[String]) -> (TestStatus, Diagnostic) {
    (
        TestStatus {
            status: RequirementStatus::Gap,
            root_cause_uid: Some(uid.to_string()),
        },
        make_diag(
            RequirementStatus::Gap,
            format!(
                "TEST {} failed in this run (selectors failed: [{}])",
                t.id,
                failed.join(", ")
            ),
            Some(entry_location(uid)),
            None,
            None,
        ),
    )
}

fn ambiguous_status(t: &ReportTest, uid: &str, ambiguous: &[String]) -> (TestStatus, Diagnostic) {
    let fix = FixHint::AddTomlKey {
        path: PathBuf::from("tests.toml"),
        toml_path: format!("tests[id={}]", t.id),
        key: "test_selectors".into(),
        value_stub: "<fully-qualified selectors>".into(),
    };
    (
        TestStatus {
            status: RequirementStatus::Gap,
            root_cause_uid: Some(uid.to_string()),
        },
        make_diag(
            RequirementStatus::Gap,
            format!(
                "TEST {} has ambiguous selector(s): {}",
                t.id,
                ambiguous.join("; ")
            ),
            Some(file_location(uid)),
            Some(fix),
            None,
        ),
    )
}

fn unmatched_status(t: &ReportTest, uid: &str, unmatched: &[String]) -> (TestStatus, Diagnostic) {
    (
        TestStatus {
            status: RequirementStatus::Gap,
            root_cause_uid: Some(uid.to_string()),
        },
        make_diag(
            RequirementStatus::Gap,
            format!(
                "TEST {}: selector(s) did not run in this session (not in cargo test output): [{}]",
                t.id,
                unmatched.join(", ")
            ),
            Some(file_location(uid)),
            None,
            None,
        ),
    )
}

fn ignored_status(t: &ReportTest, uid: &str, ignored: &[String]) -> (TestStatus, Diagnostic) {
    (
        TestStatus {
            status: RequirementStatus::Skip,
            root_cause_uid: None,
        },
        make_diag(
            RequirementStatus::Skip,
            format!(
                "TEST {} skipped — some selectors are #[ignore]'d: [{}]",
                t.id,
                ignored.join(", ")
            ),
            Some(entry_location(uid)),
            None,
            None,
        ),
    )
}

fn passed_status(t: &ReportTest, uid: &str, count: usize) -> (TestStatus, Diagnostic) {
    (
        TestStatus {
            status: RequirementStatus::Pass,
            root_cause_uid: None,
        },
        make_diag(
            RequirementStatus::Pass,
            format!(
                "TEST {} passed ({} selectors resolved and passed)",
                t.id, count
            ),
            Some(entry_location(uid)),
            None,
            None,
        ),
    )
}

fn entry_location(uid: &str) -> Location {
    Location {
        entry_uid: Some(uid.to_string()),
        ..Location::default()
    }
}

fn file_location(uid: &str) -> Location {
    Location {
        file: Some(PathBuf::from("tests.toml")),
        entry_uid: Some(uid.to_string()),
        ..Location::default()
    }
}
