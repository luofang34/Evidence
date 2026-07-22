//! Verify-time fail-closed check on the bundle's recorded
//! dependency-resolution policy (LLR-142).
//!
//! A bundle records the [`ResolutionPolicy`] it was generated under
//! in `index.json`. `online_opt_in` means cargo was allowed to reach
//! the network during generation — legitimate for development
//! iteration, but such a bundle can never back a certification or
//! record claim. The generate-time gate refuses `--online` on
//! cert/record profiles before any bundle work; this check is the
//! defense-in-depth half: a hand-assembled or tampered bundle that
//! pairs `online_opt_in` with a cert/record profile is rejected at
//! verification even when the generator was bypassed.
//!
//! Bundles written before the field existed deserialize as
//! `locked_offline` (the safe default), so legacy cert/record
//! bundles keep verifying.

use crate::bundle::EvidenceIndex;
use crate::policy::{Profile, ResolutionPolicy};

use super::errors::VerifyError;

/// Push [`VerifyError::OnlineResolutionBundle`] when the bundle
/// records `resolution_policy = online_opt_in` under a cert/record
/// profile. Development-profile bundles under the opt-in verify
/// normally — the flag narrows what the bundle can claim, not its
/// integrity.
pub fn check_resolution_policy(index: &EvidenceIndex, errors: &mut Vec<VerifyError>) {
    if index.resolution_policy == ResolutionPolicy::OnlineOptIn
        && matches!(index.profile, Profile::Cert | Profile::Record)
    {
        errors.push(VerifyError::OnlineResolutionBundle {
            profile: index.profile.to_string(),
        });
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
    use std::collections::BTreeMap;

    fn index_with(profile: Profile, policy: ResolutionPolicy) -> EvidenceIndex {
        EvidenceIndex {
            schema_version: crate::schema_versions::INDEX.to_string(),
            boundary_schema_version: crate::schema_versions::BOUNDARY.to_string(),
            trace_schema_version: crate::schema_versions::TRACE.to_string(),
            profile,
            timestamp_rfc3339: "2024-01-01T00:00:00Z".to_string(),
            git_sha: "abc123".to_string(),
            git_branch: "main".to_string(),
            git_dirty: false,
            engine_crate_version: "0.1.0".to_string(),
            engine_git_sha: "abc123".to_string(),
            engine_build_source: "git".to_string(),
            inputs_hashes_file: "inputs_hashes.json".to_string(),
            outputs_hashes_file: "outputs_hashes.json".to_string(),
            commands_file: "commands.json".to_string(),
            env_fingerprint_file: "env.json".to_string(),
            trace_roots: Vec::new(),
            trace_outputs: Vec::new(),
            bundle_complete: true,
            content_hash: "deadbeef".repeat(8),
            recipe_hash: "cafebabe".repeat(8),
            test_summary: None,
            tool_command_failures: Vec::new(),
            dal_map: BTreeMap::new(),
            boundary_policy: crate::policy::BoundaryPolicy::default(),
            resolution_policy: policy,
        }
    }

    /// TEST-159 (b): `online_opt_in` + cert/record ⇒ rejected.
    #[test]
    fn online_opt_in_cert_bundle_is_rejected() {
        for profile in [Profile::Cert, Profile::Record] {
            let idx = index_with(profile, ResolutionPolicy::OnlineOptIn);
            let mut errors = Vec::new();
            check_resolution_policy(&idx, &mut errors);
            assert!(
                matches!(
                    errors.as_slice(),
                    [VerifyError::OnlineResolutionBundle { .. }]
                ),
                "{profile} + online_opt_in must be rejected: {errors:?}"
            );
        }
    }

    /// TEST-159 (c): `online_opt_in` + dev ⇒ allowed (the opt-in is
    /// legitimate for development; it narrows the claim, not the
    /// integrity).
    #[test]
    fn online_opt_in_dev_bundle_is_allowed() {
        let idx = index_with(Profile::Dev, ResolutionPolicy::OnlineOptIn);
        let mut errors = Vec::new();
        check_resolution_policy(&idx, &mut errors);
        assert!(errors.is_empty(), "dev + online_opt_in must verify");
    }

    /// TEST-159 (d): `locked_offline` + cert ⇒ allowed (and the
    /// legacy-default path: old bundles parse to `locked_offline`).
    #[test]
    fn locked_offline_cert_bundle_is_allowed() {
        let idx = index_with(Profile::Cert, ResolutionPolicy::LockedOffline);
        let mut errors = Vec::new();
        check_resolution_policy(&idx, &mut errors);
        assert!(errors.is_empty(), "cert + locked_offline must verify");
    }
}
