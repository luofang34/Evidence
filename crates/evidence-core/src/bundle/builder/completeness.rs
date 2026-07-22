//! Completeness-facing extras on [`EvidenceBuilder`] (LLR-149):
//! the generate-time fact threading and the finalize-time
//! derivation of [`CompletenessStates`]. Split from `builder.rs`
//! to keep the parent under the 500-line workspace limit, same
//! pattern as `builder_coverage.rs`. As a child module of
//! `builder` it reads the struct's private fields directly —
//! no `pub(super)` accessor shim needed.

use super::EvidenceBuilder;
use crate::bundle::completeness::{CompletenessFacts, CompletenessStates};
use crate::policy::AssuranceLevel;

impl EvidenceBuilder {
    /// Record the generate-time trace-evidence classification.
    /// The CLI's trace-validation phase calls this before
    /// finalize so the `graph_validity` state reflects the same
    /// classification the phase acted on. Never called ⇒ the
    /// state derives `unverifiable` (or `not_applicable` when no
    /// trace roots are configured) — never an assumed pass.
    pub fn set_trace_evidence_state(&mut self, state: crate::trace::TraceEvidenceState) {
        self.trace_evidence_state = Some(state);
    }

    /// Record whether a signing key resolved for this run. The
    /// CLI's finalize-and-sign phase calls this before finalize;
    /// `true` derives `integrity = complete` because the envelope
    /// is signed immediately after the index is written. A run
    /// whose signing step aborts fails generation loudly, so the
    /// bit never attests a signature on a bundle generate
    /// reported as failed.
    pub fn set_signature_planned(&mut self, planned: bool) {
        self.signature_planned = planned;
    }

    /// Assemble the generation-time facts and derive the per-area
    /// [`CompletenessStates`] recorded on the index at finalize
    /// (LLR-149). The compliance-report check reads the bundle
    /// directory — reports are written before finalize so they
    /// land in `SHA256SUMS` — everything else is builder state.
    pub(super) fn derive_completeness_states(&self) -> CompletenessStates {
        let facts = CompletenessFacts {
            tool_command_failures_empty: self.tool_command_failures.is_empty(),
            inputs_empty: self.inputs.is_empty(),
            outputs_empty: self.outputs.is_empty(),
            skip_tests: self.config.skip_tests,
            test_summary_present: self.test_summary.is_some(),
            trace_evidence: self.trace_evidence_state.clone(),
            trace_roots_empty: self.config.trace_roots.is_empty(),
            in_scope_claim: !self.config.dal_map.is_empty(),
            any_dal_claim: self
                .config
                .dal_map
                .values()
                .any(|level| *level != AssuranceLevel::Unclassified),
            compliance_reports_complete: self.compliance_reports_on_disk(),
            signature_planned: self.signature_planned,
        };
        CompletenessStates::derive(&facts)
    }

    /// `true` iff `compliance/<crate>.json` exists in the bundle
    /// for every crate in `dal_map`. Vacuously true for an empty
    /// map — the not-applicable branch keys on the claim, not on
    /// this check.
    fn compliance_reports_on_disk(&self) -> bool {
        self.config.dal_map.keys().all(|crate_name| {
            self.bundle_dir
                .join("compliance")
                .join(format!("{crate_name}.json"))
                .is_file()
        })
    }
}
