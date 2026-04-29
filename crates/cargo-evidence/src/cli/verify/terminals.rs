//! Terminal-event constructors for the verify pipeline.
//!
//! Schema Rule 1 (HLR-001) reserves `_OK`, `_FAIL`, and `_ERROR`
//! suffixes for the last JSONL line of a `--format=jsonl` run.
//! These three helpers are the single point of truth for the
//! terminal codes the verify subcommand emits — pulled into a
//! sibling file so the orchestrator stays under the workspace
//! 500-line limit.

use evidence_core::diagnostic::{Diagnostic, Severity};

pub(super) fn terminal_ok(message: &str) -> Diagnostic {
    Diagnostic {
        code: "VERIFY_OK".to_string(),
        severity: Severity::Info,
        message: message.to_string(),
        location: None,
        fix_hint: None,
        subcommand: None,
        root_cause_uid: None,
    }
}

pub(super) fn terminal_fail(message: &str) -> Diagnostic {
    Diagnostic {
        code: "VERIFY_FAIL".to_string(),
        severity: Severity::Error,
        message: message.to_string(),
        location: None,
        fix_hint: None,
        subcommand: None,
        root_cause_uid: None,
    }
}

pub(super) fn terminal_error(message: &str) -> Diagnostic {
    Diagnostic {
        code: "VERIFY_ERROR".to_string(),
        severity: Severity::Error,
        message: message.to_string(),
        location: None,
        fix_hint: None,
        subcommand: None,
        root_cause_uid: None,
    }
}
