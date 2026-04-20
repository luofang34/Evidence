//! `cargo evidence check` — agent-facing one-shot validation.
//!
//! Dispatches on argument shape (auto mode) or explicit `--mode`:
//!
//! - **Source mode.** Runs `cargo test --workspace --no-fail-fast`,
//!   parses stdout into a per-test outcome map, walks `tool/trace/`
//!   (or `cert/trace/` via discovery), emits one
//!   `REQ_PASS` / `REQ_GAP` / `REQ_SKIP` diagnostic per requirement.
//!   Each `REQ_GAP` for a derived failure carries `root_cause_uid`;
//!   each mechanical `REQ_GAP` carries a `FixHint`.
//! - **Bundle mode.** Delegates to `cmd_verify_jsonl` via the same
//!   machinery `verify --format=jsonl` uses. Wire shape is literally
//!   identical so a script that already consumes `verify --format=jsonl`
//!   keeps working.
//!
//! Agents should always call `check`; scripts and debuggers can still
//! call `verify` directly (the low-level primitive).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use evidence_core::bundle::parse_cargo_test_output_with_outcomes;
use evidence_core::diagnostic::{Diagnostic, Severity};
use evidence_core::policy::TracePolicy;
use evidence_core::trace::{build_requirement_report, read_all_trace_files};
use evidence_core::{BoundaryConfig, EvidencePolicy};

use super::args::{CheckMode, EXIT_SUCCESS, EXIT_VERIFICATION_FAILURE};
use super::output::emit_jsonl;
use super::verify::cmd_verify;

/// `cargo evidence check [--mode=auto|source|bundle] [PATH]`.
///
/// PATH defaults to `.`. See the module docstring for mode semantics
/// and [`resolve_mode`] for the precedence table.
pub fn cmd_check(mode: CheckMode, path: Option<PathBuf>) -> Result<i32> {
    let path = path.unwrap_or_else(|| PathBuf::from("."));
    let resolved = resolve_mode(mode, &path);
    match resolved {
        ResolvedMode::Source => cmd_check_source(&path),
        ResolvedMode::Bundle => cmd_check_bundle(path),
        ResolvedMode::Invalid(reason) => emit_invalid_argument(&path, &reason),
    }
}

enum ResolvedMode {
    Source,
    Bundle,
    Invalid(String),
}

fn resolve_mode(mode: CheckMode, path: &Path) -> ResolvedMode {
    let has_sha = path.join("SHA256SUMS").is_file();
    let has_cargo = path.join("Cargo.toml").is_file();
    match mode {
        CheckMode::Auto => {
            if has_sha {
                ResolvedMode::Bundle
            } else if has_cargo {
                ResolvedMode::Source
            } else {
                ResolvedMode::Invalid(format!(
                    "path '{}' has neither SHA256SUMS nor Cargo.toml — not a bundle or source tree",
                    path.display()
                ))
            }
        }
        CheckMode::Source => {
            if has_cargo {
                ResolvedMode::Source
            } else if has_sha {
                ResolvedMode::Invalid(format!(
                    "--mode=source but path '{}' is a bundle (has SHA256SUMS, no Cargo.toml)",
                    path.display()
                ))
            } else {
                ResolvedMode::Invalid(format!(
                    "--mode=source but path '{}' has no Cargo.toml",
                    path.display()
                ))
            }
        }
        CheckMode::Bundle => {
            if has_sha {
                ResolvedMode::Bundle
            } else if has_cargo {
                ResolvedMode::Invalid(format!(
                    "--mode=bundle but path '{}' is a source tree (has Cargo.toml, no SHA256SUMS)",
                    path.display()
                ))
            } else {
                ResolvedMode::Invalid(format!(
                    "--mode=bundle but path '{}' has no SHA256SUMS",
                    path.display()
                ))
            }
        }
    }
}

fn emit_invalid_argument(path: &Path, reason: &str) -> Result<i32> {
    emit_jsonl(&Diagnostic {
        code: "CLI_INVALID_ARGUMENT".to_string(),
        severity: Severity::Error,
        message: reason.to_string(),
        location: Some(evidence_core::diagnostic::Location {
            file: Some(path.to_path_buf()),
            ..evidence_core::diagnostic::Location::default()
        }),
        fix_hint: None,
        subcommand: Some("check".to_string()),
        root_cause_uid: None,
    })?;
    emit_jsonl(&terminal_check_fail(reason))?;
    Ok(EXIT_VERIFICATION_FAILURE)
}

fn cmd_check_bundle(path: PathBuf) -> Result<i32> {
    // Pipe through cmd_verify in JSONL format. Bundle mode is a
    // passthrough by design — one command, one wire shape for agents.
    cmd_verify(path, false, None, super::args::OutputFormat::Jsonl)
}

