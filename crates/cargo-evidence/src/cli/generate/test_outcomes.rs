//! Test-outcome enrichment phase: fill
//! `TestOutcomeRecord.requirement_uids` from the trace's
//! `TestEntry.test_selectors` + `.traces_to`, then serialize
//! to `tests/test_outcomes.jsonl`.
//!
//! Runs after the trace-validation phase (so LLR data is
//! already loaded) but before finalize (so the JSONL file
//! lands in the content layer and gets SHA256SUMS coverage).

use std::path::Path;

use anyhow::{Context, Result};
use evidence_core::EvidenceBuilder;
use evidence_core::trace::{TestEntry, read_all_trace_files};

/// No-op when no test outcomes were captured (dev profile with
/// `skip_tests`, empty workspace, etc.).
pub(super) fn enrich_and_write_test_outcomes(
    builder: &mut EvidenceBuilder,
    trace_roots: &[String],
) -> Result<()> {
    if !builder.has_test_outcomes() {
        return Ok(());
    }
    let mut all_tests: Vec<TestEntry> = Vec::new();
    for root in trace_roots {
        if !Path::new(root).exists() {
            continue;
        }
        if let Ok(tf) = read_all_trace_files(root) {
            all_tests.extend(tf.tests.tests);
        }
    }
    builder.enrich_test_outcomes_with_llrs(&all_tests);
    builder
        .write_test_outcomes()
        .context("writing tests/test_outcomes.jsonl")?;
    Ok(())
}
