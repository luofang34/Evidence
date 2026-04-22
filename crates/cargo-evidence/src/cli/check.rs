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

use super::args::{CheckMode, EXIT_SUCCESS, EXIT_VERIFICATION_FAILURE, OutputFormat};
use super::output::emit_jsonl;
use super::verify::cmd_verify;

/// `cargo evidence check [--mode=auto|source|bundle] [PATH]`.
///
/// PATH defaults to `.`. See the module docstring for mode semantics
/// and [`resolve_mode`] for the precedence table.
///
/// `format` controls the output shape:
/// - `OutputFormat::Human` (default): per-requirement `[✓]` / `[⚠]`
///   / `[✗]` lines on stdout, phase-progress prose on stderr, final
///   summary line. What a developer sees interactively.
/// - `OutputFormat::Json` or `OutputFormat::Jsonl`: streaming JSONL
///   diagnostics on stdout (one object per line, newline-flushed),
///   terminal diagnostic last. What agents see. Both non-human
///   formats collapse to the same JSONL stream — `check` never had
///   a single-blob JSON shape and adding one is out of scope here.
///
/// `quiet` suppresses the stderr phase-progress markers (`check:
/// running cargo test…` / `check: validating trace…` / `check:
/// aggregating results…`) on the human path. It does not change
/// the stdout output — the per-requirement lines and the final
/// summary are still rendered. Agents pass `--quiet` when running
/// `check` as a subprocess; humans running interactively leave it
/// off to get the "still working" signal during the slow
/// cargo-test phase.
pub fn cmd_check(
    mode: CheckMode,
    path: Option<PathBuf>,
    format: OutputFormat,
    quiet: bool,
) -> Result<i32> {
    let path = path.unwrap_or_else(|| PathBuf::from("."));
    let resolved = resolve_mode(mode, &path);
    match resolved {
        ResolvedMode::Source => cmd_check_source(&path, format, quiet),
        ResolvedMode::Bundle => cmd_check_bundle(path, format),
        ResolvedMode::Invalid(reason) => emit_invalid_argument(&path, &reason, format),
    }
}

/// `true` iff output should be JSONL-streamed (machine mode). Any
/// non-Human format currently collapses here; when/if `check` gains
/// a true single-blob Json mode, split this.
fn is_machine_format(format: OutputFormat) -> bool {
    !matches!(format, OutputFormat::Human)
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

fn emit_invalid_argument(path: &Path, reason: &str, format: OutputFormat) -> Result<i32> {
    let diag = Diagnostic {
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
    };
    let terminal = terminal_check_fail(reason);
    if is_machine_format(format) {
        emit_jsonl(&diag)?;
        emit_jsonl(&terminal)?;
    } else {
        render_human_diagnostics(std::slice::from_ref(&diag), &terminal);
    }
    Ok(EXIT_VERIFICATION_FAILURE)
}

fn cmd_check_bundle(path: PathBuf, format: OutputFormat) -> Result<i32> {
    // Pipe through cmd_verify preserving the caller's format
    // choice. Bundle mode is a thin passthrough — human-mode
    // check on a bundle shows verify's human output; jsonl-mode
    // check gets verify's JSONL stream byte-for-byte.
    cmd_verify(path, false, None, format)
}

fn cmd_check_source(workspace_root: &Path, format: OutputFormat, quiet: bool) -> Result<i32> {
    let machine = is_machine_format(format);
    // Emit phase-progress markers only on the human path AND only
    // when the user hasn't asked for silence. Agents (JSONL) get
    // stderr-silent per Schema Rule 2; humans passing `--quiet`
    // explicitly asked for the same treatment (e.g. scripts
    // wrapping `check` that already drive their own progress bar).
    let show_progress = !machine && !quiet;

    // Phase 1: run `cargo test --workspace --no-fail-fast`, capture
    // stdout. We explicitly don't --format=json (that's nightly-only
    // and would force a toolchain bump). This is the slow phase —
    // on a ~30-crate workspace it can take minutes.
    if show_progress {
        eprintln!("check: running `cargo test --workspace`…");
    }
    let test_stdout = run_cargo_test(workspace_root)?;

    // Phase 2: parse into per-test outcome map.
    let Some((_summary, outcomes)) = parse_cargo_test_output_with_outcomes(&test_stdout) else {
        return emit_invalid_argument(
            workspace_root,
            "cargo test produced no parseable `test result:` line \
             (is this a Rust workspace with testable crates?)",
            format,
        );
    };

    // Phase 3: load trace. Discovery picks tool/trace → cert/trace per
    // LLR-023; fall back to cert/boundary.toml otherwise.
    if show_progress {
        eprintln!("check: validating trace…");
    }
    let trace_root = super::trace::default_trace_roots()
        .into_iter()
        .next()
        .unwrap_or_else(|| "cert/trace".to_string());
    let trace = read_all_trace_files(&trace_root)
        .with_context(|| format!("reading trace files under '{}'", trace_root))?;

    // Phase 4: policy. DAL-derived default + same `require_hlr_sys_trace`
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

    // Phase 5: build + emit per-requirement diagnostics.
    if show_progress {
        eprintln!("check: aggregating results…");
    }
    let diagnostics = build_requirement_report(&trace, &outcomes, workspace_root, &policy);
    let any_gap = diagnostics.iter().any(|d| d.code == "REQ_GAP");

    let terminal = if any_gap {
        terminal_check_fail(&format!(
            "{} requirement(s) currently in GAP",
            diagnostics.iter().filter(|d| d.code == "REQ_GAP").count()
        ))
    } else {
        terminal_check_ok(&format!(
            "{} requirement(s) satisfied",
            diagnostics.iter().filter(|d| d.code == "REQ_PASS").count()
        ))
    };

    if machine {
        for diag in &diagnostics {
            emit_jsonl(diag)?;
        }
        emit_jsonl(&terminal)?;
    } else {
        render_human_diagnostics(&diagnostics, &terminal);
    }

    if any_gap {
        Ok(EXIT_VERIFICATION_FAILURE)
    } else {
        Ok(EXIT_SUCCESS)
    }
}

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
fn render_human_diagnostics(diagnostics: &[Diagnostic], terminal: &Diagnostic) {
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
