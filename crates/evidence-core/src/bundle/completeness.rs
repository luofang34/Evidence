//! `CompletenessStates` — the per-area completeness verdicts recorded
//! on `index.json` (LLR-149 / HLR-114 / SYS-049).
//!
//! The legacy `bundle_complete` Boolean conflates every assurance
//! area into one bit derived from captured-subprocess exit status
//! alone. `CompletenessStates` supersedes it with one independently
//! derived state per area, so a consumer can tell "the capture is
//! complete but verification was never evaluated" apart from
//! "everything passed". Every state derives from facts known at
//! generation time; a state the generator cannot substantiate is
//! recorded as [`CompletenessState::Incomplete`] or
//! [`CompletenessState::Unverifiable`], never optimistically
//! [`CompletenessState::Complete`].
//!
//! # Derivation table
//!
//! | Area | Complete | Incomplete | NotApplicable | Unverifiable |
//! |------|----------|------------|---------------|--------------|
//! | capture | no failures, inputs non-empty, tests ran with a summary | any recorded command failure; empty inputs; a test-running run produced no summary | run skipped tests (after the failure/inputs gates) | — |
//! | graph_validity | trace evidence classified `Valid` at generation | classification `NotAdopted` / `Empty` / `Invalid` | no trace roots configured, or classification `NotConfigured` | roots configured but never classified |
//! | verification | immediate post-generation verification passed (patched in before sealing) | immediate verification failed (patched in) | — | not evaluated at generation (the finalize-time value) |
//! | objective_mapping | compliance reports on disk for every in-scope crate | a report missing for an in-scope crate | no in-scope claim (empty `dal_map`) | — |
//! | review_approval | — | — | always: review records are workspace-corpus state, not bundle artifacts | — |
//! | integrity | a signing key resolved for the run | — | no signing key resolved (unsigned bundle) | — |
//! | reproducibility | outputs manifest non-empty | a test-running run captured zero outputs | run skipped tests | — |
//! | tool_qualification | a claimed assurance level exists and every in-scope compliance report is on disk | a report missing for an in-scope crate | no claimed level (empty or all-unclassified `dal_map`) | — |
//!
//! The DAL-A auxiliary-MC/DC-tool gate runs upstream of finalize, so
//! a DAL-A claim that reaches derivation has already proven its
//! auxiliary evidence; `tool_qualification` keys on the recorded
//! claim plus the written reports.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::trace::TraceEvidenceState;

/// One assurance area's completeness verdict.
///
/// Serialized as snake_case (`"complete"`, `"incomplete"`,
/// `"not_applicable"`, `"unverifiable"`) so the on-disk index stays
/// ergonomic for reviewers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletenessState {
    /// The area's evidence is present and substantiated.
    Complete,
    /// The area was in scope and its evidence is absent, empty, or
    /// known-broken.
    Incomplete,
    /// The area was out of scope for this run — nothing was claimed,
    /// so nothing had to be captured.
    NotApplicable,
    /// The state was never evaluated at generation time. The honest
    /// default: a bundle that does not know must not claim either
    /// success or failure.
    Unverifiable,
}

/// Per-area completeness verdicts recorded on
/// [`EvidenceIndex`](super::EvidenceIndex). See the module-level
/// derivation table for the exact rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletenessStates {
    /// Capture completeness: source inputs, commands, and test
    /// results the run claimed to capture are actually recorded.
    pub capture: CompletenessState,
    /// Graph validity: the trace evidence behind the bundle's claim
    /// validated at generation time.
    pub graph_validity: CompletenessState,
    /// Verification completeness: the bundle passed the tool's own
    /// verification immediately after generation. Patched into the
    /// index by the CLI before the signature seals the envelope; the
    /// finalize-time value is [`CompletenessState::Unverifiable`].
    pub verification: CompletenessState,
    /// Objective-mapping completeness: every in-scope crate has its
    /// per-objective compliance report in the bundle.
    pub objective_mapping: CompletenessState,
    /// Review/approval completeness. Always
    /// [`CompletenessState::NotApplicable`]: review records are
    /// workspace-corpus state, not bundle artifacts, so no bundle
    /// can substantiate the area yet.
    pub review_approval: CompletenessState,
    /// Integrity verification: the bundle's `(SHA256SUMS,
    /// index.json)` envelope is signed.
    pub integrity: CompletenessState,
    /// Reproducibility verification: the bundle attests the outputs
    /// a rebuild would have to reproduce.
    pub reproducibility: CompletenessState,
    /// Tool qualification: the bundle records a claimed assurance
    /// level and the compliance reports that bind the standards
    /// pack the claim is evaluated against.
    pub tool_qualification: CompletenessState,
}

