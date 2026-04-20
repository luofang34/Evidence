//! `cargo evidence rules [--json]` — self-describe the tool's diagnostic
//! vocabulary.
//!
//! In JSON mode this command writes a single JSON array (sorted
//! alphabetically by code) to stdout. In human mode it prints a
//! fixed-width table. Neither mode emits JSONL — the subcommand has
//! no streaming terminal and is explicitly rejected when the user
//! passes `--format=jsonl` (handled by the dispatch guard in
//! `main.rs`).
//!
//! The underlying data lives in [`evidence::RULES`] and is pinned by
//! four bijection invariants in `diagnostic_codes_locked`. MCP (PR
//! #50) wraps the `--json` shape directly.

use anyhow::Result;

use super::args::EXIT_SUCCESS;
use super::output::emit_json;
use evidence::RULES;

/// Entrypoint for `cargo evidence rules`.
pub fn cmd_rules(json: bool) -> Result<i32> {
    if json {
        emit_rules_json()
    } else {
        emit_rules_human()
    }
}

fn emit_rules_json() -> Result<i32> {
    emit_json(&RULES)?;
    Ok(EXIT_SUCCESS)
}

/// Human-readable fixed-width table. Columns: code, severity, domain,
/// terminal?, has_fix_hint?. The output is plain text (no ANSI
/// colors) so `| less` stays readable.
fn emit_rules_human() -> Result<i32> {
    // Compute column widths.
    let code_w = RULES.iter().map(|r| r.code.len()).max().unwrap_or(4).max(4);

    println!(
        "{:<code_w$}  {:<8}  {:<10}  {:<8}  fix_hint",
        "CODE",
        "SEVERITY",
        "DOMAIN",
        "TERMINAL",
        code_w = code_w
    );
    println!(
        "{:-<code_w$}  {:-<8}  {:-<10}  {:-<8}  {:-<8}",
        "",
        "",
        "",
        "",
        "",
        code_w = code_w
    );
    for r in RULES {
        println!(
            "{:<code_w$}  {:<8}  {:<10}  {:<8}  {}",
            r.code,
            severity_label(r.severity),
            domain_label(r.domain),
            if r.terminal { "yes" } else { "-" },
            if r.has_fix_hint { "yes" } else { "-" },
            code_w = code_w,
        );
    }
    println!();
    println!("{} rule(s) total.", RULES.len());
    Ok(EXIT_SUCCESS)
}

fn severity_label(s: evidence::Severity) -> &'static str {
    match s {
        evidence::Severity::Info => "info",
        evidence::Severity::Warning => "warning",
        evidence::Severity::Error => "error",
    }
}

fn domain_label(d: evidence::Domain) -> &'static str {
    match d {
        evidence::Domain::Boundary => "boundary",
        evidence::Domain::Bundle => "bundle",
        evidence::Domain::Cli => "cli",
        evidence::Domain::Cmd => "cmd",
        evidence::Domain::Doctor => "doctor",
        evidence::Domain::Env => "env",
        evidence::Domain::Floors => "floors",
        evidence::Domain::Git => "git",
        evidence::Domain::Hash => "hash",
        evidence::Domain::Policy => "policy",
        evidence::Domain::Req => "req",
        evidence::Domain::Schema => "schema",
        evidence::Domain::Sign => "sign",
        evidence::Domain::Trace => "trace",
        evidence::Domain::Verify => "verify",
    }
}
