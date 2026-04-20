//! `cargo evidence verify`.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Serialize;

use evidence::diagnostic::{Diagnostic, DiagnosticCode, Severity};
use evidence::verify::VerifyRuntimeError;
use evidence::{VerifyResult, verify_bundle_with_key};

use super::args::{EXIT_ERROR, EXIT_SUCCESS, EXIT_VERIFICATION_FAILURE, OutputFormat};
use super::output::{emit_json, emit_jsonl};

#[derive(Serialize)]
struct VerifyOutput {
    success: bool,
    bundle_path: String,
    checks: Vec<VerifyCheck>,
    error: Option<String>,
}

#[derive(Serialize)]
struct VerifyCheck {
    name: String,
    status: String,
    message: Option<String>,
}

/// Emit a verify failure envelope and return the given exit code.
///
/// Unifies the pattern that used to repeat five times in cmd_verify:
/// build a `VerifyOutput { success: false, ... error: Some(msg) }`,
/// emit JSON on --json or print a text line with the given prefix to
/// stderr (so e.g. "verify: FAIL - foo" vs "error: foo" vs "verify:
/// ERROR - foo" are all one call site). The caller controls the
/// text prefix and exit code.
fn fail_verify(
    json_output: bool,
    bundle_path: &std::path::Path,
    checks: Vec<VerifyCheck>,
    text_prefix: &str,
    msg: impl Into<String>,
    exit_code: i32,
) -> Result<i32> {
    let msg = msg.into();
    if json_output {
        emit_json(&VerifyOutput {
            success: false,
            bundle_path: bundle_path.display().to_string(),
            checks,
            error: Some(msg),
        })?;
    } else {
        eprintln!("{} {}", text_prefix, msg);
    }
    Ok(exit_code)
}

