//! Helpers that surface silently-skipped verify sub-checks as
//! Info-severity diagnostics so the skip is wire-visible.
//!
//! Library-side checks (`check_llr_test_selectors` and siblings)
//! silently return when an optional bundle artifact is absent —
//! legal on older bundles that predate the atom, or on bundles
//! produced with `--skip-tests`. Without a signal in the JSONL
//! stream, an auditor can't distinguish "check ran and passed"
//! from "check didn't run." These helpers close that gap.

use std::path::Path;

use anyhow::Result;
use evidence_core::{Diagnostic, Location, Severity};

use crate::cli::output::emit_jsonl;

/// Emit `VERIFY_LLR_CHECK_SKIPPED_NO_OUTCOMES` (Info) when
/// `tests/test_outcomes.jsonl` is absent from the bundle. The
/// per-test ↔ LLR back-link check (LLR-052) needs the jsonl as
/// input; without it the reverse traceability assertion is
/// vacuously satisfied and the skip is invisible. Surfacing the
/// notice tells auditors that a bundle predating per-test
/// capture (or produced with `--skip-tests`) didn't exercise
/// the bidirectional loop.
pub fn maybe_emit_llr_check_skipped_no_outcomes(bundle_path: &Path) -> Result<()> {
    let jsonl_path = bundle_path.join("tests").join("test_outcomes.jsonl");
    if jsonl_path.exists() {
        return Ok(());
    }
    emit_jsonl(&Diagnostic {
        code: "VERIFY_LLR_CHECK_SKIPPED_NO_OUTCOMES".to_string(),
        severity: Severity::Info,
        message: "tests/test_outcomes.jsonl absent — LLR-052 reverse-traceability \
             check skipped (older bundle or --skip-tests run)"
            .to_string(),
        location: Some(Location {
            file: Some(jsonl_path),
            ..Location::default()
        }),
        fix_hint: None,
        subcommand: None,
        root_cause_uid: None,
    })?;
    Ok(())
}
