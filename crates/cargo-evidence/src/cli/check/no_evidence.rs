//! No-evidence gate for `check --mode=source` (LLR-105).
//! Split out of the parent `check.rs` to stay under the 500-line
//! workspace file-size limit; pulled in via `#[path]`.
//!
//! A missing trace root or a zero-requirement trace tree is an
//! adoption state, not evidence — the run must not terminate
//! `VERIFY_OK` with "0 requirement(s) satisfied". The gate emits
//! the shared `TRACE_EVIDENCE_*` semantic code (the same one
//! `trace --validate`, doctor, generate, and bundle verify use)
//! plus a `VERIFY_FAIL` terminal. Non-empty evidence — including
//! evidence that fails link validation — keeps flowing to the
//! requirement-report path, which surfaces structural problems as
//! `REQ_GAP` diagnostics.

use anyhow::Result;

use evidence_core::diagnostic::{Diagnostic, Location, Severity};
use evidence_core::policy::TracePolicy;
use evidence_core::trace::{TraceEvidenceState, evaluate_trace_evidence};

use crate::cli::args::{EXIT_ERROR, EXIT_VERIFICATION_FAILURE};
use crate::cli::output::emit_jsonl;

/// Evaluate the discovered trace root and, when it carries no
/// evidence, emit the adoption diagnostic + `VERIFY_FAIL` terminal
/// and return the exit code for the caller's format (`machine` ⇔
/// JSONL streaming, matching `cli/trace.rs`'s per-format
/// convention: exit 2 machine, exit 1 human). Returns `Ok(None)`
/// when evidence exists (valid or invalid) so the caller proceeds
/// to the requirement report.
pub(super) fn no_evidence_gate(
    trace_root: &str,
    policy: &TracePolicy,
    machine: bool,
) -> Result<Option<i32>> {
    let eval = evaluate_trace_evidence(std::slice::from_ref(&trace_root.to_string()), policy);
    let Some(code) = eval.state.gap_code() else {
        return Ok(None);
    };
    let message = match &eval.state {
        TraceEvidenceState::NotAdopted { missing_roots } => format!(
            "trace root(s) missing on disk: {} — trace evidence is not adopted",
            missing_roots.join(", ")
        ),
        TraceEvidenceState::Empty => format!(
            "trace root '{}' holds zero requirements across all layers — \
             an empty trace tree is an adoption state, not valid evidence",
            trace_root
        ),
        TraceEvidenceState::NotConfigured => {
            "no trace roots configured or discoverable".to_string()
        }
        // gap_code() above gates this to the three no-evidence states.
        TraceEvidenceState::Invalid | TraceEvidenceState::Valid => String::new(),
    };
    let diag = Diagnostic {
        code: code.to_string(),
        severity: Severity::Error,
        message: message.clone(),
        location: Some(Location {
            file: Some(std::path::PathBuf::from(trace_root)),
            ..Location::default()
        }),
        fix_hint: None,
        subcommand: Some("check".to_string()),
        root_cause_uid: None,
    };
    let terminal = super::render::terminal_check_fail(&message);
    if machine {
        emit_jsonl(&diag)?;
        emit_jsonl(&terminal)?;
        Ok(Some(EXIT_VERIFICATION_FAILURE))
    } else {
        super::render::render_human_diagnostics(std::slice::from_ref(&diag), &terminal);
        Ok(Some(EXIT_ERROR))
    }
}