/// `cargo evidence verify` handler — the **low-level primitive**
/// for bundle integrity + policy checks.
///
/// Runs every integrity + policy check on `bundle_path` and emits a
/// per-check pass/fail report. Returns [`EXIT_VERIFICATION_FAILURE`]
/// when any check fails (or when any warning fires in `--strict`
/// mode), and [`EXIT_ERROR`] only when the tool itself can't run —
/// e.g. the bundle directory is missing.
///
/// **Agents and humans should prefer `cargo evidence check`**
/// ([`crate::cli::check::cmd_check`]) as the default entry point:
/// it auto-detects whether the argument is a source tree or bundle,
/// emits per-requirement `REQ_*` diagnostics in source mode, and
/// carries `FixHint` variants for mechanical autofix. `verify`
/// remains supported for CI scripts and shell pipelines that want a
/// stable bundle-only surface. MCP wraps `check`; `verify`
/// is deliberately not exposed over MCP to avoid agents picking
/// between two commands that overlap in bundle mode.
pub fn cmd_verify(
    bundle_path: PathBuf,
    strict: bool,
    verify_key: Option<PathBuf>,
    format: OutputFormat,
) -> Result<i32> {
    // Jsonl takes a dedicated streaming path so each finding flushes
    // per-line and a terminal `VERIFY_OK` / `VERIFY_FAIL` event lands
    // last — Schema Rules 1, 2, 4. Human and Json both round up to
    // the legacy single-envelope shape.
    if format == OutputFormat::Jsonl {
        return cmd_verify_jsonl(bundle_path, strict, verify_key);
    }
    let json_output = format == OutputFormat::Json;
    let mut checks = Vec::new();

    // Check bundle exists
    if !bundle_path.exists() {
        let err_msg = format!("bundle not found: {:?}", bundle_path);
        return fail_verify(
            json_output,
            &bundle_path,
            vec![VerifyCheck {
                name: "bundle_exists".to_string(),
                status: "fail".to_string(),
                message: Some(err_msg.clone()),
            }],
            "error:",
            err_msg,
            EXIT_VERIFICATION_FAILURE,
        );
    }

    checks.push(VerifyCheck {
        name: "bundle_exists".to_string(),
        status: "pass".to_string(),
        message: None,
    });

    // Strict mode: require BUNDLE.sig to exist
    if strict && !bundle_path.join("BUNDLE.sig").exists() && verify_key.is_none() {
        let err_msg = "strict mode: BUNDLE.sig not found and no --verify-key provided";
        return fail_verify(
            json_output,
            &bundle_path,
            vec![VerifyCheck {
                name: "bundle_signature".to_string(),
                status: "fail".to_string(),
                message: Some(err_msg.to_string()),
            }],
            "verify: FAIL -",
            err_msg,
            EXIT_VERIFICATION_FAILURE,
        );
    }

    // Load verify key if provided
    let key_bytes = match &verify_key {
        Some(path) => {
            Some(fs::read(path).with_context(|| format!("reading verify key from {:?}", path))?)
        }
        None => None,
    };

    // Run verification
    match verify_bundle_with_key(&bundle_path, key_bytes.as_deref()) {
        Ok(VerifyResult::Pass) => {
            checks.push(VerifyCheck {
                name: "bundle_integrity".to_string(),
                status: "pass".to_string(),
                message: None,
            });
            checks.push(VerifyCheck {
                name: "sha256sums".to_string(),
                status: "pass".to_string(),
                message: None,
            });

            if json_output {
                emit_json(&VerifyOutput {
                    success: true,
                    bundle_path: bundle_path.display().to_string(),
                    checks,
                    error: None,
                })?;
            } else {
                println!("verify: PASS - bundle {:?}", bundle_path);
            }
            Ok(EXIT_SUCCESS)
        }
        Ok(VerifyResult::Fail(errors)) => {
            let reason = errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            // Map each VerifyError to its own VerifyCheck for granular JSON output
            for err in &errors {
                let name = match err {
                    evidence::VerifyError::UnexpectedFile(_) => "unexpected_file",
                    evidence::VerifyError::HmacFailure => "hmac_signature",
                    evidence::VerifyError::HashMismatch { .. } => "hash_mismatch",
                    evidence::VerifyError::MissingHashedFile(_) => "missing_file",
                    evidence::VerifyError::ContentHashMismatch { .. } => "content_hash",
                    evidence::VerifyError::UnsafePath(_) => "unsafe_path",
                    evidence::VerifyError::FormatError { .. } => "format_error",
                    evidence::VerifyError::CrossFileInconsistency { .. } => "cross_file_mismatch",
                    evidence::VerifyError::DeterministicHashMismatch { .. } => "deterministic_hash",
                    evidence::VerifyError::ManifestProjectionDrift { .. } => "manifest_projection",
                    evidence::VerifyError::TraceOutputNotHashed(_) => "trace_output_not_hashed",
                    evidence::VerifyError::TestSummaryMismatch { .. } => "test_summary_mismatch",
                    evidence::VerifyError::DalMapMismatch { .. } => "dal_map_mismatch",
                    evidence::VerifyError::DalMapOrphan { .. } => "dal_map_orphan",
                };
                checks.push(VerifyCheck {
                    name: name.to_string(),
                    status: "fail".to_string(),
                    message: Some(err.to_string()),
                });
            }

            fail_verify(
                json_output,
                &bundle_path,
                checks,
                "verify: FAIL -",
                reason,
                EXIT_VERIFICATION_FAILURE,
            )
        }
        Ok(VerifyResult::Skipped(reason)) => {
            // In strict mode, skipped checks are treated as failures
            let treat_as_fail = strict;
            checks.push(VerifyCheck {
                name: "bundle_integrity".to_string(),
                status: if treat_as_fail { "fail" } else { "skipped" }.to_string(),
                message: Some(reason.clone()),
            });

            if json_output {
                emit_json(&VerifyOutput {
                    success: !treat_as_fail,
                    bundle_path: bundle_path.display().to_string(),
                    checks,
                    error: if treat_as_fail {
                        Some(format!("strict mode: {}", reason))
                    } else {
                        None
                    },
                })?;
            } else if treat_as_fail {
                eprintln!("verify: FAIL (strict) - {}", reason);
            } else {
                println!("verify: SKIPPED - {}", reason);
            }
            Ok(if treat_as_fail {
                EXIT_VERIFICATION_FAILURE
            } else {
                EXIT_SUCCESS
            })
        }
        Err(e) => fail_verify(
            json_output,
            &bundle_path,
            checks,
            "verify: ERROR -",
            e.to_string(),
            EXIT_VERIFICATION_FAILURE,
        ),
    }
}

