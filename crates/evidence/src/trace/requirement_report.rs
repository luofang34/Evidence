//! Per-requirement pass/gap reporting for `cargo evidence check`.
//!
//! Walks a parsed [`TraceFiles`] and, for each SYS/HLR/LLR/Test entry,
//! emits one [`Diagnostic`]:
//!
//! - `REQ_PASS` (info) — entry's test selector resolved and the named
//!   test passed this run; or a higher-level entry whose children all
//!   passed.
//! - `REQ_GAP` (error) — entry has a structural problem (missing uid,
//!   empty `traces_to` under policy, unresolvable selector) OR its
//!   downstream chain contains a failure. Derived GAPs carry
//!   `root_cause_uid` pointing at the primary failure; mechanical GAPs
//!   carry a `FixHint` variant whose kind matches the sub-case.
//! - `REQ_SKIP` (warning) — entry intentionally excluded (currently
//!   only produced for `#[ignore]`-marked tests).
//!
//! Dedup semantics (Schema Rule 7 + PR #46 Decision 2): one event per
//! requirement, not one total. Agents group client-side by
//! `root_cause_uid`. See
//! [`Diagnostic::root_cause_uid`](crate::diagnostic::Diagnostic::root_cause_uid).
//!
//! The heavy diagnostic-construction logic lives in the sibling
//! [`builders`] module so this file stays under the workspace 500-line
//! file-size limit.

mod builders;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::bundle::TestOutcome;
use crate::diagnostic::{Diagnostic, DiagnosticCode, Severity};
use crate::policy::TracePolicy;
use crate::trace::read::TraceFiles;
use crate::trace::selector_check::resolve_test_selectors;

use builders::{
    CascadeEntry, aggregate_child_status, build_cascade_diag, build_test_diag,
    find_toml_path_by_id, hlr_children_of, llr_children_of, test_children_of,
};

/// Closed enum for the three per-requirement codes. Implementing
/// [`DiagnosticCode`] here registers the codes in the walked registry
/// so `diagnostic_codes_locked` enforces regex + uniqueness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequirementStatus {
    /// `REQ_PASS` — the requirement is currently satisfied.
    Pass,
    /// `REQ_GAP` — the requirement is currently *not* satisfied.
    Gap,
    /// `REQ_SKIP` — the requirement is intentionally excluded from
    /// this run.
    Skip,
}

impl std::fmt::Display for RequirementStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            RequirementStatus::Pass => "requirement satisfied",
            RequirementStatus::Gap => "requirement not satisfied",
            RequirementStatus::Skip => "requirement skipped",
        })
    }
}

impl DiagnosticCode for RequirementStatus {
    fn code(&self) -> &'static str {
        match self {
            RequirementStatus::Pass => "REQ_PASS",
            RequirementStatus::Gap => "REQ_GAP",
            RequirementStatus::Skip => "REQ_SKIP",
        }
    }

    fn severity(&self) -> Severity {
        match self {
            RequirementStatus::Pass => Severity::Info,
            RequirementStatus::Gap => Severity::Error,
            RequirementStatus::Skip => Severity::Warning,
        }
    }
}

/// Internal shape carrying both the final status and the root-cause
/// UID (for GAP events whose failure lives downstream).
#[derive(Debug, Clone)]
pub(super) struct TestStatus {
    pub status: RequirementStatus,
    pub root_cause_uid: Option<String>,
}

/// Which requirement layer an entry belongs to. Used by the builder
/// to pick error wording and the traces-up parent label.
#[derive(Debug, Clone, Copy)]
pub(super) enum RequirementKind {
    Sys,
    Hlr,
    Llr,
}

impl RequirementKind {
    pub(super) fn parent_label(self) -> &'static str {
        match self {
            RequirementKind::Sys => "???",
            RequirementKind::Hlr => "SYS",
            RequirementKind::Llr => "HLR",
        }
    }
}

