//! `cargo evidence trace`.

use std::path::{Path, PathBuf};

use anyhow::Result;

use evidence_core::diagnostic::{Diagnostic, DiagnosticCode, Location, Severity};
use evidence_core::{
    AssuranceSelection, BoundaryConfig, EvidencePolicy, backfill_uuids,
    trace::{
        LinkError, TraceEvidenceEval, TraceEvidenceState, TraceReadError, TraceValidationError,
        evaluate_trace_evidence, read_all_trace_files, resolve_test_selectors,
    },
};

use super::args::{EXIT_ERROR, EXIT_SUCCESS, EXIT_VERIFICATION_FAILURE, OutputFormat};
use super::output::emit_jsonl;

/// `cargo evidence trace` handler: multiplexes the two tracing
/// utilities — `--validate` (cross-check HLR/LLR/Test links) and
/// `--backfill-uuids` (assign stable UUIDs to entries missing them).
/// Either / both may be set; with neither the command is a no-op and
/// exits with [`EXIT_ERROR`].
pub fn cmd_trace(
    do_validate: bool,
    do_backfill: bool,
    require_hlr_sys_trace: bool,
    require_hlr_surface_bijection: bool,
    check_test_selectors: bool,
    trace_roots_arg: Option<String>,
    format: OutputFormat,
) -> Result<i32> {
    let json_output = format == OutputFormat::Json;
    let jsonl_output = format == OutputFormat::Jsonl;

    if !do_backfill && !do_validate {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "error": "specify an action, e.g. --validate or --backfill-uuids"
                }))?
            );
        } else if jsonl_output {
            emit_jsonl(&Diagnostic {
                code: "CLI_INVALID_ARGUMENT".to_string(),
                severity: Severity::Error,
                message: "specify an action, e.g. --validate or --backfill-uuids".to_string(),
                location: None,
                fix_hint: None,
                subcommand: Some("trace".to_string()),
                root_cause_uid: None,
            })?;
            emit_jsonl(&terminal_trace_fail(
                "no action specified (use --validate or --backfill-uuids)",
            ))?;
        } else {
            eprintln!("error: specify an action, e.g. --validate or --backfill-uuids");
        }
        return Ok(EXIT_ERROR);
    }

    // `cargo evidence trace` doesn't accept a workspace PATH argument;
    // it operates on the process CWD. Passing `Path::new(".")`
    // preserves the pre-#72 behaviour: `workspace_root.join("cert/trace")`
    // becomes `./cert/trace`, which is CWD-relative exactly as before.
    // Contrast with `cmd_check_source` which resolves `default_trace_roots`
    // against its explicit `<PATH>` argument — see check.rs.
    let roots: Vec<String> = trace_roots_arg
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
        .unwrap_or_else(|| default_trace_roots(Path::new(".")));

    // Validate trace links
    if do_validate {
        // DAL-driven validation derives from the explicit assurance
        // selection (LLR-109); no usable selection → unclassified
        // (least-strict policy row), never a silent DAL-D.
        let selection = BoundaryConfig::load(&PathBuf::from("cert/boundary.toml"))
            .ok()
            .and_then(|cfg| cfg.assurance_selection())
            .unwrap_or_else(AssuranceSelection::unclassified);
        let mut evidence_policy = EvidencePolicy::for_dal(selection.level.effective_policy_dal());
        // CLI flag overrides the DAL-derived default. Opt-in: external
        // projects without a SYS layer keep passing by default; the
        // tool's own CI enables the flag to make SYS coverage
        // load-bearing for itself.
        if require_hlr_sys_trace {
            evidence_policy.trace.require_hlr_sys_trace = true;
        }
        if require_hlr_surface_bijection {
            evidence_policy.trace.require_hlr_surface_bijection = true;
        }

        let mut all_valid = true;
        let mut results: Vec<serde_json::Value> = Vec::new();

        // Aggregate not-configured state: zero roots means the loop
        // below would vacuously terminate VERIFY_OK over no evidence
        // — the exact silent pass LLR-105 bans. Emit the adoption
        // diagnostic up front.
        if roots.is_empty() {
            let eval = evaluate_trace_evidence(&roots, &evidence_policy.trace);
            emit_evidence_gap(&eval, json_output, jsonl_output, &mut results)?;
            all_valid = false;
        }

        for root in &roots {
            // One semantic evaluation per root — the same
            // classification doctor / check / generate / bundle
            // verify consume. Missing roots, empty trees, and
            // validation failures each get a stable non-success
            // diagnostic; only `Valid` reaches the pass path.
            let eval = evaluate_trace_evidence(std::slice::from_ref(root), &evidence_policy.trace);
            if eval.state.gap_code().is_some() {
                emit_evidence_gap(&eval, json_output, jsonl_output, &mut results)?;
                all_valid = false;
                continue;
            }
            let Some(validation) = eval.validation.as_ref() else {
                // `Valid`: evidence exists and link validation
                // passed. Selector resolution is opt-in and runs
                // only on this arm. The two checks are independent —
                // selector rot does not imply a broken link
                // structure — but a broken link structure usually
                // means selectors are also wrong, so deferring keeps
                // the error messages focused. (`Invalid` with a
                // carried validation error is handled below;
                // `Invalid` from a read/parse failure carries
                // `read_error` and is handled by the same emit.)
                if let Some(read_error) = eval.read_error.as_ref() {
                    emit_read_error(read_error, root, json_output, jsonl_output, &mut results)?;
                    all_valid = false;
                    continue;
                }
                let selector_result = if check_test_selectors {
                    let files = read_all_trace_files(root)?;
                    let unresolved = resolve_test_selectors(&files.tests.tests, Path::new("."));
                    if unresolved.is_empty() {
                        Ok(())
                    } else {
                        let lines: Vec<String> = unresolved
                            .iter()
                            .map(|u| {
                                format!("  {}: selector '{}' did not resolve", u.id, u.selector)
                            })
                            .collect();
                        // Prefix-free message so jsonl callers don't
                        // see `code + ": " + code + ": " + body`. The
                        // human / json paths prepend the code at print
                        // time (see the match arm below); the jsonl
                        // path uses this raw body inside the
                        // `Diagnostic { code, message }` pair.
                        Err(format!(
                            "{} selector(s) did not resolve to a real #[test] fn:\n{}",
                            unresolved.len(),
                            lines.join("\n")
                        ))
                    }
                } else {
                    Ok(())
                };
                match selector_result {
                    Ok(()) => {
                        if json_output {
                            results.push(serde_json::json!({
                                "root": root,
                                "status": "pass"
                            }));
                        } else if jsonl_output {
                            // No per-root event on success; the terminal
                            // covers the aggregate pass signal.
                        } else {
                            println!("[✓] {}: validation passed", root);
                        }
                    }
                    Err(msg) => {
                        if jsonl_output {
                            emit_jsonl(&Diagnostic {
                                code: "TRACE_SELECTOR_UNRESOLVED".to_string(),
                                severity: Severity::Error,
                                message: msg.clone(),
                                location: Some(Location {
                                    file: Some(PathBuf::from(root)),
                                    ..Location::default()
                                }),
                                fix_hint: None,
                                subcommand: Some("trace".to_string()),
                                root_cause_uid: None,
                            })?;
                        } else if json_output {
                            // Keep the legacy human-readable `code: body`
                            // shape for non-jsonl consumers so existing
                            // `jq .message` pipelines that grep for the
                            // TRACE_SELECTOR_UNRESOLVED string still match.
                            let prefixed = format!("TRACE_SELECTOR_UNRESOLVED: {}", msg);
                            results.push(serde_json::json!({
                                "root": root,
                                "status": "fail",
                                "message": prefixed,
                            }));
                        } else {
                            eprintln!("[✗] {} (TRACE_SELECTOR_UNRESOLVED): {}", root, msg);
                        }
                        all_valid = false;
                    }
                }
                continue;
            };
            // `Invalid` with a carried validation error: existing
            // per-variant link-error emission.
            if jsonl_output {
                emit_link_errors_jsonl(validation, root)?;
            } else if json_output {
                results.push(serde_json::json!({
                    "root": root,
                    "status": "fail",
                    "message": validation.to_string()
                }));
            } else {
                emit_link_errors_human(validation, root);
            }
            all_valid = false;
        }
        if !json_output && !jsonl_output {
            if all_valid {
                println!("\nTRACE_OK: {} trace root(s) validated", roots.len());
            } else {
                eprintln!("\nTRACE_FAIL: at least one trace root has unresolved errors");
            }
        }
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "command": "validate",
                    "success": all_valid,
                    "results": results
                }))?
            );
        }
        if jsonl_output {
            // One terminal per run per Rule 1. Aggregate status
            // covers every root; per-variant events already emitted
            // above.
            if all_valid {
                emit_jsonl(&terminal_trace_ok(&format!(
                    "{} trace root(s) validated",
                    roots.len()
                )))?;
            } else {
                emit_jsonl(&terminal_trace_fail(
                    "trace validation failed; see preceding events for per-variant details",
                ))?;
            }
        }
        if !all_valid {
            return Ok(if jsonl_output {
                EXIT_VERIFICATION_FAILURE
            } else {
                EXIT_ERROR
            });
        }
    }

    // Backfill UUIDs
    if do_backfill {
        let mut total = 0;
        for root in &roots {
            let root_path = Path::new(root);
            if !root_path.exists() {
                eprintln!("warning: trace root '{}' does not exist, skipping", root);
                continue;
            }
            let n = backfill_uuids(root)?;
            if n > 0 && !json_output {
                println!("trace: assigned {} UUID(s) in {}", n, root);
            }
            total += n;
        }
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "command": "backfill_uuids",
                    "uuids_assigned": total
                }))?
            );
        } else if total == 0 {
            println!("trace: all entries already have UUIDs");
        } else {
            println!("trace: assigned {} UUID(s) total", total);
        }
    }

    Ok(EXIT_SUCCESS)
}

