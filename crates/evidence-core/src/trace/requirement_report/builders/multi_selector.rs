//! N:M selector aggregation for `build_test_diag`.
//!
//! When a TEST entry carries more than one selector (//! additive widening: `test_selectors: Vec<String>` alongside the
//! legacy `test_selector: Option<String>`), the pass/fail/skip
//! aggregation rule is strict — the TEST passes iff every selector
//! matches a run fn AND every matched fn passed. Split into this
//! sibling file to keep `builders.rs` under the 500-line workspace
//! file-size limit.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::bundle::TestOutcome;
use crate::diagnostic::{Diagnostic, FixHint, Location};
use crate::trace::entries::TestEntry;

use super::super::{RequirementStatus, TestStatus};
use super::{ends_with_fn, make_diag};

/// Aggregate outcome across every selector in the TEST entry.
/// Caller guarantees `selectors.len() >= 2` (the 1:1 path is
/// handled inline in `build_test_diag` for message-compatibility
/// with-era tests).
pub(super) fn status(
    t: &TestEntry,
    uid: &str,
    selectors: &[String],
    outcomes: &BTreeMap<String, TestOutcome>,
) -> (TestStatus, Diagnostic) {
    let mut unmatched: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    let mut ignored: Vec<String> = Vec::new();
    let mut ambiguous: Vec<String> = Vec::new();

    for sel in selectors {
        let matches: Vec<&String> = outcomes
            .keys()
            .filter(|k| k.as_str() == sel.as_str() || ends_with_fn(k, sel))
            .collect();
        match matches.as_slice() {
            [] => unmatched.push(sel.clone()),
            [only] => match outcomes[*only] {
                TestOutcome::Passed => {}
                TestOutcome::Failed => failed.push(sel.clone()),
                TestOutcome::Ignored => ignored.push(sel.clone()),
            },
            many => ambiguous.push(format!(
                "{} matches [{}]",
                sel,
                many.iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    if !failed.is_empty() {
        return (
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
                Some(Location {
                    entry_uid: Some(uid.to_string()),
                    ..Location::default()
                }),
                None,
                None,
            ),
        );
    }
    if !ambiguous.is_empty() {
        return (
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
                Some(Location {
                    file: Some(PathBuf::from("tests.toml")),
                    entry_uid: Some(uid.to_string()),
                    ..Location::default()
                }),
                Some(FixHint::AddTomlKey {
                    path: PathBuf::from("tests.toml"),
                    toml_path: format!("tests[id={}]", t.id),
                    key: "test_selectors".into(),
                    value_stub: "<fully-qualified selectors>".into(),
                }),
                None,
            ),
        );
    }
    if !unmatched.is_empty() {
        return (
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
                Some(Location {
                    file: Some(PathBuf::from("tests.toml")),
                    entry_uid: Some(uid.to_string()),
                    ..Location::default()
                }),
                None,
                None,
            ),
        );
    }
    if !ignored.is_empty() {
        return (
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
                Some(Location {
                    entry_uid: Some(uid.to_string()),
                    ..Location::default()
                }),
                None,
                None,
            ),
        );
    }
    // All selectors matched and all passed.
    (
        TestStatus {
            status: RequirementStatus::Pass,
            root_cause_uid: None,
        },
        make_diag(
            RequirementStatus::Pass,
            format!(
                "TEST {} passed ({} selectors resolved and passed)",
                t.id,
                selectors.len()
            ),
            Some(Location {
                entry_uid: Some(uid.to_string()),
                ..Location::default()
            }),
            None,
            None,
        ),
    )
}