/// Stdout-strict JSON-Lines path for `--format=jsonl`.
///
/// Each finding and the terminal event are serialized as a compact JSON
/// object on their own line, flushed per-event (Schema Rule 4). The
/// LAST line emitted is always the terminal event whose `code` ends in
/// `_OK` or `_FAIL` — Schema Rule 1 makes that the contract for the
/// matching exit code. Runtime errors (bundle-not-found, hash I/O,
/// parse failures) emit a per-error diagnostic but NO terminal event,
/// and the process returns exit 1 — also per Schema Rule 1.
fn cmd_verify_jsonl(
    bundle_path: PathBuf,
    strict: bool,
    verify_key: Option<PathBuf>,
) -> Result<i32> {
    // Bundle-path existence failure is a runtime fault: emit the
    // VerifyRuntimeError::BundleNotFound finding first, then the
    // `VERIFY_ERROR` terminal (Schema Rule 1 — every run emits
    // exactly one terminal so truncation is detectable).
    if !bundle_path.exists() {
        emit_jsonl(&VerifyRuntimeError::BundleNotFound(bundle_path.clone()).to_diagnostic())?;
        emit_jsonl(&terminal_error(&format!(
            "bundle path does not exist: {}",
            bundle_path.display()
        )))?;
        return Ok(EXIT_ERROR);
    }

    // Strict mode: missing signature is a *finding* (the bundle exists
    // but doesn't meet the strict-mode contract), so it gets a terminal
    // `_FAIL` and exit 2.
    if strict && !bundle_path.join("BUNDLE.sig").exists() && verify_key.is_none() {
        emit_jsonl(&Diagnostic {
            code: "VERIFY_STRICT_SIGNATURE_MISSING".to_string(),
            severity: Severity::Error,
            message: "strict mode: BUNDLE.sig not found and no --verify-key provided".to_string(),
            location: None,
            fix_hint: None,
            subcommand: None,
            root_cause_uid: None,
        })?;
        emit_jsonl(&terminal_fail("bundle failed strict signature requirement"))?;
        return Ok(EXIT_VERIFICATION_FAILURE);
    }

    // Load verify key. `fs::read` failure here is a runtime fault (the
    // caller's key file is missing / unreadable) — anyhow's `?` with
    // `with_context` bubbles it up as an `Err(anyhow)`, which main's
    // `run` prints to stderr and returns exit 1. No JSONL surfacing
    // for key-file I/O is intentional: the error precedes verify, so
    // there is no bundle-level diagnostic to correlate with.
    let key_bytes = match &verify_key {
        Some(path) => {
            Some(fs::read(path).with_context(|| format!("reading verify key from {:?}", path))?)
        }
        None => None,
    };

    match verify_bundle_with_key(&bundle_path, key_bytes.as_deref()) {
        Ok(VerifyResult::Pass) => {
            emit_jsonl(&terminal_ok(&format!(
                "bundle verified at {:?}",
                bundle_path
            )))?;
            Ok(EXIT_SUCCESS)
        }
        Ok(VerifyResult::Fail(errors)) => {
            // Schema Rule 7: each finding is an independent observation.
            // Emit one per error, then the aggregate terminal event.
            for err in &errors {
                emit_jsonl(&err.to_diagnostic())?;
            }
            let reason = format!("{} finding(s)", errors.len());
            emit_jsonl(&terminal_fail(&reason))?;
            Ok(EXIT_VERIFICATION_FAILURE)
        }
        Ok(VerifyResult::Skipped(reason)) => {
            // Advisory event before the terminal — agents can see why
            // the bundle was skipped even when the outcome is OK.
            emit_jsonl(&Diagnostic {
                code: "VERIFY_SKIPPED".to_string(),
                severity: Severity::Info,
                message: reason.clone(),
                location: None,
                fix_hint: None,
                subcommand: None,
                root_cause_uid: None,
            })?;
            if strict {
                emit_jsonl(&terminal_fail(&format!(
                    "strict mode: verification skipped: {}",
                    reason
                )))?;
                Ok(EXIT_VERIFICATION_FAILURE)
            } else {
                emit_jsonl(&terminal_ok("verification skipped"))?;
                Ok(EXIT_SUCCESS)
            }
        }
        Err(runtime) => {
            // Runtime fault: emit the underlying runtime diag first so
            // the agent has the root cause, then the VERIFY_ERROR
            // terminal so the stream has an unambiguous end marker.
            // Exit 1 per Schema Rule 1.
            let runtime_msg = runtime.to_string();
            emit_jsonl(&runtime.to_diagnostic())?;
            emit_jsonl(&terminal_error(&runtime_msg))?;
            Ok(EXIT_ERROR)
        }
    }
}

fn terminal_ok(message: &str) -> Diagnostic {
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

fn terminal_fail(message: &str) -> Diagnostic {
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

fn terminal_error(message: &str) -> Diagnostic {
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
