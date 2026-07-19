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

pub(super) use cascade::{CascadeEntry, build_cascade_diag};

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::bundle::TestOutcome;
use crate::diagnostic::{Diagnostic, DiagnosticCode, FixHint, Location};

use super::view::ReportTest;
use super::{RequirementStatus, TestStatus};

pub(super) fn build_test_diag(
    t: &ReportTest,
    outcomes: &BTreeMap<String, TestOutcome>,
    unresolved_ids: &std::collections::BTreeSet<String>,
) -> (TestStatus, Diagnostic) {
    let uid = t.uid.as_str();
    if let Some(result) = link_gap(t, uid) {
        return result;
    }
    if let Some(result) = unresolved_gap(t, uid, unresolved_ids) {
        return result;
    }
    let selectors = &t.selectors;
    if let Some(result) = missing_selector_gap(t, uid) {
        return result;
    }
    if let Some(result) = orphan_gap(t, uid) {
        return result;
    }
    if selectors.len() > 1 {
        return multi_selector::status(t, uid, selectors, outcomes);
    }
    single_selector_status(t, uid, &selectors[0], outcomes)
}

fn link_gap(t: &ReportTest, uid: &str) -> Option<(TestStatus, Diagnostic)> {
    t.link_gap
        .as_ref()
        .map(|message| test_gap(t, uid, message.clone(), None))
}

fn unresolved_gap(
    t: &ReportTest,
    uid: &str,
    unresolved_ids: &std::collections::BTreeSet<String>,
) -> Option<(TestStatus, Diagnostic)> {
    if !unresolved_ids.contains(&t.id) {
        return None;
    }
    let selectors = t.selectors.join(", ");
    let fix = FixHint::AddTomlKey {
        path: PathBuf::from("tests.toml"),
        toml_path: format!("tests[id={}]", t.id),
        key: "test_selector".into(),
        value_stub: format!("<fully-qualified selector; [{selectors}] did not resolve>"),
    };
    Some(test_gap(
        t,
        uid,
        format!(
            "TEST {} selector(s) [{}] did not resolve to a real #[test] fn",
            t.id, selectors
        ),
        Some(fix),
    ))
}

fn missing_selector_gap(t: &ReportTest, uid: &str) -> Option<(TestStatus, Diagnostic)> {
    if !t.selectors.is_empty() {
        return None;
    }
    let fix = FixHint::AddTomlKey {
        path: PathBuf::from("tests.toml"),
        toml_path: format!("tests[id={}]", t.id),
        key: "test_selector".into(),
        value_stub: "<crate>::<module>::<fn_name>".into(),
    };
    Some(test_gap(
        t,
        uid,
        format!("TEST {} has no `test_selector` or `test_selectors`", t.id),
        Some(fix),
    ))
}

fn orphan_gap(t: &ReportTest, uid: &str) -> Option<(TestStatus, Diagnostic)> {
    if !t.traces_to.is_empty() {
        return None;
    }
    let fix = FixHint::AddTomlKey {
        path: PathBuf::from("tests.toml"),
        toml_path: format!("tests[id={}]", t.id),
        key: "traces_to".into(),
        value_stub: "[\"<LLR-uuid>\"]".into(),
    };
    Some(test_gap(
        t,
        uid,
        format!("TEST {} is orphaned with no LLR verification edge", t.id),
        Some(fix),
    ))
}

fn test_gap(
    t: &ReportTest,
    uid: &str,
    message: String,
    fix_hint: Option<FixHint>,
) -> (TestStatus, Diagnostic) {
    let status = TestStatus {
        status: RequirementStatus::Gap,
        root_cause_uid: Some(uid.to_string()),
    };
    let diagnostic = make_diag(
        RequirementStatus::Gap,
        message,
        Some(test_location(t, uid)),
        fix_hint,
        None,
    );
    (status, diagnostic)
}

fn single_selector_status(
    t: &ReportTest,
    uid: &str,
    selector: &str,
    outcomes: &BTreeMap<String, TestOutcome>,
) -> (TestStatus, Diagnostic) {
    let matches: Vec<&String> = outcomes
        .keys()
        .filter(|k| k.as_str() == selector || ends_with_fn(k, selector))
        .collect();
    match matches.as_slice() {
        [] => selector_did_not_run(t, uid, selector),
        [only_match] => matched_selector(t, uid, only_match, outcomes[*only_match]),
        many => ambiguous_selector(t, uid, selector, many),
    }
}

fn selector_did_not_run(t: &ReportTest, uid: &str, selector: &str) -> (TestStatus, Diagnostic) {
    test_gap(
        t,
        uid,
        format!(
            "TEST {}: selector '{}' did not run in this session (not in cargo test output)",
            t.id, selector
        ),
        None,
    )
}

fn matched_selector(
    t: &ReportTest,
    uid: &str,
    matched: &str,
    outcome: TestOutcome,
) -> (TestStatus, Diagnostic) {
    let (status, root_cause_uid, message) = match outcome {
        TestOutcome::Passed => (
            RequirementStatus::Pass,
            None,
            format!("TEST {} passed ({matched})", t.id),
        ),
        TestOutcome::Failed => (
            RequirementStatus::Gap,
            Some(uid.to_string()),
            format!("TEST {} failed in this run ({matched})", t.id),
        ),
        TestOutcome::Ignored => (
            RequirementStatus::Skip,
            None,
            format!("TEST {} was #[ignore]'d in this run", t.id),
        ),
    };
    let test_status = TestStatus {
        status,
        root_cause_uid,
    };
    let diagnostic = make_diag(
        status,
        message,
        Some(Location {
            entry_uid: Some(uid.to_string()),
            ..Location::default()
        }),
        None,
        None,
    );
    (test_status, diagnostic)
}

fn ambiguous_selector(
    t: &ReportTest,
    uid: &str,
    selector: &str,
    matches: &[&String],
) -> (TestStatus, Diagnostic) {
    let matched = matches
        .iter()
        .map(|value| value.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let fix = FixHint::AddTomlKey {
        path: PathBuf::from("tests.toml"),
        toml_path: format!("tests[id={}]", t.id),
        key: "test_selector".into(),
        value_stub: format!("<one of: {matched}>"),
    };
    test_gap(
        t,
        uid,
        format!(
            "TEST {}: selector '{}' is ambiguous — matches {} outcome keys: [{}]",
            t.id,
            selector,
            matches.len(),
            matched
        ),
        Some(fix),
    )
}

fn test_location(t: &ReportTest, uid: &str) -> Location {
    Location {
        file: Some(PathBuf::from("tests.toml")),
        toml_path: Some(format!("tests[id={}]", t.id)),
        entry_uid: Some(uid.to_string()),
        ..Location::default()
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
        let agg = cascade::aggregate_child_status(&[], &status);
        assert_eq!(agg.status, RequirementStatus::Pass);
        assert!(agg.root_cause_uid.is_none());
    }
}