impl CompletenessStates {
    /// The deserialization default for bundles written before the
    /// field existed: every state is [`CompletenessState::Unverifiable`]
    /// because nothing was recorded. Honest by construction — a
    /// legacy bundle never retroactively claims completeness.
    pub fn legacy() -> Self {
        Self {
            capture: CompletenessState::Unverifiable,
            graph_validity: CompletenessState::Unverifiable,
            verification: CompletenessState::Unverifiable,
            objective_mapping: CompletenessState::Unverifiable,
            review_approval: CompletenessState::Unverifiable,
            integrity: CompletenessState::Unverifiable,
            reproducibility: CompletenessState::Unverifiable,
            tool_qualification: CompletenessState::Unverifiable,
        }
    }

    /// Derive every area from generation-time facts. Pure: the
    /// builder assembles [`CompletenessFacts`] from its own state
    /// (and a disk check for the compliance reports) so the rules
    /// stay unit-testable without a bundle directory.
    ///
    /// `verification` always derives as
    /// [`CompletenessState::Unverifiable`] — the immediate
    /// verification runs after finalize, and the CLI patches the
    /// real outcome in via [`record_verification_state`] before
    /// sealing. A bundle that never gets patched therefore never
    /// claims verification completeness.
    pub fn derive(facts: &CompletenessFacts) -> Self {
        Self {
            capture: derive_capture(facts),
            graph_validity: derive_graph_validity(facts),
            verification: CompletenessState::Unverifiable,
            objective_mapping: derive_objective_mapping(facts),
            review_approval: CompletenessState::NotApplicable,
            integrity: if facts.signature_planned {
                CompletenessState::Complete
            } else {
                CompletenessState::NotApplicable
            },
            reproducibility: derive_reproducibility(facts),
            tool_qualification: derive_tool_qualification(facts),
        }
    }
}

/// The generation-time facts [`CompletenessStates::derive`] reads.
/// Assembled by the builder at finalize; every field is a fact the
/// pipeline already knows — no re-running of verify, no inference
/// from artifacts outside the builder's own records.
#[derive(Debug, Clone)]
pub struct CompletenessFacts {
    /// `true` when no captured subprocess failed during the run.
    pub tool_command_failures_empty: bool,
    /// `true` when `inputs_hashes.json` records zero source inputs.
    pub inputs_empty: bool,
    /// `true` when `outputs_hashes.json` records zero outputs.
    pub outputs_empty: bool,
    /// `true` when the run skipped test capture (`--skip-tests`).
    pub skip_tests: bool,
    /// `true` when a parsed test summary is recorded on the index.
    pub test_summary_present: bool,
    /// The generate-time trace-evidence classification when the
    /// pipeline ran one; `None` when it never evaluated the roots.
    pub trace_evidence: Option<TraceEvidenceState>,
    /// `true` when no trace roots were configured for the run.
    pub trace_roots_empty: bool,
    /// `true` when the bundle declares in-scope crates (non-empty
    /// `dal_map`).
    pub in_scope_claim: bool,
    /// `true` when at least one in-scope crate claims a real
    /// assurance level (anything other than `unclassified`).
    pub any_dal_claim: bool,
    /// `true` when `compliance/<crate>.json` exists on disk for
    /// every crate in `dal_map`.
    pub compliance_reports_complete: bool,
    /// `true` when a signing key resolved for the run, so finalize
    /// is followed by envelope signing.
    pub signature_planned: bool,
}

/// `capture`: a run that recorded failures or hashed no inputs is
/// never capture-complete; a `--skip-tests` run made no test claim
/// (checked only after the failure and input gates so an empty
/// baseline still fails closed); a test-running run without a
/// parsed summary lost its results.
fn derive_capture(facts: &CompletenessFacts) -> CompletenessState {
    if !facts.tool_command_failures_empty || facts.inputs_empty {
        return CompletenessState::Incomplete;
    }
    if facts.skip_tests {
        return CompletenessState::NotApplicable;
    }
    if !facts.test_summary_present {
        return CompletenessState::Incomplete;
    }
    CompletenessState::Complete
}

