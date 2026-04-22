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
//! The underlying data lives in [`evidence_core::RULES`] and is pinned by
//! four bijection invariants in `diagnostic_codes_locked`. MCP (PR
//! #50) wraps the `--json` shape directly.

use anyhow::Result;

use super::args::EXIT_SUCCESS;
use super::output::emit_json;
use evidence_core::RULES;

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

fn severity_label(s: evidence_core::Severity) -> &'static str {
    match s {
        evidence_core::Severity::Info => "info",
        evidence_core::Severity::Warning => "warning",
        evidence_core::Severity::Error => "error",
    }
}

fn domain_label(d: evidence_core::Domain) -> &'static str {
    match d {
        evidence_core::Domain::Boundary => "boundary",
        evidence_core::Domain::Bundle => "bundle",
        evidence_core::Domain::Check => "check",
        evidence_core::Domain::Cli => "cli",
        evidence_core::Domain::Cmd => "cmd",
        evidence_core::Domain::Doctor => "doctor",
        evidence_core::Domain::Env => "env",
        evidence_core::Domain::Floors => "floors",
        evidence_core::Domain::Git => "git",
        evidence_core::Domain::Hash => "hash",
        evidence_core::Domain::Policy => "policy",
        evidence_core::Domain::Req => "req",
        evidence_core::Domain::Schema => "schema",
        evidence_core::Domain::Sign => "sign",
        evidence_core::Domain::Trace => "trace",
        evidence_core::Domain::Verify => "verify",
    }
}
