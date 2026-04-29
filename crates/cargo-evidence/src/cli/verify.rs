//! `cargo evidence verify`.

use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use serde::Serialize;

use evidence_core::diagnostic::{Diagnostic, DiagnosticCode, Severity};
use evidence_core::verify::VerifyRuntimeError;
use evidence_core::{VerifyResult, verify_bundle_with_key};

use super::args::{EXIT_ERROR, EXIT_SUCCESS, EXIT_VERIFICATION_FAILURE, OutputFormat};
use super::output::{emit_json, emit_jsonl};

mod incomplete_bundle;
mod skipped_notices;
mod terminals;
use incomplete_bundle::maybe_emit_bundle_incomplete_warning;
use skipped_notices::maybe_emit_llr_check_skipped_no_outcomes;
use terminals::{terminal_error, terminal_fail, terminal_ok};

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

    // Bundle-path existence is an I/O / runtime fault, not a
    // verification finding. Harmonize with the JSONL path at
    // `cmd_verify_jsonl` below: both return `EXIT_ERROR` (1) here.
    // `EXIT_VERIFICATION_FAILURE` (2) stays reserved for "verify
    // ran successfully against a real bundle and found problems
    // inside it" (hash mismatch, missing declared files,
    // cross-file inconsistency, etc.). Same condition, same exit
    // code across `--format={human,json,jsonl}`.
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
            EXIT_ERROR,
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

    // Load verify key if provided. An I/O failure here is a runtime
    // fault (key file missing / unreadable) — mirror the
    // bundle-not-found shape above so all three formats stay
    // HLR-016-consistent: human prints `error: ...`, json wraps the
    // failure in `VerifyOutput { success: false, ... }`, both exit 1.
    let key_bytes = match &verify_key {
        Some(path) => match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(source) => {
                let err = VerifyRuntimeError::ReadVerifyKey {
                    path: path.clone(),
                    source,
                };
                let msg = err.to_string();
                return fail_verify(
                    json_output,
                    &bundle_path,
                    vec![VerifyCheck {
                        name: "verify_key".to_string(),
                        status: "fail".to_string(),
                        message: Some(msg.clone()),
                    }],
                    "error:",
                    msg,
                    EXIT_ERROR,
                );
            }
        },
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
            // Pre-release tool on dev profile: downgrade to warning +
            // pass. Mirrors the JSONL path's severity split.
            let all_prerelease_on_dev = !errors.is_empty()
                && errors.iter().all(|e| {
                    matches!(
                        e,
                        evidence_core::VerifyError::PrereleaseToolDetected { profile, .. }
                            if profile == "dev"
                    )
                });
            if all_prerelease_on_dev {
                for err in &errors {
                    if let evidence_core::VerifyError::PrereleaseToolDetected {
                        engine_crate_version,
                        ..
                    } = err
                    {
                        eprintln!(
                            "verify: warning - bundle produced by pre-release tool ({}) on dev profile; non-blocking",
                            engine_crate_version
                        );
                    }
                }
                checks.push(VerifyCheck {
                    name: "bundle_integrity".to_string(),
                    status: "pass".to_string(),
                    message: Some(
                        "pre-release tool warning on dev profile — non-blocking".to_string(),
                    ),
                });
                if json_output {
                    emit_json(&VerifyOutput {
                        success: true,
                        bundle_path: bundle_path.display().to_string(),
                        checks,
                        error: None,
                    })?;
                } else {
                    println!("verify: PASS (with warning) - bundle {:?}", bundle_path);
                }
                return Ok(EXIT_SUCCESS);
            }

            let reason = errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            // Map each VerifyError to its own VerifyCheck for granular JSON output
            for err in &errors {
                use evidence_core::VerifyError as VE;
                #[rustfmt::skip]
                let name = match err {
                    VE::UnexpectedFile(_)               => "unexpected_file",
                    VE::HmacFailure                     => "hmac_signature",
                    VE::HashMismatch { .. }             => "hash_mismatch",
                    VE::MissingHashedFile(_)            => "missing_file",
                    VE::ContentHashMismatch { .. }      => "content_hash",
                    VE::UnsafePath(_)                   => "unsafe_path",
                    VE::FormatError { .. }              => "format_error",
                    VE::CrossFileInconsistency { .. }   => "cross_file_mismatch",
                    VE::DeterministicHashMismatch { .. } => "deterministic_hash",
                    VE::ManifestProjectionDrift { .. }  => "manifest_projection",
                    VE::TraceOutputNotHashed(_)         => "trace_output_not_hashed",
                    VE::TestSummaryMismatch { .. }      => "test_summary_mismatch",
                    VE::DalMapMismatch { .. }           => "dal_map_mismatch",
                    VE::DalMapOrphan { .. }             => "dal_map_orphan",
                    VE::PrereleaseToolDetected { .. }   => "prerelease_tool_detected",
                    VE::BundleIncompletelyClaimed { .. } => "bundle_incompletely_claimed",
                    VE::ToolCommandsFailedSilently { .. } => "tool_commands_failed_silently",
                    VE::TestSummaryAbsentOnFailedRun { .. } => "test_summary_absent_on_failed_run",
                    VE::LlrTestSelectorUnresolved { .. } => "llr_test_selector_unresolved",
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

    // Load verify key. An I/O failure (missing / unreadable key
    // file) is a runtime fault but must still emit the JSONL
    // terminal pair — Schema Rule 1 mandates exactly one terminal
    // per --format=jsonl run, and the user-visible verify pipeline
    // already started. Mirror the BundleNotFound shape above:
    // emit the structured `VERIFY_RUNTIME_READ_VERIFY_KEY` finding,
    // then the `VERIFY_ERROR` terminal, then exit 1.
    let key_bytes = match &verify_key {
        Some(path) => match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(source) => {
                let err = VerifyRuntimeError::ReadVerifyKey {
                    path: path.clone(),
                    source,
                };
                let msg = err.to_string();
                emit_jsonl(&err.to_diagnostic())?;
                emit_jsonl(&terminal_error(&msg))?;
                return Ok(EXIT_ERROR);
            }
        },
        None => None,
    };

    match verify_bundle_with_key(&bundle_path, key_bytes.as_deref()) {
        Ok(VerifyResult::Pass) => {
            // A dev-profile bundle can legitimately land in
            // Pass with `bundle_complete: false` + recorded
            // tool-command failures — the library only pushes
            // errors for cert/record. Surface the incomplete-
            // bundle signal as a Warning diagnostic so agents +
            // humans see it alongside VERIFY_OK, without
            // blocking verification (dev snapshots of broken
            // builds are a legitimate debugging artifact).
            maybe_emit_bundle_incomplete_warning(&bundle_path)?;
            maybe_emit_llr_check_skipped_no_outcomes(&bundle_path)?;
            emit_jsonl(&terminal_ok(&format!(
                "bundle verified at {:?}",
                bundle_path
            )))?;
            Ok(EXIT_SUCCESS)
        }
        Ok(VerifyResult::Fail(errors)) => {
            // Schema Rule 7: one diagnostic per finding, then the
            // aggregate terminal. `VERIFY_PRERELEASE_TOOL` on dev
            // profile downgrades to Warning (non-blocking for
            // pre-release-tool trial runs); every other variant
            // stays Error regardless of profile.
            let mut any_error = false;
            for err in &errors {
                let mut diag = err.to_diagnostic();
                if let evidence_core::VerifyError::PrereleaseToolDetected { profile, .. } = err
                    && profile == "dev"
                {
                    diag.severity = Severity::Warning;
                } else {
                    any_error = true;
                }
                emit_jsonl(&diag)?;
            }
            if any_error {
                let reason = format!("{} finding(s)", errors.len());
                emit_jsonl(&terminal_fail(&reason))?;
                Ok(EXIT_VERIFICATION_FAILURE)
            } else {
                emit_jsonl(&terminal_ok(
                    "pre-release tool warning on dev profile — non-blocking",
                ))?;
                Ok(EXIT_SUCCESS)
            }
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