/// `graph_validity`: the trace-evidence classification the
/// pipeline recorded at generation, mapped state-for-state. Only
/// `Valid` substantiates the graph; an unconfigured workspace has
/// no graph to claim; configured-but-unevaluated roots are
/// honestly unverifiable.
fn derive_graph_validity(facts: &CompletenessFacts) -> CompletenessState {
    match &facts.trace_evidence {
        Some(TraceEvidenceState::Valid) => CompletenessState::Complete,
        Some(TraceEvidenceState::NotConfigured) => CompletenessState::NotApplicable,
        Some(
            TraceEvidenceState::NotAdopted { .. }
            | TraceEvidenceState::Empty
            | TraceEvidenceState::Invalid,
        ) => CompletenessState::Incomplete,
        None => {
            if facts.trace_roots_empty {
                CompletenessState::NotApplicable
            } else {
                CompletenessState::Unverifiable
            }
        }
    }
}

/// `objective_mapping`: the per-crate objective reports must exist
/// for every crate the bundle declares in scope.
fn derive_objective_mapping(facts: &CompletenessFacts) -> CompletenessState {
    if !facts.in_scope_claim {
        return CompletenessState::NotApplicable;
    }
    if facts.compliance_reports_complete {
        CompletenessState::Complete
    } else {
        CompletenessState::Incomplete
    }
}

/// `reproducibility`: recorded outputs are the manifest a rebuild
/// would have to reproduce; a test-running run that captured none
/// cannot back a reproducibility claim.
fn derive_reproducibility(facts: &CompletenessFacts) -> CompletenessState {
    if !facts.outputs_empty {
        return CompletenessState::Complete;
    }
    if facts.skip_tests {
        CompletenessState::NotApplicable
    } else {
        CompletenessState::Incomplete
    }
}

/// `tool_qualification`: a real claimed level plus the compliance
/// reports binding the standards pack. The DAL-A auxiliary-MC/DC
/// gate runs upstream of finalize, so any surviving DAL-A claim
/// has already proven its auxiliary evidence.
fn derive_tool_qualification(facts: &CompletenessFacts) -> CompletenessState {
    if !facts.any_dal_claim {
        return CompletenessState::NotApplicable;
    }
    if facts.compliance_reports_complete {
        CompletenessState::Complete
    } else {
        CompletenessState::Incomplete
    }
}

/// Errors from [`record_verification_state`]. Deliberately uncoded
/// (no [`crate::diagnostic::DiagnosticCode`] impl), same as
/// [`crate::corpus::CorpusError`]: the patch runs inside the CLI's
/// generate pipeline, which owns the diagnostic surface.
#[derive(Debug, thiserror::Error)]
pub enum CompletenessError {
    /// Reading or writing `index.json` failed.
    #[error("{op} {path:?}")]
    Io {
        /// Operation attempted (`"reading"` / `"writing"`).
        op: &'static str,
        /// File whose access failed.
        path: std::path::PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// `index.json` did not parse as an [`super::EvidenceIndex`].
    #[error("parsing {path:?} as index.json")]
    Parse {
        /// File that failed to parse.
        path: std::path::PathBuf,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },
    /// Re-serializing the patched index failed.
    #[error("serializing patched index.json")]
    Serialize {
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },
}

/// Patch `completeness.verification` on an already-finalized
/// bundle's `index.json`. The CLI calls this with the immediate
/// post-generation verification outcome, BEFORE the ed25519
/// signature seals the `(SHA256SUMS, index.json)` envelope, so the
/// sealed bytes carry the recorded outcome. `index.json` is
/// metadata-layer (excluded from `SHA256SUMS`), so the patch leaves
/// `content_hash` untouched.
///
/// Only the `verification` field is patched — every other state is
/// derive-time final. Passing [`CompletenessState::Complete`] for a
/// bundle verification rejected is exactly the lie this function
/// exists to prevent; callers pass the outcome they observed.
pub fn record_verification_state(
    bundle: &Path,
    state: CompletenessState,
) -> Result<(), CompletenessError> {
    let path = bundle.join("index.json");
    let bytes = std::fs::read(&path).map_err(|source| CompletenessError::Io {
        op: "reading",
        path: path.clone(),
        source,
    })?;
    let mut index: super::EvidenceIndex =
        serde_json::from_slice(&bytes).map_err(|source| CompletenessError::Parse {
            path: path.clone(),
            source,
        })?;
    index.completeness.verification = state;
    let patched = serde_json::to_vec_pretty(&index)
        .map_err(|source| CompletenessError::Serialize { source })?;
    std::fs::write(&path, patched).map_err(|source| CompletenessError::Io {
        op: "writing",
        path: path.clone(),
        source,
    })
}

// Tests live in a sibling file pulled in via `#[path]` so this
// module stays under the workspace 500-line limit.
#[cfg(test)]
#[path = "completeness/tests.rs"]
mod tests;
