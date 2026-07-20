//! Presentation helpers for `check --mode=source`: the human-mode
//! renderer plus the two terminal builders. Split out of the
//! parent `check.rs` to stay under the 500-line workspace
//! file-size limit; pulled in via `#[path]`.

use evidence_core::diagnostic::{Diagnostic, Severity};

/// Render per-requirement diagnostics as `[✓]` / `[⚠]` / `[✗]`
/// tagged lines on stdout, followed by the terminal's message as
/// a final summary line. Invoked when `format == Human`.
///
/// Requirement ID (`TEST-NNN` / `HLR-NNN` / …) is extracted from
/// the diagnostic's message prefix when available — the message
/// shape is `"TEST TEST-050 passed (selector…)"` for REQ_PASS and
/// `"TEST TEST-050: selector(s) did not run…"` for REQ_GAP. If
/// parsing fails (e.g. CLI_INVALID_ARGUMENT on an empty-dir run)
/// the whole message becomes the line.
pub(super) fn render_human_diagnostics(diagnostics: &[Diagnostic], terminal: &Diagnostic) {
    for diag in diagnostics {
        let tag = match diag.severity {
            Severity::Info => "[✓]",
            Severity::Warning => "[⚠]",
            Severity::Error => "[✗]",
        };
        println!("{} {}", tag, diag.message);
    }
    println!();
    let tag = match terminal.severity {
        Severity::Info => "check:",
        Severity::Warning => "check (warning):",
        Severity::Error => "check: FAIL —",
    };
    println!("{} {}", tag, terminal.message);
}

pub(super) fn terminal_check_ok(message: &str) -> Diagnostic {
    Diagnostic {
        code: "VERIFY_OK".to_string(),
        severity: Severity::Info,
        message: message.to_string(),
        location: None,
        fix_hint: None,
        subcommand: Some("check".to_string()),
        root_cause_uid: None,
    }
}

pub(super) fn terminal_check_fail(message: &str) -> Diagnostic {
    Diagnostic {
        code: "VERIFY_FAIL".to_string(),
        severity: Severity::Error,
        message: message.to_string(),
        location: None,
        fix_hint: None,
        subcommand: Some("check".to_string()),
        root_cause_uid: None,
    }
}