fn cmd_check_source(workspace_root: &Path) -> Result<i32> {
    // Step 1: run `cargo test --workspace --no-fail-fast`, capture
    // stdout. We explicitly don't --format=json (that's nightly-only
    // and would force a toolchain bump).
    let test_stdout = run_cargo_test(workspace_root)?;

    // Step 2: parse into per-test outcome map.
    let Some((_summary, outcomes)) = parse_cargo_test_output_with_outcomes(&test_stdout) else {
        return emit_invalid_argument(
            workspace_root,
            "cargo test produced no parseable `test result:` line \
             (is this a Rust workspace with testable crates?)",
        );
    };

    // Step 3: load trace. Discovery picks tool/trace → cert/trace per
    // LLR-023; fall back to cert/boundary.toml otherwise.
    let trace_root = super::trace::default_trace_roots()
        .into_iter()
        .next()
        .unwrap_or_else(|| "cert/trace".to_string());
    let trace = read_all_trace_files(&trace_root)
        .with_context(|| format!("reading trace files under '{}'", trace_root))?;

    // Step 4: policy. DAL-derived default + same `require_hlr_sys_trace`
    // behavior as `trace --validate` so `check` enforces the same
    // contract.
    let boundary = BoundaryConfig::load_or_default(&PathBuf::from("cert/boundary.toml"));
    let dal = boundary
        .dal_map()
        .values()
        .copied()
        .max()
        .unwrap_or_default();
    let mut policy = EvidencePolicy::for_dal(dal).trace;
    policy.require_hlr_sys_trace = true;
    policy.require_hlr_surface_bijection = true;

    // Step 5: build + emit per-requirement diagnostics.
    let diagnostics = build_requirement_report(&trace, &outcomes, workspace_root, &policy);
    let mut any_gap = false;
    for diag in &diagnostics {
        if diag.code == "REQ_GAP" {
            any_gap = true;
        }
        emit_jsonl(diag)?;
    }

    // Step 6: terminal.
    if any_gap {
        emit_jsonl(&terminal_check_fail(&format!(
            "{} requirement(s) currently in GAP",
            diagnostics.iter().filter(|d| d.code == "REQ_GAP").count()
        )))?;
        Ok(EXIT_VERIFICATION_FAILURE)
    } else {
        emit_jsonl(&terminal_check_ok(&format!(
            "{} requirement(s) satisfied",
            diagnostics.iter().filter(|d| d.code == "REQ_PASS").count()
        )))?;
        Ok(EXIT_SUCCESS)
    }
}

fn run_cargo_test(workspace_root: &Path) -> Result<String> {
    // Running `cargo test` from a subprocess of `cargo test` (as would
    // happen in our own integration tests) risks target/ lock
    // contention. Caller is responsible for arranging a sensible
    // invocation context; the integration tests of `check` itself
    // call `build_requirement_report` directly rather than spawning
    // cargo. See commit 4's tests.
    //
    // Force colorless output: GitHub Actions sets
    // `CARGO_TERM_COLOR=always` at the workflow level, which wraps
    // cargo's `Running target/debug/deps/<binary>` headers in ANSI
    // escape codes. The parser's `starts_with("Running ")` filter then
    // misses every binary name, keys every test under
    // `__unknown_binary__::<fn>`, and every requirement silently
    // reports `REQ_GAP`. Setting both `CARGO_TERM_COLOR=never` and
    // `NO_COLOR=1` neutralizes cargo's and libtest's color paths.
    let out = Command::new("cargo")
        .arg("test")
        .arg("--workspace")
        .arg("--no-fail-fast")
        .env("CARGO_TERM_COLOR", "never")
        .env("NO_COLOR", "1")
        .current_dir(workspace_root)
        .output()
        .with_context(|| format!("spawning `cargo test` in {}", workspace_root.display()))?;
    let mut buf = String::new();
    buf.push_str(&String::from_utf8_lossy(&out.stdout));
    buf.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(buf)
}

fn terminal_check_ok(message: &str) -> Diagnostic {
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

fn terminal_check_fail(message: &str) -> Diagnostic {
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

// Clippy lint-discharge: TracePolicy is referenced via field access
// only — silence unused-import warnings without removing the use (it
// improves readability of cmd_check_source).
#[allow(dead_code)]
fn _policy_reference(_p: &TracePolicy) {}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn resolve_auto_picks_bundle_when_sha256sums_present() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("SHA256SUMS"), "").unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
        assert!(matches!(
            resolve_mode(CheckMode::Auto, tmp.path()),
            ResolvedMode::Bundle
        ));
    }

    #[test]
    fn resolve_auto_picks_source_when_only_cargo_toml() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
        assert!(matches!(
            resolve_mode(CheckMode::Auto, tmp.path()),
            ResolvedMode::Source
        ));
    }

    #[test]
    fn resolve_auto_rejects_empty_dir() {
        let tmp = TempDir::new().unwrap();
        match resolve_mode(CheckMode::Auto, tmp.path()) {
            ResolvedMode::Invalid(msg) => assert!(msg.contains("neither")),
            other => panic!("expected Invalid, got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn resolve_source_on_bundle_is_invalid() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("SHA256SUMS"), "").unwrap();
        assert!(matches!(
            resolve_mode(CheckMode::Source, tmp.path()),
            ResolvedMode::Invalid(_)
        ));
    }

    #[test]
    fn resolve_bundle_on_source_is_invalid() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
        assert!(matches!(
            resolve_mode(CheckMode::Bundle, tmp.path()),
            ResolvedMode::Invalid(_)
        ));
    }
}
