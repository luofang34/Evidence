//! Cascade aggregation: roll up child TEST statuses into one parent
//! `Diagnostic` per HLR / LLR / SYS entry.
//!
//! Split from `builders.rs` to keep the facade under the 500-line
//! workspace file-size limit.

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
    pub uid: Option<&'a str>,
    pub traces_to: &'a [String],
    pub toml_path: String,
    pub file: PathBuf,
}

pub(in super::super) fn build_cascade_diag(
    entry: CascadeEntry<'_>,
    child_uids: &[&str],
    child_status: &BTreeMap<String, TestStatus>,
    policy: &crate::policy::TracePolicy,
) -> Diagnostic {
    let CascadeEntry {
        kind,
        id,
        uid,
        traces_to,
        toml_path,
        file,
    } = entry;
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