/// Walk `trace` and `test_outcomes`, emit one diagnostic per entry.
///
/// `workspace_root` anchors `resolve_test_selectors` when it walks the
/// `.rs` source tree. `policy` is only read for its
/// `require_hlr_sys_trace` flag, matching the same gate that PR #45's
/// `trace --validate` applies.
pub fn build_requirement_report(
    trace: &TraceFiles,
    test_outcomes: &BTreeMap<String, TestOutcome>,
    workspace_root: &Path,
    policy: &TracePolicy,
) -> Vec<Diagnostic> {
    let unresolved_selectors: std::collections::BTreeSet<String> =
        resolve_test_selectors(&trace.tests.tests, workspace_root)
            .into_iter()
            .map(|u| u.id)
            .collect();

    let mut test_status: BTreeMap<String, TestStatus> = BTreeMap::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for t in &trace.tests.tests {
        let (status, diag) = build_test_diag(t, test_outcomes, &unresolved_selectors);
        if let Some(uid) = t.uid.as_deref() {
            test_status.insert(uid.to_string(), status);
        }
        diagnostics.push(diag);
    }

    // LLR cascade: children are TestEntries tracing to this LLR.
    let llr_ids: Vec<&str> = trace
        .llr
        .requirements
        .iter()
        .map(|e| e.id.as_str())
        .collect();
    for r in &trace.llr.requirements {
        let children = test_children_of(&trace.tests.tests, r.uid.as_deref());
        let toml_path = find_toml_path_by_id(&llr_ids, &r.id);
        diagnostics.push(build_cascade_diag(
            CascadeEntry {
                kind: RequirementKind::Llr,
                id: &r.id,
                uid: r.uid.as_deref(),
                traces_to: &r.traces_to,
                toml_path,
                file: PathBuf::from("llr.toml"),
            },
            &children
                .iter()
                .filter_map(|t| t.uid.as_deref())
                .collect::<Vec<_>>(),
            &test_status,
            policy,
        ));
    }

    // Side table of LLR → status for the HLR cascade.
    let llr_status: BTreeMap<String, TestStatus> = trace
        .llr
        .requirements
        .iter()
        .filter_map(|r| {
            let uid = r.uid.clone()?;
            let children = test_children_of(&trace.tests.tests, Some(&uid));
            let status = aggregate_child_status(
                &children
                    .iter()
                    .filter_map(|t| t.uid.as_deref())
                    .collect::<Vec<_>>(),
                &test_status,
            );
            Some((uid, status))
        })
        .collect();

    // HLR cascade: children are LLRs.
    let hlr_ids: Vec<&str> = trace
        .hlr
        .requirements
        .iter()
        .map(|e| e.id.as_str())
        .collect();
    for r in &trace.hlr.requirements {
        let children = llr_children_of(&trace.llr.requirements, r.uid.as_deref());
        let toml_path = find_toml_path_by_id(&hlr_ids, &r.id);
        diagnostics.push(build_cascade_diag(
            CascadeEntry {
                kind: RequirementKind::Hlr,
                id: &r.id,
                uid: r.uid.as_deref(),
                traces_to: &r.traces_to,
                toml_path,
                file: PathBuf::from("hlr.toml"),
            },
            &children
                .iter()
                .filter_map(|l| l.uid.as_deref())
                .collect::<Vec<_>>(),
            &llr_status,
            policy,
        ));
    }

    // Side table of HLR → status for the SYS cascade.
    let hlr_status: BTreeMap<String, TestStatus> = trace
        .hlr
        .requirements
        .iter()
        .filter_map(|r| {
            let uid = r.uid.clone()?;
            let children = llr_children_of(&trace.llr.requirements, Some(&uid));
            let child_uids: Vec<&str> = children.iter().filter_map(|l| l.uid.as_deref()).collect();
            Some((uid, aggregate_child_status(&child_uids, &llr_status)))
        })
        .collect();

    // SYS entries reuse HlrEntry — their children are HLRs that
    // trace_to them.
    let sys_ids: Vec<&str> = trace
        .sys
        .requirements
        .iter()
        .map(|e| e.id.as_str())
        .collect();
    for s in &trace.sys.requirements {
        let children = hlr_children_of(&trace.hlr.requirements, s.uid.as_deref());
        let toml_path = find_toml_path_by_id(&sys_ids, &s.id);
        diagnostics.push(build_cascade_diag(
            CascadeEntry {
                kind: RequirementKind::Sys,
                id: &s.id,
                uid: s.uid.as_deref(),
                traces_to: &s.traces_to,
                toml_path,
                file: PathBuf::from("sys.toml"),
            },
            &children
                .iter()
                .filter_map(|h| h.uid.as_deref())
                .collect::<Vec<_>>(),
            &hlr_status,
            policy,
        ));
    }

    diagnostics
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
    fn req_status_code_regex_and_suffix_rules() {
        // None of the REQ_* codes may end in a reserved terminal
        // suffix — they're per-requirement findings, not terminals.
        for s in [
            RequirementStatus::Pass,
            RequirementStatus::Gap,
            RequirementStatus::Skip,
        ] {
            let code = s.code();
            assert!(code.starts_with("REQ_"));
            assert!(!code.ends_with("_OK"));
            assert!(!code.ends_with("_FAIL"));
            assert!(!code.ends_with("_ERROR"));
        }
    }

    #[test]
    fn req_status_severity_matches_intent() {
        assert_eq!(RequirementStatus::Pass.severity(), Severity::Info);
        assert_eq!(RequirementStatus::Gap.severity(), Severity::Error);
        assert_eq!(RequirementStatus::Skip.severity(), Severity::Warning);
    }
}
