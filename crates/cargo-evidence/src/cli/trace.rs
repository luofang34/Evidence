//! `cargo evidence trace`.

use std::path::{Path, PathBuf};

use anyhow::Result;

use evidence::{
    BoundaryConfig, EvidencePolicy, backfill_uuids, load_trace_roots,
    trace::{
        TraceFiles, read_all_trace_files, resolve_test_selectors, validate_trace_links_with_policy,
    },
};

use super::args::{EXIT_ERROR, EXIT_SUCCESS};

/// `cargo evidence trace` handler: multiplexes the two tracing
/// utilities — `--validate` (cross-check HLR/LLR/Test links) and
/// `--backfill-uuids` (assign stable UUIDs to entries missing them).
/// Either / both may be set; with neither the command is a no-op and
/// exits with [`EXIT_ERROR`].
pub fn cmd_trace(
    do_validate: bool,
    do_backfill: bool,
    require_hlr_sys_trace: bool,
    check_test_selectors: bool,
    trace_roots_arg: Option<String>,
    json_output: bool,
) -> Result<i32> {
    if !do_backfill && !do_validate {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "error": "specify an action, e.g. --validate or --backfill-uuids"
                }))?
            );
        } else {
            eprintln!("error: specify an action, e.g. --validate or --backfill-uuids");
        }
        return Ok(EXIT_ERROR);
    }

    let roots: Vec<String> = trace_roots_arg
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
        .unwrap_or_else(|| load_trace_roots(Path::new("cert/boundary.toml")));

    // Validate trace links
    if do_validate {
        // Load DAL config from boundary.toml for DAL-driven validation.
        // Missing/malformed file → default config (DAL-D everywhere).
        let boundary_config = BoundaryConfig::load_or_default(&PathBuf::from("cert/boundary.toml"));
        let dal_map = boundary_config.dal_map();
        let dal = dal_map.values().copied().max().unwrap_or_default();
        let mut evidence_policy = EvidencePolicy::for_dal(dal);
        // CLI flag overrides the DAL-derived default. Opt-in: external
        // projects without a SYS layer keep passing by default; the
        // tool's own CI enables the flag to make SYS coverage
        // load-bearing for itself.
        if require_hlr_sys_trace {
            evidence_policy.trace.require_hlr_sys_trace = true;
        }

        let mut all_valid = true;
        let mut results: Vec<serde_json::Value> = Vec::new();
        for root in &roots {
            let root_path = Path::new(root);
            if !root_path.exists() {
                if json_output {
                    results.push(serde_json::json!({
                        "root": root,
                        "status": "skipped",
                        "message": "trace root does not exist"
                    }));
                } else {
                    eprintln!("warning: trace root '{}' does not exist, skipping", root);
                }
                continue;
            }
            let TraceFiles {
                sys,
                hlr,
                llr,
                tests,
                ..
            } = read_all_trace_files(root)?;
            let link_result = validate_trace_links_with_policy(
                &sys.requirements,
                &hlr.requirements,
                &llr.requirements,
                &tests.tests,
                &[], // derived entries (TODO: wire from derived.toml)
                &evidence_policy.trace,
            );

            // Selector resolution is opt-in and runs only when the
            // UID-level validation passes. The two checks are
            // independent — selector rot does not imply a broken
            // link structure — but a broken link structure usually
            // means selectors are also wrong, so deferring keeps the
            // error messages focused.
            let selector_result = if check_test_selectors && link_result.is_ok() {
                let unresolved = resolve_test_selectors(&tests.tests, std::path::Path::new("."));
                if unresolved.is_empty() {
                    Ok(())
                } else {
                    let lines: Vec<String> = unresolved
                        .iter()
                        .map(|u| format!("  {}: selector '{}' did not resolve", u.id, u.selector))
                        .collect();
                    Err(format!(
                        "TRACE_SELECTOR_UNRESOLVED: {} selector(s) did not \
                         resolve to a real #[test] fn:\n{}",
                        unresolved.len(),
                        lines.join("\n")
                    ))
                }
            } else {
                Ok(())
            };

            match (link_result, selector_result) {
                (Ok(()), Ok(())) => {
                    if json_output {
                        results.push(serde_json::json!({
                            "root": root,
                            "status": "pass"
                        }));
                    } else {
                        println!("trace: validation passed for '{}'", root);
                    }
                }
                (Err(e), _) => {
                    if json_output {
                        results.push(serde_json::json!({
                            "root": root,
                            "status": "fail",
                            "message": e.to_string()
                        }));
                    } else {
                        eprintln!("trace: validation FAILED for '{}': {}", root, e);
                    }
                    all_valid = false;
                }
                (Ok(()), Err(msg)) => {
                    if json_output {
                        results.push(serde_json::json!({
                            "root": root,
                            "status": "fail",
                            "message": msg,
                        }));
                    } else {
                        eprintln!("trace: validation FAILED for '{}': {}", root, msg);
                    }
                    all_valid = false;
                }
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
        if !all_valid {
            return Ok(EXIT_ERROR);
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
