//! `cargo evidence context [<selector>] [--crate|--module] [--json|--format=jsonl]`
//!
//! Returns the per-module trace + boundary + floors + `CLAUDE.md`
//! slice an agent needs before editing a file. Pure inspection —
//! never spawns `cargo test`, never writes to disk.
//!
//! Three output shapes (mirroring `floors` / `rules`):
//!
//! - **human** (default): a compact summary table.
//! - **json** (`--json` / `--format=json`): the full
//!   [`ContextReport`] as pretty JSON on stdout.
//! - **jsonl** (`--format=jsonl`): the report serialized as a
//!   single JSON object on the first line, followed by one
//!   `CONTEXT_*` warning per warning on the report, and a
//!   `CONTEXT_OK` / `CONTEXT_FAIL` / `CONTEXT_ERROR` terminal.
//!
//! Exit codes:
//!
//! - `0` — `CONTEXT_OK` (report built; or the graceful
//!   `CONTEXT_NO_TRACE_CONFIGURED` info path for non-adopters).
//! - `1` — `CONTEXT_ERROR` (runtime / I/O failure).
//! - `2` — `CONTEXT_FAIL` (selector invalid or out-of-scope).

use std::path::PathBuf;

use anyhow::Result;

use evidence_core::context::{
    ContextError, ContextReport, ContextWarning, context_for, resolve_selector,
};
use evidence_core::diagnostic::{Diagnostic, Severity};

use super::args::{EXIT_ERROR, EXIT_SUCCESS, EXIT_VERIFICATION_FAILURE, OutputFormat};
use super::output::{emit_json, emit_jsonl};

/// Entrypoint for `cargo evidence context`.
pub fn cmd_context(
    positional: Option<String>,
    crate_flag: Option<String>,
    module_flag: Option<String>,
    format: OutputFormat,
) -> Result<i32> {
    let workspace = std::env::current_dir()?;
    let raw = pick_selector_input(positional, crate_flag, module_flag);

    let selector = match resolve_selector(&workspace, raw.as_deref()) {
        Ok(s) => s,
        Err(err) => return handle_resolver_error(err, format),
    };

    let report = match context_for(&workspace, &selector) {
        Ok(r) => r,
        Err(ContextError::TraceNotConfigured(path)) => {
            return handle_trace_not_configured(path, format);
        }
        Err(err) => return handle_runtime_error(err, format),
    };

    render(&report, format)
}

fn pick_selector_input(
    positional: Option<String>,
    crate_flag: Option<String>,
    module_flag: Option<String>,
) -> Option<String> {
    if let Some(p) = positional.filter(|p| !p.is_empty()) {
        return Some(p);
    }
    if let Some(c) = crate_flag.filter(|c| !c.is_empty()) {
        return Some(c);
    }
    if let Some(m) = module_flag.filter(|m| !m.is_empty()) {
        return Some(m);
    }
    None
}

fn render(report: &ContextReport, format: OutputFormat) -> Result<i32> {
    match format {
        OutputFormat::Jsonl => emit_jsonl_stream(report),
        OutputFormat::Json => {
            emit_json(report)?;
            Ok(EXIT_SUCCESS)
        }
        OutputFormat::Human => {
            print_human(report);
            Ok(EXIT_SUCCESS)
        }
    }
}

fn emit_jsonl_stream(report: &ContextReport) -> Result<i32> {
    // The report itself goes out as the first line so an agent reading
    // jsonl sees the structured blob before any per-warning diagnostic.
    // Use the same emit_jsonl helper as everywhere else so the per-line
    // flush stays consistent.
    let report_line = serde_json::to_string(report)?;
    let stdout = std::io::stdout();
    {
        use std::io::Write;
        let mut h = stdout.lock();
        writeln!(h, "{}", report_line)?;
        h.flush()?;
    }
    for warning in &report.warnings {
        emit_jsonl(&warning_to_diag(warning))?;
    }
    emit_jsonl(&terminal_ok(format!(
        "context resolved for {} ({})",
        report.selector.kind,
        if report.selector.resolved.is_empty() {
            "<workspace>".to_string()
        } else {
            report.selector.resolved.clone()
        }
    )))?;
    Ok(EXIT_SUCCESS)
}

fn warning_to_diag(w: &ContextWarning) -> Diagnostic {
    Diagnostic {
        code: w.code.clone(),
        severity: Severity::Warning,
        message: w.message.clone(),
        location: None,
        fix_hint: None,
        subcommand: Some("context".to_string()),
        root_cause_uid: None,
    }
}

fn terminal(code: &'static str, severity: Severity, message: String) -> Diagnostic {
    Diagnostic {
        code: code.to_string(),
        severity,
        message,
        location: None,
        fix_hint: None,
        subcommand: Some("context".to_string()),
        root_cause_uid: None,
    }
}

