//! Per-requirement pass/gap reporting for `cargo evidence check`.
//!
//! Projects a [`CorpusGraph`] into one canonical diagnostic per
//! SYS/HLR/LLR/Test node:
//!
//! - `REQ_PASS` (info) — entry's test selector resolved and the named
//!   test passed this run; or a higher-level entry whose children all
//!   passed.
//! - `REQ_GAP` (error) — entry has a structural problem (missing uid,
//!   empty `traces_to` under policy, unresolvable selector) OR its
//!   downstream chain contains a failure. Derived GAPs carry
//!   `root_cause_uid` pointing at the primary failure; mechanical GAPs
//!   carry a `FixHint` variant whose kind matches the sub-case.
//! - `REQ_SKIP` (warning) — an entry excluded by `#[ignore]`.
//!
//! Dedup semantics (Schema Rule 7): one event per
//! requirement, not one total. Agents group client-side by
//! `root_cause_uid`. See
//! [`Diagnostic::root_cause_uid`](crate::diagnostic::Diagnostic::root_cause_uid).

mod builders;
mod view;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::bundle::TestOutcome;
use crate::corpus::{CorpusGraph, EdgeKind, graph_from_trace_files};
use crate::diagnostic::{Diagnostic, DiagnosticCode, Severity};
use crate::policy::TracePolicy;
use crate::trace::read::TraceFiles;
use crate::trace::selector_check::resolve_selector_inputs;

use builders::{CascadeEntry, build_cascade_diag, build_test_diag, make_diag};
use view::{ReportRequirement, ReportView};

/// A graph shape that cannot be represented by the requirement report.
#[derive(Debug, Error)]
pub enum RequirementReportError {
    /// A node carries an edge kind that is invalid for its report role.
    #[error("node {from} carries unsupported {kind:?} edge in requirement report")]
    UnsupportedEdge {
        /// Source node uid.
        from: String,
        /// Edge kind that cannot be interpreted.
        kind: EdgeKind,
    },
}

/// Closed enum for the three per-requirement codes. Implementing
/// [`DiagnosticCode`] here registers the codes in the walked registry
/// so `diagnostic_codes_locked` enforces regex + uniqueness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequirementStatus {
    /// `REQ_PASS` — the requirement is satisfied.
    Pass,
    /// `REQ_GAP` — the requirement is not satisfied.
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

/// Adapt `trace` to the corpus graph and emit one diagnostic per node.
///
/// Graph construction failures become a `REQ_GAP`, so a malformed
/// trace cannot disappear from the terminal verdict.
pub fn build_requirement_report(
    trace: &TraceFiles,
    test_outcomes: &BTreeMap<String, TestOutcome>,
    workspace_root: &Path,
    policy: &TracePolicy,
) -> Vec<Diagnostic> {
    let graph = match graph_from_trace_files(trace) {
        Ok(graph) => graph,
        Err(error) => return vec![graph_failure(error.to_string())],
    };
    match build_corpus_requirement_report(&graph, test_outcomes, workspace_root, policy) {
        Ok(diagnostics) => diagnostics,
        Err(error) => vec![graph_failure(error.to_string())],
    }
}

/// Emit the canonical requirement report derived from `graph`.
///
/// # Errors
///
/// Returns [`RequirementReportError`] when a node carries an edge kind
/// that has no requirement-report interpretation.
pub fn build_corpus_requirement_report(
    graph: &CorpusGraph,
    test_outcomes: &BTreeMap<String, TestOutcome>,
    workspace_root: &Path,
    policy: &TracePolicy,
) -> Result<Vec<Diagnostic>, RequirementReportError> {
    let view = ReportView::from_graph(graph)?;
    let unresolved: BTreeSet<String> =
        resolve_selector_inputs(&view.selector_inputs(), workspace_root)
            .into_iter()
            .map(|value| value.id)
            .collect();
    let mut diagnostics = Vec::new();
    let mut test_status = BTreeMap::new();
    for test in &view.tests {
        let (status, diagnostic) = build_test_diag(test, test_outcomes, &unresolved);
        test_status.insert(test.uid.clone(), status);
        diagnostics.push(diagnostic);
    }
    let llr_status = build_layer(
        &view.llrs,
        RequirementKind::Llr,
        "llr.toml",
        &view,
        &test_status,
        policy,
        &mut diagnostics,
    );
    let hlr_status = build_layer(
        &view.hlrs,
        RequirementKind::Hlr,
        "hlr.toml",
        &view,
        &llr_status,
        policy,
        &mut diagnostics,
    );
    build_layer(
        &view.sys,
        RequirementKind::Sys,
        "sys.toml",
        &view,
        &hlr_status,
        policy,
        &mut diagnostics,
    );
    Ok(diagnostics)
}

fn build_layer(
    entries: &[ReportRequirement],
    kind: RequirementKind,
    file: &str,
    view: &ReportView,
    child_status: &BTreeMap<String, TestStatus>,
    policy: &TracePolicy,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<String, TestStatus> {
    let mut statuses = BTreeMap::new();
    for (index, entry) in entries.iter().enumerate() {
        let children: Vec<&str> = view
            .children_of(&entry.uid)
            .iter()
            .map(String::as_str)
            .collect();
        let (status, diagnostic) = build_cascade_diag(
            CascadeEntry {
                kind,
                id: &entry.id,
                uid: &entry.uid,
                traces_to: &entry.traces_to,
                verification_methods: &entry.verification_methods,
                link_gap: entry.link_gap.as_deref(),
                toml_path: format!("requirements[{index}]"),
                file: PathBuf::from(file),
            },
            &children,
            child_status,
            policy,
        );
        statuses.insert(entry.uid.clone(), status);
        diagnostics.push(diagnostic);
    }
    statuses
}

fn graph_failure(message: String) -> Diagnostic {
    make_diag(
        RequirementStatus::Gap,
        format!("cannot derive requirement report from corpus graph: {message}"),
        None,
        None,
        None,
    )
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
