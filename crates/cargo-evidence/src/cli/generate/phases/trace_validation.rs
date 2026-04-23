//! Phase 6 — trace-link validation. Extracted from sibling
//! `phases.rs` via `#[path]` to stay under the 500-line workspace
//! limit while keeping the surface co-located with its struct
//! definition.
//!
//! Semantics are decomposed across three LLR claims:
//! - **LLR-061**: `TraceValidationResult` carries both the pass/
//!   fail signal (feeds A3-6 compliance status) and the strict-
//!   profile short-circuit exit code.
//! - Non-strict profile warn-and-continue records `passed =
//!   false` so `write_compliance_reports` claims A3-6 Partial
//!   rather than Met — compliance honesty is the load-bearing
//!   invariant this closes.
//! - Strict profile (Cert/Record) treats the first failure as
//!   short-circuit: emits a JSON failure envelope via the
//!   `fail` helper and returns the exit code in `short_circuit`.

use std::path::Path;

use anyhow::Result;

use evidence_core::{
    EvidencePolicy, Profile,
    trace::{TraceFiles, read_all_trace_files, validate_trace_links_with_policy},
};

use crate::cli::generate::fail;

/// Outcome of Phase 6. `passed` feeds compliance reporting
/// (A3-6 Met vs Partial). `short_circuit` carries the strict-
/// profile exit code. See LLR-061.
pub(in crate::cli::generate) struct TraceValidationResult {
    pub(in crate::cli::generate) passed: bool,
    pub(in crate::cli::generate) short_circuit: Option<i32>,
}

/// Phase 6 — validate trace links. Strict mode: first failure
/// emits JSON failure envelope + sets `short_circuit`. Non-
/// strict: warnings record `passed = false` and continue.
pub(in crate::cli::generate) fn validate_trace_links_phase(
    trace_roots: &[String],
    policy: &EvidencePolicy,
    profile: Profile,
    strict: bool,
    quiet: bool,
    json_output: bool,
) -> Result<TraceValidationResult> {
    let mut passed = true;
    for root in trace_roots {
        let root_path = Path::new(root);
        if !root_path.exists() {
            if !quiet && !json_output {
                eprintln!(
                    "warning: trace root '{}' does not exist, skipping validation",
                    root
                );
            }
            continue;
        }
        match read_all_trace_files(root) {
            Ok(TraceFiles {
                sys,
                hlr,
                llr,
                tests,
                derived,
            }) => {
                let derived_reqs = derived
                    .as_ref()
                    .map(|d| d.requirements.as_slice())
                    .unwrap_or(&[]);
                if let Err(e) = validate_trace_links_with_policy(
                    &sys.requirements,
                    &hlr.requirements,
                    &llr.requirements,
                    &tests.tests,
                    derived_reqs,
                    &policy.trace,
                ) {
                    if strict {
                        let code = fail(
                            json_output,
                            profile,
                            format!("Trace validation failed in '{}': {}", root, e),
                        )?;
                        return Ok(TraceValidationResult {
                            passed: false,
                            short_circuit: Some(code),
                        });
                    }
                    passed = false;
                    eprintln!("warning: trace validation failed in '{}': {}", root, e);
                } else if !quiet && !json_output {
                    println!("evidence: trace links valid in '{}'", root);
                }
            }
            Err(e) => {
                if strict {
                    return Err(anyhow::Error::new(e)
                        .context(format!("reading trace files from '{}'", root)));
                }
                passed = false;
                eprintln!("warning: could not read trace files from '{}': {}", root, e);
            }
        }
    }
    Ok(TraceValidationResult {
        passed,
        short_circuit: None,
    })
}
