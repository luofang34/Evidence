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

/// Command names whose recorded failure implies `test_summary`
/// should be populated on `EvidenceIndex`. Exact-match (not
/// prefix): a hypothetical `cargo test-bench-all` subcommand
/// wouldn't populate our TestSummary shape, so firing the
/// invariant against it would be wrong. When a new captured
/// subprocess starts populating `test_summary`, add it here
/// and the test at the bottom of this module pins the shape.
pub const COMMANDS_OWNING_TEST_SUMMARY: &[&str] = &["cargo test --workspace"];

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

    // The test_summary consistency check is tied to the small
    // set of captured subprocesses that populate TestSummary.
    // Exact-match against COMMANDS_OWNING_TEST_SUMMARY — not a
    // prefix — so a future `cargo test-bench-all`-style
    // subcommand doesn't falsely fire the invariant.
    for failure in failures {
        let owns_test_summary = COMMANDS_OWNING_TEST_SUMMARY
            .iter()
            .any(|c| *c == failure.command_name);
        if owns_test_summary && index.test_summary.is_none() {
            errors.push(VerifyError::TestSummaryAbsentOnFailedRun {
                command_name: failure.command_name.clone(),
            });
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;

    /// Pin the exact list of commands the invariant considers
    /// responsible for `test_summary`. Bumping it is a
    /// deliberate edit — and a hint that a new producer needs
    /// to grow a `test_summary` companion.
    #[test]
    fn commands_owning_test_summary_is_exactly_cargo_test_workspace() {
        assert_eq!(COMMANDS_OWNING_TEST_SUMMARY, &["cargo test --workspace"]);
    }

    /// Non-matching command names do NOT fire the invariant
    /// even when `test_summary` is absent. Guards against the
    /// old `starts_with("cargo test")` prefix-match footgun.
    #[test]
    fn unrelated_cargo_subcommand_does_not_fire_invariant() {
        use crate::bundle::ToolCommandFailure;
        let mut errors = Vec::new();
        let index = EvidenceIndex {
            schema_version: crate::schema_versions::INDEX.to_string(),
            boundary_schema_version: crate::schema_versions::BOUNDARY.to_string(),
            trace_schema_version: crate::schema_versions::TRACE.to_string(),
            profile: Profile::Dev,
            timestamp_rfc3339: "2026-01-01T00:00:00Z".to_string(),
            git_sha: "0".repeat(40),
            git_branch: "main".to_string(),
            git_dirty: false,
            engine_crate_version: "0.1.0".to_string(),
            engine_git_sha: "0".repeat(40),
            engine_build_source: "git".to_string(),
            inputs_hashes_file: "inputs_hashes.json".to_string(),
            outputs_hashes_file: "outputs_hashes.json".to_string(),
            commands_file: "commands.json".to_string(),
            env_fingerprint_file: "env.json".to_string(),
            trace_roots: vec![],
            trace_outputs: vec![],
            bundle_complete: false,
            content_hash: "0".repeat(64),
            deterministic_hash: "0".repeat(64),
            test_summary: None,
            tool_command_failures: vec![ToolCommandFailure {
                command_name: "cargo test-bench-all".to_string(),
                exit_code: 1,
                stderr_tail: String::new(),
            }],
            dal_map: std::collections::BTreeMap::new(),
        };
        check_bundle_completeness(&index, &mut errors);
        assert!(
            !errors
                .iter()
                .any(|e| matches!(e, VerifyError::TestSummaryAbsentOnFailedRun { .. })),
            "`cargo test-bench-all` must not fire TestSummaryAbsentOnFailedRun; got {errors:?}",
        );
    }
}
