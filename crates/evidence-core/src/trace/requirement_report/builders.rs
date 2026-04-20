//! Diagnostic builders for `requirement_report`.
//!
//! Every item here is `pub(super)` — the module is an implementation
//! detail of [`super::build_requirement_report`]. Sibling files:
//!
//! - [`multi_selector`] — N:M selector aggregation for TEST entries
//!   carrying `test_selectors: Vec<String>`.
//! - [`cascade`] — HLR / LLR / SYS aggregation; rolls up child
//!   TEST statuses into a single parent `Diagnostic`.
//!
//! [`multi_selector`]: multi_selector
//! [`cascade`]: cascade

mod cascade;
mod multi_selector;

pub(super) use cascade::{CascadeEntry, aggregate_child_status, build_cascade_diag};

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::bundle::TestOutcome;
use crate::diagnostic::{Diagnostic, DiagnosticCode, FixHint, Location};
use crate::trace::entries::{HlrEntry, LlrEntry, TestEntry};

use super::{RequirementStatus, TestStatus};

pub(super) fn build_test_diag(
    t: &TestEntry,
    outcomes: &BTreeMap<String, TestOutcome>,
    unresolved_ids: &std::collections::BTreeSet<String>,
) -> (TestStatus, Diagnostic) {
    // Missing uid — emit GAP with an AssignUuid FixHint.
    if t.uid.is_none() {
        return (
            TestStatus {
                status: RequirementStatus::Gap,
                root_cause_uid: None,
            },
            make_diag(
                RequirementStatus::Gap,
                format!("TEST {} is missing `uid`", t.id),
                Some(Location {
                    file: Some(PathBuf::from("tests.toml")),
                    ..Location::default()
                }),
                Some(FixHint::AssignUuid {
                    path: PathBuf::from("tests.toml"),
                    toml_path: format!("tests[id={}]", t.id),
                }),
                None,
            ),
        );
    }
    let uid = t.uid.as_deref().unwrap_or("?");

    // Unresolvable selector — flagged by `resolve_test_selectors`.
    if unresolved_ids.contains(&t.id) {
        let selector = t.all_selectors().join(", ");
        return (
            TestStatus {
                status: RequirementStatus::Gap,
                root_cause_uid: Some(uid.to_string()),
            },
            make_diag(
                RequirementStatus::Gap,
                format!(
                    "TEST {} selector(s) [{}] did not resolve to a real #[test] fn",
                    t.id, selector
                ),
                Some(Location {
                    file: Some(PathBuf::from("tests.toml")),
                    entry_uid: Some(uid.to_string()),
                    ..Location::default()
                }),
                Some(FixHint::AddTomlKey {
                    path: PathBuf::from("tests.toml"),
                    toml_path: format!("tests[id={}]", t.id),
                    key: "test_selector".into(),
                    value_stub: format!(
                        "<fully-qualified selector; current [{}] did not resolve>",
                        selector
                    ),
                }),
                None,
            ),
        );
    }

    // No selectors — structural but untestable. N:M widening
    // means a TEST may carry a singular `test_selector` (legacy), a
    // `test_selectors` Vec, or both; `all_selectors()` merges and
    // dedupes. An empty result after merge is the untestable case.
    let selectors = t.all_selectors();
    if selectors.is_empty() {
        return (
            TestStatus {
                status: RequirementStatus::Gap,
                root_cause_uid: Some(uid.to_string()),
            },
            make_diag(
                RequirementStatus::Gap,
                format!("TEST {} has no `test_selector` or `test_selectors`", t.id),
                Some(Location {
                    file: Some(PathBuf::from("tests.toml")),
                    entry_uid: Some(uid.to_string()),
                    ..Location::default()
                }),
                Some(FixHint::AddTomlKey {
                    path: PathBuf::from("tests.toml"),
                    toml_path: format!("tests[id={}]", t.id),
                    key: "test_selector".into(),
                    value_stub: "<crate>::<module>::<fn_name>".into(),
                }),
                None,
            ),
        );
    }

    // N:M selectors: resolve each against `outcomes`, aggregate by
    // the strict rule (TEST passes iff every selector matches a run
    // fn AND every matched fn passed). decision: strict
    // resolution — laxity defeats the contract of selector
    // check.
    //
    // 1:1 fast path (single selector) preserves the pre-N:M messages
    // so existing integration tests don't need flipping; the Vec
    // path aggregates across all selectors.
    if selectors.len() > 1 {
        return multi_selector::status(t, uid, &selectors, outcomes);
    }
    let selector = &selectors[0];
    let matches: Vec<&String> = outcomes
        .keys()
        .filter(|k| k.as_str() == selector.as_str() || ends_with_fn(k, selector))
        .collect();
    match matches.as_slice() {
        [] => (
            TestStatus {
                status: RequirementStatus::Gap,
                root_cause_uid: Some(uid.to_string()),
            },
            make_diag(
                RequirementStatus::Gap,
                format!(
                    "TEST {}: selector '{}' did not run in this session (not in cargo test output)",
                    t.id, selector
                ),
                Some(Location {
                    file: Some(PathBuf::from("tests.toml")),
                    entry_uid: Some(uid.to_string()),
                    ..Location::default()
                }),
                None,
                None,
            ),
        ),
        [only_match] => match outcomes[*only_match] {
            TestOutcome::Passed => (
                TestStatus {
                    status: RequirementStatus::Pass,
                    root_cause_uid: None,
                },
                make_diag(
                    RequirementStatus::Pass,
                    format!("TEST {} passed ({})", t.id, only_match),
                    Some(Location {
                        entry_uid: Some(uid.to_string()),
                        ..Location::default()
                    }),
                    None,
                    None,
                ),
            ),
            TestOutcome::Failed => (
                TestStatus {
                    status: RequirementStatus::Gap,
                    root_cause_uid: Some(uid.to_string()),
                },
                make_diag(
                    RequirementStatus::Gap,
                    format!("TEST {} failed in this run ({})", t.id, only_match),
                    Some(Location {
                        entry_uid: Some(uid.to_string()),
                        ..Location::default()
                    }),
                    // No FixHint — fix lives in source.
                    None,
                    None,
                ),
            ),
            TestOutcome::Ignored => (
                TestStatus {
                    status: RequirementStatus::Skip,
                    root_cause_uid: None,
                },
                make_diag(
                    RequirementStatus::Skip,
                    format!("TEST {} was #[ignore]'d in this run", t.id),
                    Some(Location {
                        entry_uid: Some(uid.to_string()),
                        ..Location::default()
                    }),
                    None,
                    None,
                ),
            ),
        },
        many => (
            TestStatus {
                status: RequirementStatus::Gap,
                root_cause_uid: Some(uid.to_string()),
            },
            make_diag(
                RequirementStatus::Gap,
                format!(
                    "TEST {}: selector '{}' is ambiguous — matches {} outcome keys: [{}]",
                    t.id,
                    selector,
                    many.len(),
                    many.iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                Some(Location {
                    file: Some(PathBuf::from("tests.toml")),
                    entry_uid: Some(uid.to_string()),
                    ..Location::default()
                }),
                Some(FixHint::AddTomlKey {
                    path: PathBuf::from("tests.toml"),
                    toml_path: format!("tests[id={}]", t.id),
                    key: "test_selector".into(),
                    value_stub: format!(
                        "<one of: {}>",
                        many.iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                }),
                None,
            ),
        ),
    }
}

pub(super) fn make_diag(
    status: RequirementStatus,
    message: String,
    location: Option<Location>,
    fix_hint: Option<FixHint>,
    root_cause_uid: Option<String>,
) -> Diagnostic {
    Diagnostic {
        code: status.code().to_string(),
        severity: status.severity(),
        message,
        location,
        fix_hint,
        subcommand: None,
        root_cause_uid,
    }
}

/// Return entries whose `traces_to` contains `parent_uid`.
pub(super) fn test_children_of<'a>(
    entries: &'a [TestEntry],
    parent_uid: Option<&str>,
) -> Vec<&'a TestEntry> {
    let Some(parent) = parent_uid else {
        return vec![];
    };
    entries
        .iter()
        .filter(|e| e.traces_to.iter().any(|u| u == parent))
        .collect()
}

