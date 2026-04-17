//! `cargo evidence trace`.

use std::path::{Path, PathBuf};

use anyhow::Result;

use evidence::{
    BoundaryConfig, EvidencePolicy, backfill_uuids, load_trace_roots,
    trace::{TraceFiles, read_all_trace_files, validate_trace_links_with_policy},
};

use super::args::{EXIT_ERROR, EXIT_SUCCESS};

pub fn cmd_trace(
    do_validate: bool,
    do_backfill: bool,
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
        let evidence_policy = EvidencePolicy::for_dal(dal);

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
                hlr, llr, tests, ..
            } = read_all_trace_files(root)?;
            match validate_trace_links_with_policy(
                &hlr.requirements,
                &llr.requirements,
                &tests.tests,
                &[], // derived entries (TODO: wire from derived.toml)
                &evidence_policy.trace,
            ) {
                Ok(()) => {
                    if json_output {
                        results.push(serde_json::json!({
                            "root": root,
                            "status": "pass"
                        }));
                    } else {
                        println!("trace: validation passed for '{}'", root);
                    }
                }
                Err(e) => {
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
