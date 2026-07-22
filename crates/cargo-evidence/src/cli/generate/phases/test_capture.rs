//! Phase 5 — run nextest and capture. Lives in this sibling file
//! (same pattern as `output_inventory.rs`) so the parent `phases.rs`
//! stays under the workspace 500-line limit.

use anyhow::Result;

use evidence_core::{EvidenceBuilder, parse_nextest_libtest_json};

/// Run `cargo nextest run --workspace` under `libtest-json-plus`
/// through the builder's `run_capture`, parse the machine-readable
/// event stream, and record the per-test outcomes + summary on the
/// builder. Machine-readable output preserves per-binary identity, so
/// LLR `test_selector`s resolve to executed results — the identity that
/// plain libtest text loses as `__unknown_binary__`.
///
/// `skip_tests` short-circuits. In strict mode any failure to *run*
/// nextest bails (so cert bundles never silently omit test evidence);
/// in dev mode a spawn failure degrades to a warning. The nextest argv
/// carries the run's dependency-resolution policy flags (LLR-140).
pub(in crate::cli::generate) fn run_tests_and_capture(
    builder: &mut EvidenceBuilder,
    skip_tests: bool,
    strict: bool,
    quiet: bool,
    json_output: bool,
) -> Result<()> {
    if skip_tests {
        return Ok(());
    }
    let mut test_cmd = std::process::Command::new("cargo");
    test_cmd.args([
        "nextest",
        "run",
        "--workspace",
        // Record every test's outcome even when some fail — evidence
        // generation must not stop at the first failure (nextest's
        // default), or the bundle would omit results for the rest.
        "--no-fail-fast",
        "--message-format",
        "libtest-json-plus",
    ]);
    // The shared dependency-resolution policy (LLR-140): under
    // `locked_offline` nextest resolves the pinned graph from the
    // local cache; under the dev online opt-in no flags are added.
    test_cmd.args(builder.resolution_policy().cargo_args());
    // The libtest-json format is gated behind this env in current
    // nextest; NO_COLOR keeps the JSON stream free of ANSI escapes.
    test_cmd.env("NEXTEST_EXPERIMENTAL_LIBTEST_JSON", "1");
    test_cmd.env("NO_COLOR", "1");
    match builder.run_capture(
        test_cmd,
        "tests",
        "cargo_test",
        "cargo nextest run --workspace",
    ) {
        Ok((stdout, _stderr)) => {
            let stdout_str = String::from_utf8_lossy(&stdout);
            let run = parse_nextest_libtest_json(&stdout_str);
            // The suite-level summary and the per-test records are two
            // independent tallies of the same stream; if they disagree
            // the capture dropped an event. Fail closed in strict mode
            // (a cert bundle must not ship inconsistent test evidence);
            // warn on dev.
            let discrepancies = run.reconcile();
            if !discrepancies.is_empty() {
                let detail = discrepancies
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                if strict {
                    return Err(anyhow::anyhow!(
                        "nextest summary does not reconcile with per-test records \
                         ({detail}); captured test evidence is inconsistent"
                    ));
                }
                tracing::warn!("nextest summary/record reconciliation mismatch: {detail}");
            }
            if !quiet && !json_output {
                println!(
                    "evidence: tests: {} passed, {} failed, {} ignored",
                    run.summary.passed, run.summary.failed, run.summary.ignored
                );
            }
            builder.set_test_summary(run.summary);
            if !run.records.is_empty() {
                // Write is deferred to `enrich_and_write_test_outcomes`,
                // which runs after the trace phase loads LLR data and
                // populates the per-test → LLR back-links.
                builder.set_test_outcomes(run.records);
            }
        }
        Err(e) => {
            // run_capture returns Err only on subprocess spawn
            // failure; non-zero exit goes through the Ok arm and is
            // recorded inside run_capture. Record spawn failures here
            // so verify sees the bundle as incomplete either way.
            builder.record_command_failure(evidence_core::ToolCommandFailure {
                command_name: "cargo nextest run --workspace".to_string(),
                exit_code: -1,
                stderr_tail: e.to_string(),
            });
            if strict {
                return Err(anyhow::Error::new(e).context("running cargo nextest"));
            }
            tracing::warn!("cargo nextest could not be spawned: {}", e);
        }
    }
    Ok(())
}