fn terminal_ok(msg: String) -> Diagnostic {
    terminal("CONTEXT_OK", Severity::Info, msg)
}

fn terminal_fail(msg: String) -> Diagnostic {
    terminal("CONTEXT_FAIL", Severity::Error, msg)
}

fn terminal_error(msg: String) -> Diagnostic {
    terminal("CONTEXT_ERROR", Severity::Error, msg)
}

fn handle_resolver_error(err: ContextError, format: OutputFormat) -> Result<i32> {
    let code = err.content_code();
    let msg = err.to_string();
    if format == OutputFormat::Jsonl {
        emit_jsonl(&Diagnostic {
            code: code.to_string(),
            severity: Severity::Error,
            message: msg.clone(),
            location: None,
            fix_hint: None,
            subcommand: Some("context".to_string()),
            root_cause_uid: None,
        })?;
        emit_jsonl(&terminal_fail(msg))?;
    } else {
        eprintln!("error: {}", msg);
    }
    Ok(EXIT_VERIFICATION_FAILURE)
}

fn handle_trace_not_configured(path: PathBuf, format: OutputFormat) -> Result<i32> {
    let msg = format!(
        "no trace configured at {} — context surface is not configured for this project",
        path.display()
    );
    if format == OutputFormat::Jsonl {
        emit_jsonl(&Diagnostic {
            code: "CONTEXT_NO_TRACE_CONFIGURED".to_string(),
            severity: Severity::Info,
            message: msg.clone(),
            location: None,
            fix_hint: None,
            subcommand: Some("context".to_string()),
            root_cause_uid: None,
        })?;
        emit_jsonl(&terminal_ok(msg))?;
    } else if format == OutputFormat::Json {
        emit_json(&ContextReport::workspace_default())?;
    } else {
        eprintln!("info: {}", msg);
    }
    Ok(EXIT_SUCCESS)
}

fn handle_runtime_error(err: ContextError, format: OutputFormat) -> Result<i32> {
    let msg = err.to_string();
    if format == OutputFormat::Jsonl {
        emit_jsonl(&Diagnostic {
            code: err.content_code().to_string(),
            severity: Severity::Error,
            message: msg.clone(),
            location: None,
            fix_hint: None,
            subcommand: Some("context".to_string()),
            root_cause_uid: None,
        })?;
        emit_jsonl(&terminal_error(msg))?;
    } else {
        eprintln!("error: {}", msg);
    }
    Ok(EXIT_ERROR)
}

fn print_human(report: &ContextReport) {
    println!(
        "selector: {} ({})",
        report.selector.kind,
        if report.selector.resolved.is_empty() {
            "<workspace>".to_string()
        } else {
            report.selector.resolved.clone()
        }
    );
    println!(
        "crate:    {}",
        if report.crate_name.is_empty() {
            "<workspace>".to_string()
        } else {
            report.crate_name.clone()
        }
    );
    println!("dal:      {}", report.dal);
    println!();
    println!(
        "requirements ({}): {}",
        report.requirements.len(),
        ids_summary(report.requirements.iter().map(|r| r.id.as_str()))
    );
    println!(
        "parents      ({}): {}",
        report.parents.len(),
        ids_summary(report.parents.iter().map(|p| p.id.as_str()))
    );
    println!(
        "tests        ({}): {}",
        report.tests.len(),
        ids_summary(report.tests.iter().map(|t| t.id.as_str()))
    );
    println!(
        "codes        ({}): {}",
        report.diagnostic_codes.len(),
        ids_summary(report.diagnostic_codes.iter().map(String::as_str))
    );
    println!("floors       ({} row(s))", report.floors.len());
    for f in &report.floors {
        println!(
            "  {}/{}: current={} limit={}",
            f.kind, f.dimension, f.current, f.floor
        );
    }
    println!(
        "boundary:    in_scope={} forbidden_deps={}",
        report.boundary.in_scope,
        report.boundary.forbidden_deps.len()
    );
    println!(
        "conventions: nearest_claude_md={}",
        report
            .conventions
            .nearest_claude_md
            .as_deref()
            .unwrap_or("<none>")
    );
    if !report.warnings.is_empty() {
        println!();
        println!("warnings ({}):", report.warnings.len());
        for w in &report.warnings {
            println!("  [{}] {}", w.code, w.message);
        }
    }
}

fn ids_summary<'a>(iter: impl Iterator<Item = &'a str>) -> String {
    let mut v: Vec<&str> = iter.collect();
    if v.is_empty() {
        return "<none>".to_string();
    }
    let max = 6;
    if v.len() > max {
        let tail = format!("(+{} more)", v.len() - max);
        v.truncate(max);
        let mut joined = v.join(", ");
        joined.push(' ');
        joined.push_str(&tail);
        joined
    } else {
        v.join(", ")
    }
}
