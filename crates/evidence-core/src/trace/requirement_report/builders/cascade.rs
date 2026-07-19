//! Cascade aggregation: roll up child TEST statuses into one parent
//! `Diagnostic` per HLR / LLR / SYS entry.
//!
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::diagnostic::{Diagnostic, FixHint, Location};

use super::super::{RequirementKind, RequirementStatus, TestStatus};
use super::make_diag;

/// Bundled per-entry location + structural data for
/// [`build_cascade_diag`]. Kept as a struct (rather than free args) to
/// satisfy `clippy::too_many_arguments` — the cascade builder needs
/// all of these cohesively, and splitting them into positional
/// parameters just moves the ceremony to every caller.
pub(in super::super) struct CascadeEntry<'a> {
    pub kind: RequirementKind,
    pub id: &'a str,
    pub uid: &'a str,
    pub traces_to: &'a [String],
    pub verification_methods: &'a [String],
    pub link_gap: Option<&'a str>,
    pub toml_path: String,
    pub file: PathBuf,
}

pub(in super::super) fn build_cascade_diag(
    entry: CascadeEntry<'_>,
    child_uids: &[&str],
    child_status: &BTreeMap<String, TestStatus>,
    policy: &crate::policy::TracePolicy,
) -> (TestStatus, Diagnostic) {
    if let Some((message, fix_hint)) = structural_gap(&entry, policy) {
        return direct_gap(&entry, message, fix_hint);
    }
    if child_uids.is_empty() {
        return direct_gap(
            &entry,
            format!(
                "{} {} has no {} coverage",
                label(entry.kind),
                entry.id,
                child_label(entry.kind)
            ),
            None,
        );
    }

    let aggregated = aggregate_child_status(child_uids, child_status);
    let msg = match aggregated.status {
        RequirementStatus::Pass => format!("{} {} satisfied", label(entry.kind), entry.id),
        RequirementStatus::Gap => format!(
            "{} {}: one or more downstream requirements failed",
            label(entry.kind),
            entry.id
        ),
        RequirementStatus::Skip => format!(
            "{} {}: all downstream requirements were skipped",
            label(entry.kind),
            entry.id
        ),
    };
    let diag = make_diag(
        aggregated.status,
        msg,
        Some(location(&entry)),
        None,
        aggregated.root_cause_uid.clone(),
    );
    (aggregated, diag)
}

fn structural_gap(
    entry: &CascadeEntry<'_>,
    policy: &crate::policy::TracePolicy,
) -> Option<(String, Option<FixHint>)> {
    if let Some(message) = entry.link_gap {
        return Some((message.to_string(), None));
    }
    if matches!(entry.kind, RequirementKind::Hlr)
        && policy.require_hlr_sys_trace
        && entry.traces_to.is_empty()
    {
        return Some((
            format!(
                "HLR {} has empty `traces_to` under `require_hlr_sys_trace` policy",
                entry.id
            ),
            Some(add_key_fix(entry, "traces_to", "[\"<SYS-uuid>\"]")),
        ));
    }
    if matches!(entry.kind, RequirementKind::Llr) && entry.traces_to.is_empty() {
        return Some((
            format!("LLR {} has no parent HLR edge", entry.id),
            Some(add_key_fix(entry, "traces_to", "[\"<HLR-uuid>\"]")),
        ));
    }
    if verification_required(entry.kind, policy) && entry.verification_methods.is_empty() {
        return Some((
            format!(
                "{} {} is missing verification methods",
                label(entry.kind),
                entry.id
            ),
            Some(add_key_fix(entry, "verification_methods", "[\"test\"]")),
        ));
    }
    None
}

fn add_key_fix(entry: &CascadeEntry<'_>, key: &str, value_stub: &str) -> FixHint {
    FixHint::AddTomlKey {
        path: entry.file.clone(),
        toml_path: entry.toml_path.clone(),
        key: key.into(),
        value_stub: value_stub.into(),
    }
}

fn direct_gap(
    entry: &CascadeEntry<'_>,
    message: String,
    fix_hint: Option<FixHint>,
) -> (TestStatus, Diagnostic) {
    let status = TestStatus {
        status: RequirementStatus::Gap,
        root_cause_uid: Some(entry.uid.to_string()),
    };
    let diag = make_diag(
        RequirementStatus::Gap,
        message,
        Some(location(entry)),
        fix_hint,
        None,
    );
    (status, diag)
}

fn location(entry: &CascadeEntry<'_>) -> Location {
    Location {
        file: Some(entry.file.clone()),
        toml_path: Some(entry.toml_path.clone()),
        entry_uid: Some(entry.uid.to_string()),
        ..Location::default()
    }
}

fn verification_required(kind: RequirementKind, policy: &crate::policy::TracePolicy) -> bool {
    match kind {
        RequirementKind::Hlr => policy.require_hlr_verification_methods,
        RequirementKind::Llr => policy.require_llr_verification_methods,
        RequirementKind::Sys => false,
    }
}

fn label(kind: RequirementKind) -> &'static str {
    match kind {
        RequirementKind::Sys => "SYS",
        RequirementKind::Hlr => "HLR",
        RequirementKind::Llr => "LLR",
    }
}

fn child_label(kind: RequirementKind) -> &'static str {
    match kind {
        RequirementKind::Sys => "HLR",
        RequirementKind::Hlr => "LLR",
        RequirementKind::Llr => "TEST",
    }
}

pub(in super::super) fn aggregate_child_status(
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
