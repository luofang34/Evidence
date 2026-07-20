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
//!
//! Every root is classified by the shared
//! [`evaluate_trace_evidence`] (LLR-105): a missing root or
//! a zero-requirement tree is an adoption state and counts as
//! non-success exactly like a validation failure — strict
//! short-circuits, non-strict warns with `passed = false`.

use anyhow::Result;

use evidence_core::{
    EvidencePolicy, Profile,
    trace::{TraceEvidenceState, evaluate_trace_evidence},
};

use crate::cli::generate::fail;

/// Outcome of Phase 6. `passed` feeds compliance reporting
/// (A3-6 Met vs Partial). `short_circuit` carries the strict-
/// profile exit code. See LLR-061.
pub(in crate::cli::generate) struct TraceValidationResult {
    pub(in crate::cli::generate) passed: bool,
    pub(in crate::cli::generate) short_circuit: Option<i32>,
}

/// Phase 6 — validate trace links. Every root goes through the
/// shared [`evaluate_trace_evidence`] classification (LLR-105), so a
/// missing root or a zero-requirement tree counts as non-success
/// exactly like a validation failure: strict mode (cert/record)
/// emits the JSON failure envelope + sets `short_circuit`;
/// non-strict records `passed = false` and warns. Only `Valid`
/// roots keep the historical pass print.
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
        let eval = evaluate_trace_evidence(std::slice::from_ref(root), &policy.trace);
        match &eval.state {
            TraceEvidenceState::Valid => {
                if !quiet && !json_output {
                    println!("evidence: trace links valid in '{}'", root);
                }
            }
            TraceEvidenceState::Invalid => {
                if let Some(validation) = &eval.validation {
                    if strict {
                        let code = fail(
                            json_output,
                            profile,
                            format!("Trace validation failed in '{}': {}", root, validation),
                        )?;
                        return Ok(TraceValidationResult {
                            passed: false,
                            short_circuit: Some(code),
                        });
                    }
                    passed = false;
                    eprintln!(
                        "warning: trace validation failed in '{}': {}",
                        root, validation
                    );
                } else if let Some(read_error) = &eval.read_error {
                    if strict {
                        return Err(anyhow::anyhow!("{}", read_error)
                            .context(format!("reading trace files from '{}'", root)));
                    }
                    passed = false;
                    eprintln!(
                        "warning: could not read trace files from '{}': {}",
                        root, read_error
                    );
                }
            }
            no_evidence => {
                // Missing roots / zero-requirement trees are
                // adoption states, not evidence — the requested
                // trace claim behind this bundle cannot succeed
                // over them (LLR-107).
                let detail = match no_evidence {
                    TraceEvidenceState::NotAdopted { missing_roots } => format!(
                        "not adopted (root(s) missing on disk: {})",
                        missing_roots.join(", ")
                    ),
                    TraceEvidenceState::Empty => {
                        "empty (zero requirements across all layers)".to_string()
                    }
                    TraceEvidenceState::NotConfigured => "not configured".to_string(),
                    TraceEvidenceState::Invalid | TraceEvidenceState::Valid => String::new(),
                };
                let message = format!(
                    "trace evidence {} in '{}'; the requested trace claim has no evidence",
                    detail, root
                );
                if strict {
                    let code = fail(json_output, profile, message)?;
                    return Ok(TraceValidationResult {
                        passed: false,
                        short_circuit: Some(code),
                    });
                }
                passed = false;
                eprintln!("warning: {}", message);
            }
        }
    }
    Ok(TraceValidationResult {
        passed,
        short_circuit: None,
    })
}