/// Re-export of [`evidence_core::trace::default_trace_roots`] —
/// the single trace-root discovery path every `cargo evidence` verb
/// shares with `evidence_core::floors::count_trace_per_layer`.
pub use evidence_core::trace::default_trace_roots;

/// Emit the typed adoption-state diagnostic for a no-evidence
/// evaluation ([`TraceEvidenceState::NotConfigured`] /
/// [`TraceEvidenceState::NotAdopted`] / [`TraceEvidenceState::Empty`]).
/// These are the states that used to slide through to `VERIFY_OK`;
/// they now carry the shared `TRACE_EVIDENCE_*` codes and mark the
/// run non-success in every output shape. No-op for `Invalid` /
/// `Valid` states (those have their own emission paths).
fn emit_evidence_gap(
    eval: &TraceEvidenceEval,
    json_output: bool,
    jsonl_output: bool,
    results: &mut Vec<serde_json::Value>,
) -> Result<()> {
    let Some(code) = eval.state.gap_code() else {
        return Ok(());
    };
    let root_label = eval
        .roots
        .first()
        .cloned()
        .unwrap_or_else(|| "<no trace roots>".to_string());
    let message = match &eval.state {
        TraceEvidenceState::NotConfigured => {
            "no trace roots configured or discoverable — a requested trace validation \
             over no evidence cannot succeed"
                .to_string()
        }
        TraceEvidenceState::NotAdopted { missing_roots } => format!(
            "trace root(s) missing on disk: {} — trace evidence is not adopted; \
             absence of evidence is not valid evidence",
            missing_roots.join(", ")
        ),
        TraceEvidenceState::Empty => format!(
            "trace root(s) [{}] hold zero requirements across SYS/HLR/LLR/TEST/DERIVED — \
             an empty trace tree is an adoption state, not valid evidence",
            eval.roots.join(", ")
        ),
        // gap_code() above gates this to the three no-evidence states.
        TraceEvidenceState::Invalid | TraceEvidenceState::Valid => String::new(),
    };
    if jsonl_output {
        emit_jsonl(&Diagnostic {
            code: code.to_string(),
            severity: Severity::Error,
            message,
            location: eval.roots.first().map(|r| Location {
                file: Some(PathBuf::from(r)),
                ..Location::default()
            }),
            fix_hint: None,
            subcommand: Some("trace".to_string()),
            root_cause_uid: None,
        })?;
    } else if json_output {
        results.push(serde_json::json!({
            "root": root_label,
            "status": "fail",
            "message": format!("{}: {}", code, message),
        }));
    } else {
        eprintln!("[✗] {} ({}): {}", root_label, code, message);
    }
    Ok(())
}

