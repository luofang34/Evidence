//! `cargo evidence floors [--json]` — the ratcheting-floors gate.
//!
//! Reads `cert/floors.toml`, computes current measurements via
//! [`evidence::floors`], and fails with `FLOORS_BELOW_MIN` on any
//! dimension where `current < committed_floor`. Exit 0 on pass,
//! exit 2 on any violation.
//!
//! Delta-ceiling enforcement (new-dead-code-allows, new library
//! panics in a PR diff) lands with the CI-wiring commit. The
//! `[delta_ceilings]` table is parsed and reported as "skipped"
//! until that step.
//!
//! `--format=jsonl` is explicitly unsupported — the gate emits a
//! single JSON array per run, not a stream. The existing
//! `CLI_UNSUPPORTED_FORMAT` dispatch guard rejects it.

use std::path::PathBuf;

use anyhow::Result;
use serde::Serialize;

use evidence::floors::LoadOutcome;
use evidence::{FloorsConfig, current_measurements};

use super::args::{EXIT_ERROR, EXIT_SUCCESS, EXIT_VERIFICATION_FAILURE};
use super::output::emit_json;

/// One row of the floors report.
#[derive(Debug, Serialize)]
struct FloorRow {
    name: String,
    kind: &'static str,
    current: u64,
    floor: u64,
    status: &'static str,
}

/// Entrypoint for `cargo evidence floors`.
pub fn cmd_floors(json: bool, config: Option<PathBuf>) -> Result<i32> {
    let workspace = std::env::current_dir()?;
    let floors_path = config.unwrap_or_else(|| workspace.join("cert").join("floors.toml"));
    let config = match FloorsConfig::load_or_missing(&floors_path) {
        LoadOutcome::Loaded(c) => c,
        LoadOutcome::Missing => {
            // Downstream users who haven't adopted the floors gate
            // hit this path. Emit a friendly info message on stderr
            // (so stdout stays clean for piping), exit 0. Set up a
            // `cert/floors.toml` or pass `--config <path>` to
            // enable the gate.
            eprintln!(
                "info: no floors config at {} — floors gate is not configured for this project. \
                 Create cert/floors.toml (or pass --config) to enable. See README \
                 \"`cargo evidence floors` — the ratchet\" for the expected shape.",
                floors_path.display()
            );
            return Ok(EXIT_SUCCESS);
        }
        LoadOutcome::Error(e) => {
            eprintln!("error: {}", e);
            return Ok(EXIT_ERROR);
        }
    };
    let measurements = current_measurements(&workspace);

    let rows = build_rows(&config, &measurements);
    let any_fail = rows.iter().any(|r| r.status == "fail");

    if json {
        emit_json(&rows)?;
    } else {
        print_human(&rows);
    }

    Ok(if any_fail {
        EXIT_VERIFICATION_FAILURE
    } else {
        EXIT_SUCCESS
    })
}

fn build_rows(
    config: &FloorsConfig,
    measurements: &std::collections::BTreeMap<String, u64>,
) -> Vec<FloorRow> {
    let mut rows: Vec<FloorRow> = Vec::new();
    for (name, &floor) in &config.floors {
        let current = measurements.get(name).copied().unwrap_or(0);
        let status = if current >= floor { "pass" } else { "fail" };
        rows.push(FloorRow {
            name: name.clone(),
            kind: "floor",
            current,
            floor,
            status,
        });
    }
    // Delta ceilings: parsed, reported as "deferred" pending CI
    // commit. Keeps the shape stable for agents reading the JSON
    // output today.
    for (name, &floor) in &config.delta_ceilings {
        rows.push(FloorRow {
            name: name.clone(),
            kind: "delta_ceiling",
            current: 0,
            floor,
            status: "deferred",
        });
    }
    rows
}

fn print_human(rows: &[FloorRow]) {
    let name_w = rows.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
    println!(
        "{:<name_w$}  {:<14}  {:>8}  {:>8}  STATUS",
        "DIMENSION",
        "KIND",
        "CURRENT",
        "FLOOR",
        name_w = name_w
    );
    println!(
        "{:-<name_w$}  {:-<14}  {:->8}  {:->8}  {:-<8}",
        "",
        "",
        "",
        "",
        "",
        name_w = name_w
    );
    for r in rows {
        let marker = match r.status {
            "pass" => "✓ pass",
            "fail" => "✗ FAIL",
            "deferred" => "⏸ deferred (not enforced yet)",
            other => other,
        };
        println!(
            "{:<name_w$}  {:<14}  {:>8}  {:>8}  {}",
            r.name,
            r.kind,
            r.current,
            r.floor,
            marker,
            name_w = name_w,
        );
    }
    println!();
    let fails = rows.iter().filter(|r| r.status == "fail").count();
    if fails == 0 {
        let deferred = rows.iter().filter(|r| r.status == "deferred").count();
        let pass = rows.iter().filter(|r| r.status == "pass").count();
        if deferred > 0 {
            println!(
                "{} floor(s) pass. {} delta_ceiling(s) declared but NOT enforced yet — \
                 parsed only for wire-shape stability; the diff-enforcement path lands \
                 in a follow-up commit.",
                pass, deferred
            );
        } else {
            println!("{} floor(s) pass.", pass);
        }
    } else {
        println!(
            "FLOORS_BELOW_MIN: {} floor violation(s). Rigor has slipped — \
             either restore the measurement or edit cert/floors.toml with \
             a `Lower-Floor:` justification line in the PR body.",
            fails
        );
    }
}
