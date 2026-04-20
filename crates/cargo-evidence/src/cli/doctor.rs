//! `cargo evidence doctor` — downstream rigor audit (LLR-048).
//!
//! Runs a fixed-order checklist against the current workspace and
//! emits one `Diagnostic` per check plus one terminal
//! (`DOCTOR_OK` / `DOCTOR_FAIL`) per Schema Rule 1. Jsonl-native.
//!
//! The six MVP checks:
//!
//! - **trace validity** (`DOCTOR_TRACE_INVALID` on fail) — load
//!   `tool/trace/` and run the SYS / surface / selector bijections
//!   the tool's own CI enables on itself.
//! - **floors present + satisfied**
//!   (`DOCTOR_FLOORS_MISSING` / `DOCTOR_FLOORS_VIOLATED`) — load
//!   `cert/floors.toml`, compute measurements, fail if any floor is
//!   breached.
//! - **boundary config** (`DOCTOR_BOUNDARY_MISSING`) — load
//!   `cert/boundary.toml`.
//! - **CI integration** (`DOCTOR_CI_INTEGRATION_MISSING` warning) —
//!   grep `.github/workflows/*.yml` for `cargo evidence`.
//! - **merge-style policy** (`DOCTOR_MERGE_STYLE_RISK` warning) —
//!   scan last 20 main-branch commits for `Merge pull request #N`
//!   pattern; warn because merge-commit style leaves the
//!   `Override-Deterministic-Baseline:` line in the PR body
//!   invisible to the push-event gate.
//! - **override protocol docs**
//!   (`DOCTOR_OVERRIDE_PROTOCOL_UNDOCUMENTED` warning) — grep
//!   `README.md` + `CONTRIBUTING.md` for the override string so
//!   contributors know the convention.
//!
//! Every check that passes emits `DOCTOR_CHECK_PASSED` with the
//! check name in `message`, so the stream shape is exactly one
//! line per check plus the terminal. Downstream tooling can
//! detect truncation.

use std::path::Path;

use anyhow::Result;

use evidence::diagnostic::{Diagnostic, Severity};

use super::args::{EXIT_SUCCESS, EXIT_VERIFICATION_FAILURE};
use super::output::emit_jsonl;

mod checks;
use checks::{
    check_boundary, check_ci_integration, check_floors, check_merge_style, check_override_protocol,
    check_trace,
};

const SUBCOMMAND: &str = "doctor";

/// Fixed deterministic check order. The `doctor_checks_locked` test
/// asserts every entry here is actually invoked by `cmd_doctor` +
/// that its severity category is stable.
const CHECKS: &[(&str, CheckKind)] = &[
    ("trace validity", CheckKind::Error),
    ("floors config", CheckKind::Error),
    ("boundary config", CheckKind::Error),
    ("CI integration", CheckKind::Warning),
    ("merge-style policy", CheckKind::Warning),
    ("override protocol docs", CheckKind::Warning),
];

/// Severity category for a given check's failure.
enum CheckKind {
    Error,
    Warning,
}

/// Entrypoint for `cargo evidence doctor`.
///
/// Runs each check in a fixed deterministic order and renders the
/// result. Streams one `Diagnostic` per check + one terminal
/// (`DOCTOR_OK` / `DOCTOR_FAIL`) in JSONL mode; prints a
/// `[✓]` / `[⚠]` / `[✗]` table in human mode.
///
/// Exit codes: `DOCTOR_OK` → 0, `DOCTOR_FAIL` → 2 (mirrors
/// `verify`'s verification-failure convention). Only error-severity
/// findings flip the exit code; warnings appear in the stream but
/// the command exits 0 so a local `doctor` run isn't blocked by
/// lint-grade findings. Cert-profile `generate` escalates via
/// `precheck_doctor` — see that function for the stricter gate.
pub fn cmd_doctor(json: bool) -> Result<i32> {
    let workspace = std::env::current_dir()?;
    let mut rows: Vec<Row> = Vec::new();
    let mut any_error = false;

    for (name, kind) in CHECKS {
        let result = run_named_check(name, &workspace);
        let row = Row::from_result(name, kind, result);
        if matches!(row.status, Status::Error) {
            any_error = true;
        }
        rows.push(row);
    }

    if json {
        for row in &rows {
            emit_jsonl(&row.to_diagnostic())?;
        }
        let terminal = if any_error {
            terminal_fail()
        } else {
            terminal_ok()
        };
        emit_jsonl(&terminal)?;
    } else {
        render_human(&rows, any_error);
    }

    Ok(if any_error {
        EXIT_VERIFICATION_FAILURE
    } else {
        EXIT_SUCCESS
    })
}