/// Emit a read/parse failure from a root that exists but could not
/// be loaded ([`TraceEvidenceState::Invalid`] with `read_error`).
/// Uses the error's own typed code (`TRACE_READ_FAILED` /
/// `TRACE_PARSE_FAILED`) so the wire shape matches the other
/// per-variant emissions.
fn emit_read_error(
    err: &TraceReadError,
    root: &str,
    json_output: bool,
    jsonl_output: bool,
    results: &mut Vec<serde_json::Value>,
) -> Result<()> {
    if jsonl_output {
        emit_jsonl(&Diagnostic {
            code: err.code().to_string(),
            severity: err.severity(),
            message: err.to_string(),
            location: Some(Location {
                file: Some(PathBuf::from(root)),
                ..Location::default()
            }),
            fix_hint: None,
            subcommand: Some("trace".to_string()),
            root_cause_uid: None,
        })?;
    } else if json_output {
        results.push(serde_json::json!({
            "root": root,
            "status": "fail",
            "message": format!("{}: {}", err.code(), err),
        }));
    } else {
        eprintln!("[✗] {} ({}): {}", root, err.code(), err);
    }
    Ok(())
}

/// Stream one JSONL `Diagnostic` per `LinkError` variant inside the
/// `TraceValidationError::Link` envelope. Register-phase errors
/// (kept as `Vec<String>` scope) surface as a single
/// `TRACE_REGISTER_FAILED` aggregate event with the concatenated
/// messages in the payload — typed per-variant Register-phase codes
/// are a separate follow-up.
/// Human-mode renderer for a [`TraceValidationError`]. Mirrors the
/// jsonl expansion: one `[✗]` line per `LinkError` variant so each
/// failure carries its typed `code()` in-line, or a single
/// `TRACE_REGISTER_FAILED` line for register-phase aggregation.
/// stderr so stdout stays the structured channel.
fn emit_link_errors_human(err: &TraceValidationError, root: &str) {
    match err {
        TraceValidationError::Link { errors } => {
            for le in errors {
                eprintln!("[✗] {} ({}): {}", root, le.code(), le);
            }
        }
        TraceValidationError::Register { errors } => {
            eprintln!(
                "[✗] {} (TRACE_REGISTER_FAILED): {}",
                root,
                errors.join("; ")
            );
        }
    }
}

