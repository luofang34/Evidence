//! `check_bundle_completeness` — verify-time cross-check of
//! `bundle_complete` ↔ `tool_command_failures` ↔ `test_summary`.
//!
//! Carved out of `verify/bundle.rs` to keep that facade under
//! the 500-line limit. The function is shape-identical to the
//! existing `check_env_vs_index` / `check_prerelease_tool`
//! helpers: takes the parsed index, pushes onto the shared
//! error vec.
//!
//! See `EvidenceBuilder::tool_command_failures` for the wiring
//! that makes this cross-check meaningful at generate time.

use super::errors::VerifyError;
use crate::bundle::EvidenceIndex;
use crate::policy::Profile;

/// Cross-check `bundle_complete` ↔ `tool_command_failures` ↔
/// `test_summary`. Three distinct failure modes, each mapped to
/// its own [`VerifyError`] variant:
///
/// 1. `bundle_complete=true` + non-empty `tool_command_failures`:
///    tamper or generator bug (these fields are wired strict-
///    consistent at finalize time). Fires
///    [`VerifyError::BundleIncompletelyClaimed`] on any profile.
///
/// 2. `bundle_complete=false` + profile ∈ {cert, record}:
///    cert/record bundles must not ship with recorded failures.
///    Fires [`VerifyError::ToolCommandsFailedSilently`]. Dev
///    profile with `bundle_complete=false` is a CLI-layer
///    Warning, NOT a VerifyError — library stays policy-free
///    the way the prerelease check does.
///
/// 3. `tool_command_failures` has a cargo-test row but
///    `test_summary` is absent: the two fields have drifted.
///    Fires [`VerifyError::TestSummaryAbsentOnFailedRun`].
pub fn check_bundle_completeness(index: &EvidenceIndex, errors: &mut Vec<VerifyError>) {
    let failures = &index.tool_command_failures;

    if index.bundle_complete && !failures.is_empty() {
        errors.push(VerifyError::BundleIncompletelyClaimed {
            failure_count: failures.len(),
        });
    }

    if !index.bundle_complete && matches!(index.profile, Profile::Cert | Profile::Record) {
        errors.push(VerifyError::ToolCommandsFailedSilently {
            profile: index.profile.to_string(),
            commands: failures.iter().map(|f| f.command_name.clone()).collect(),
        });
    }

    // The test_summary consistency check is tied to cargo-test
    // specifically (other captured subprocesses don't populate
    // test_summary at all). Match by command_name prefix so
    // plain `cargo test --workspace` AND `cargo test -- --nocapture`
    // variants both trigger the check.
    for failure in failures {
        if failure.command_name.starts_with("cargo test") && index.test_summary.is_none() {
            errors.push(VerifyError::TestSummaryAbsentOnFailedRun {
                command_name: failure.command_name.clone(),
            });
        }
    }
}