/// Public entrypoint for `generate --profile cert` / `record` to
/// gate bundle assembly on a successful audit. Cert-profile is the
/// high-bar mode — warnings ARE blockers here (a cert bundle
/// produced with no CI integration can't claim "CI verified"; a
/// merge-style gap lets override drift uncaught; an undocumented
/// override protocol invites silent bypass). `cmd_doctor`
/// standalone keeps the dev-friendly split (warnings exit 0);
/// this function blocks on ANY non-pass finding.
///
/// Returns `Err(anyhow!(...))` enumerating the triggered
/// `DOCTOR_*` codes so the caller surfaces them in a
/// `GENERATE_ERROR` terminal without rerunning doctor.
pub fn precheck_doctor(workspace: &Path) -> Result<()> {
    let mut failed_codes: Vec<&'static str> = Vec::new();
    for (name, _kind) in CHECKS {
        if let CheckResult::Fail(code, _) = run_named_check(name, workspace) {
            failed_codes.push(code);
        }
    }
    if failed_codes.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "cargo evidence doctor precheck failed: {} — run `cargo evidence doctor` \
             for details",
            failed_codes.join(", ")
        ))
    }
}

fn run_named_check(name: &str, workspace: &Path) -> CheckResult {
    match name {
        "trace validity" => check_trace(workspace),
        "floors config" => check_floors(workspace),
        "boundary config" => check_boundary(workspace),
        "CI integration" => check_ci_integration(workspace),
        "merge-style policy" => check_merge_style(workspace),
        "override protocol docs" => check_override_protocol(workspace),
        other => CheckResult::Fail("DOCTOR_FAIL", format!("unknown check name '{}'", other)),
    }
}

/// Per-row render state for the two output modes.
struct Row {
    name: String,
    status: Status,
    message: Option<String>,
    code: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Pass,
    Warning,
    Error,
}

impl Row {
    fn from_result(name: &str, kind: &CheckKind, result: CheckResult) -> Self {
        match result {
            CheckResult::Pass => Self {
                name: name.to_string(),
                status: Status::Pass,
                message: None,
                code: None,
            },
            CheckResult::Fail(code, msg) => {
                let status = match kind {
                    CheckKind::Error => Status::Error,
                    CheckKind::Warning => Status::Warning,
                };
                Self {
                    name: name.to_string(),
                    status,
                    message: Some(msg),
                    code: Some(code),
                }
            }
        }
    }

    fn to_diagnostic(&self) -> Diagnostic {
        match self.status {
            Status::Pass => check_passed(&self.name),
            Status::Warning | Status::Error => {
                let severity = if matches!(self.status, Status::Error) {
                    Severity::Error
                } else {
                    Severity::Warning
                };
                Diagnostic {
                    code: self.code.unwrap_or("DOCTOR_FAIL").to_string(),
                    severity,
                    message: self.message.clone().unwrap_or_default(),
                    location: None,
                    fix_hint: None,
                    subcommand: Some(SUBCOMMAND.to_string()),
                    root_cause_uid: None,
                }
            }
        }
    }
}

fn render_human(rows: &[Row], any_error: bool) {
    for row in rows {
        let tag = match row.status {
            Status::Pass => "[✓]",
            Status::Warning => "[⚠]",
            Status::Error => "[✗]",
        };
        match (&row.message, &row.code) {
            (Some(msg), Some(code)) => println!("{} {} ({}): {}", tag, row.name, code, msg),
            _ => println!("{} {}", tag, row.name),
        }
    }
    if any_error {
        println!("\nDOCTOR_FAIL: at least one error-severity check fired");
    } else {
        println!("\nDOCTOR_OK: all error-severity checks passed");
    }
}

/// Outcome of a single check. The check function doesn't emit
/// directly so `precheck_doctor` can reuse it without generating
/// stdout noise during bundle assembly.
pub(super) enum CheckResult {
    Pass,
    Fail(&'static str, String),
}

fn check_passed(name: &str) -> Diagnostic {
    Diagnostic {
        code: "DOCTOR_CHECK_PASSED".to_string(),
        severity: Severity::Info,
        message: format!("{}: ok", name),
        location: None,
        fix_hint: None,
        subcommand: Some(SUBCOMMAND.to_string()),
        root_cause_uid: None,
    }
}

fn terminal_ok() -> Diagnostic {
    Diagnostic {
        code: "DOCTOR_OK".to_string(),
        severity: Severity::Info,
        message: "doctor: all error-severity checks passed".to_string(),
        location: None,
        fix_hint: None,
        subcommand: Some(SUBCOMMAND.to_string()),
        root_cause_uid: None,
    }
}

fn terminal_fail() -> Diagnostic {
    Diagnostic {
        code: "DOCTOR_FAIL".to_string(),
        severity: Severity::Error,
        message: "doctor: at least one error-severity check fired".to_string(),
        location: None,
        fix_hint: None,
        subcommand: Some(SUBCOMMAND.to_string()),
        root_cause_uid: None,
    }
}