fn emit_link_errors_jsonl(err: &TraceValidationError, root: &str) -> Result<()> {
    match err {
        TraceValidationError::Link { errors } => {
            for le in errors {
                emit_jsonl(&link_error_to_diagnostic(le, root))?;
            }
        }
        TraceValidationError::Register { errors } => {
            emit_jsonl(&Diagnostic {
                code: "TRACE_REGISTER_FAILED".to_string(),
                severity: Severity::Error,
                message: errors.join("; "),
                location: Some(Location {
                    file: Some(PathBuf::from(root)),
                    ..Location::default()
                }),
                fix_hint: None,
                subcommand: Some("trace".to_string()),
                root_cause_uid: None,
            })?;
        }
    }
    Ok(())
}

/// Build a `Diagnostic` from a single `LinkError` variant. The
/// variant's `code()` is the `Diagnostic.code`; `to_string()` is
/// the message; `location.file` is the trace root (finer TOML-path
/// locations land as follow-up since they require threading the
/// file path through `validate_trace_links_with_policy`).
fn link_error_to_diagnostic(le: &LinkError, root: &str) -> Diagnostic {
    Diagnostic {
        code: le.code().to_string(),
        severity: le.severity(),
        message: le.to_string(),
        location: Some(Location {
            file: Some(PathBuf::from(root)),
            ..Location::default()
        }),
        fix_hint: None,
        subcommand: Some("trace".to_string()),
        root_cause_uid: None,
    }
}

fn terminal_trace_ok(message: &str) -> Diagnostic {
    Diagnostic {
        code: "VERIFY_OK".to_string(),
        severity: Severity::Info,
        message: message.to_string(),
        location: None,
        fix_hint: None,
        subcommand: Some("trace".to_string()),
        root_cause_uid: None,
    }
}

fn terminal_trace_fail(message: &str) -> Diagnostic {
    Diagnostic {
        code: "VERIFY_FAIL".to_string(),
        severity: Severity::Error,
        message: message.to_string(),
        location: None,
        fix_hint: None,
        subcommand: Some("trace".to_string()),
        root_cause_uid: None,
    }
}
