//! The `verify --json` envelope (`VerifyOutput`) plus the
//! best-effort index read that populates its profile +
//! completeness fields. Split from `cli/verify.rs` to keep the
//! parent under the 500-line workspace limit; same pattern as
//! the sibling `terminals.rs` / `key_resolve.rs`.

use serde::Serialize;

/// One named check row in the envelope.
#[derive(Serialize)]
pub(super) struct VerifyCheck {
    pub(super) name: String,
    pub(super) status: String,
    pub(super) message: Option<String>,
}

/// The JSON envelope emitted by `cargo evidence verify --json`
/// on every outcome path (pass / fail / skipped / error).
#[derive(Serialize)]
pub(super) struct VerifyOutput {
    pub(super) success: bool,
    pub(super) bundle_path: String,
    pub(super) checks: Vec<VerifyCheck>,
    /// The profile the bundle was generated under — the named
    /// claim context the recorded states were derived for.
    /// `None` when `index.json` could not be read (the error
    /// checks then carry the reason).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) profile: Option<String>,
    /// The bundle's recorded per-area completeness states
    /// (LLR-149). Reported, not re-derived: verification checks
    /// integrity and policy consistency; the states are the
    /// generator's own per-area account, included so a consumer
    /// sees exactly which areas the bundle claims complete and
    /// which it honestly marks incomplete or unverifiable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) completeness: Option<evidence_core::CompletenessStates>,
    pub(super) error: Option<String>,
}

/// Best-effort read of the bundle's profile + recorded
/// completeness states for the JSON envelope. `None` on any
/// failure — the checks vector already carries the parse verdict.
pub(super) fn index_states(
    bundle_path: &std::path::Path,
) -> (Option<String>, Option<evidence_core::CompletenessStates>) {
    let content = std::fs::read_to_string(bundle_path.join("index.json")).ok();
    let index: Option<evidence_core::EvidenceIndex> =
        content.and_then(|c| serde_json::from_str(&c).ok());
    match index {
        Some(index) => (Some(index.profile.to_string()), Some(index.completeness)),
        None => (None, None),
    }
}
