//! Diagnostic builders + cascade helpers for `requirement_report`.
//!
//! Split out of the parent module to stay under the 500-line file-size
//! limit enforced by `crates/evidence/tests/file_size_limit.rs`. Every
//! item here is `pub(super)` — the module is an implementation detail
//! of [`super::build_requirement_report`].

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::bundle::TestOutcome;
use crate::diagnostic::{Diagnostic, DiagnosticCode, FixHint, Location};
use crate::trace::entries::{HlrEntry, LlrEntry, TestEntry};

use super::{RequirementKind, RequirementStatus, TestStatus};

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
        let selector = t.test_selector.clone().unwrap_or_default();
        return (
            TestStatus {
                status: RequirementStatus::Gap,
                root_cause_uid: Some(uid.to_string()),
            },
            make_diag(
                RequirementStatus::Gap,
                format!(
                    "TEST {} selector '{}' did not resolve to a real #[test] fn",
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
                        "<fully-qualified selector; current '{}' did not resolve>",
                        selector
                    ),
                }),
                None,
            ),
        );
    }

    // No selector — structural but untestable.
    let selector = match t.test_selector.as_deref() {
        Some(s) if !s.trim().is_empty() => s,
        _ => {
            return (
                TestStatus {
                    status: RequirementStatus::Gap,
                    root_cause_uid: Some(uid.to_string()),
                },
                make_diag(
                    RequirementStatus::Gap,
                    format!("TEST {} has no `test_selector`", t.id),
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
    };

    // Try exact + suffix-fn match.
    let matches: Vec<&String> = outcomes
        .keys()
        .filter(|k| k.as_str() == selector || ends_with_fn(k, selector))
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

pub(super) fn build_cascade_diag(
    kind: RequirementKind,
    id: &str,
    uid: Option<&str>,
    traces_to: &[String],
    child_uids: &[&str],
    child_status: &BTreeMap<String, TestStatus>,
    policy: &crate::policy::TracePolicy,
    toml_path: String,
    file: PathBuf,
) -> Diagnostic {
    let kind_label = match kind {
        RequirementKind::Sys => "SYS",
        RequirementKind::Hlr => "HLR",
        RequirementKind::Llr => "LLR",
    };

    // Structural: missing uid.
    let Some(uid_str) = uid else {
        return make_diag(
            RequirementStatus::Gap,
            format!("{} {} is missing `uid`", kind_label, id),
            Some(Location {
                file: Some(file.clone()),
                toml_path: Some(toml_path.clone()),
                ..Location::default()
            }),
            Some(FixHint::AssignUuid {
                path: file,
                toml_path,
            }),
            None,
        );
    };
    let uid = uid_str.to_string();

    // Structural: empty traces_to under the HLR→SYS policy gate.
    if matches!(kind, RequirementKind::Hlr) && policy.require_hlr_sys_trace && traces_to.is_empty()
    {
        return make_diag(
            RequirementStatus::Gap,
            format!(
                "HLR {} has empty `traces_to` under `require_hlr_sys_trace` policy",
                id
            ),
            Some(Location {
                file: Some(file.clone()),
                toml_path: Some(toml_path.clone()),
                entry_uid: Some(uid.clone()),
                ..Location::default()
            }),
            Some(FixHint::AddTomlKey {
                path: file,
                toml_path,
                key: "traces_to".into(),
                value_stub: format!("[\"<{}-uuid>\"]", kind.parent_label()),
            }),
            None,
        );
    }

    let aggregated = aggregate_child_status(child_uids, child_status);
    let msg = match aggregated.status {
        RequirementStatus::Pass => format!("{} {} satisfied", kind_label, id),
        RequirementStatus::Gap => format!(
            "{} {}: one or more downstream requirements failed",
            kind_label, id
        ),
        RequirementStatus::Skip => format!(
            "{} {}: all downstream requirements were skipped",
            kind_label, id
        ),
    };
    make_diag(
        aggregated.status,
        msg,
        Some(Location {
            file: Some(file),
            toml_path: Some(toml_path),
            entry_uid: Some(uid),
            ..Location::default()
        }),
        None,
        aggregated.root_cause_uid,
    )
}

pub(super) fn aggregate_child_status(
    child_uids: &[&str],
    child_status: &BTreeMap<String, TestStatus>,
) -> TestStatus {
    if child_uids.is_empty() {
        return TestStatus {
            status: RequirementStatus::Pass,
            root_cause_uid: None,
        };
    }
    let mut first_gap_root: Option<String> = None;
    let mut any_pass = false;
    let mut any_skip = false;
    for u in child_uids {
        if let Some(s) = child_status.get(*u) {
            match s.status {
                RequirementStatus::Gap => {
                    if first_gap_root.is_none() {
                        first_gap_root = s.root_cause_uid.clone().or_else(|| Some(u.to_string()));
                    }
                }
                RequirementStatus::Pass => any_pass = true,
                RequirementStatus::Skip => any_skip = true,
            }
        }
    }
    if let Some(root) = first_gap_root {
        TestStatus {
            status: RequirementStatus::Gap,
            root_cause_uid: Some(root),
        }
    } else if any_pass {
        TestStatus {
            status: RequirementStatus::Pass,
            root_cause_uid: None,
        }
    } else if any_skip {
        TestStatus {
            status: RequirementStatus::Skip,
            root_cause_uid: None,
        }
    } else {
        TestStatus {
            status: RequirementStatus::Pass,
            root_cause_uid: None,
        }
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