pub(super) fn llr_children_of<'a>(
    entries: &'a [LlrEntry],
    parent_uid: Option<&str>,
) -> Vec<&'a LlrEntry> {
    let Some(parent) = parent_uid else {
        return vec![];
    };
    entries
        .iter()
        .filter(|e| e.traces_to.iter().any(|u| u == parent))
        .collect()
}

pub(super) fn hlr_children_of<'a>(
    entries: &'a [HlrEntry],
    parent_uid: Option<&str>,
) -> Vec<&'a HlrEntry> {
    let Some(parent) = parent_uid else {
        return vec![];
    };
    entries
        .iter()
        .filter(|e| e.traces_to.iter().any(|u| u == parent))
        .collect()
}

/// TOML-pointer-style path: `requirements[N]` where N is the 0-based
/// index of `needle` inside `ids`. Flat `&[&str]` so the function is
/// entry-type-agnostic.
pub(super) fn find_toml_path_by_id(ids: &[&str], needle: &str) -> String {
    let idx = ids.iter().position(|id| *id == needle).unwrap_or(0);
    format!("requirements[{}]", idx)
}

/// Does a fully-qualified outcome key end in the given fn-name
/// selector? Used for suffix-match when the trace carries an
/// unqualified selector (bare `fn_name`) against outcome keys like
/// `binary::module::fn_name`.
pub(super) fn ends_with_fn(key: &str, selector: &str) -> bool {
    if selector.contains("::") {
        key == selector || key.ends_with(&format!("::{}", selector))
    } else {
        key.rsplit("::").next() == Some(selector)
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
    fn ends_with_fn_handles_qualified_and_unqualified() {
        assert!(ends_with_fn("binary::module::test_x", "test_x"));
        assert!(ends_with_fn("binary::module::test_x", "module::test_x"));
        assert!(!ends_with_fn("binary::other::test_x", "module::test_x"));
        assert!(!ends_with_fn("binary::module::other", "test_x"));
        assert!(ends_with_fn("foo::bar", "foo::bar"));
    }

    #[test]
    fn aggregate_empty_children_is_pass() {
        let status: BTreeMap<String, TestStatus> = BTreeMap::new();
        let agg = aggregate_child_status(&[], &status);
        assert_eq!(agg.status, RequirementStatus::Pass);
        assert!(agg.root_cause_uid.is_none());
    }
}
