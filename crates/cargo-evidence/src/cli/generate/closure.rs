//! Generator closure — run the release verifier against the just-written
//! bundle and refuse `GENERATE_OK` if it would reject it. A generated
//! bundle is not "complete" when the same tool rejects the unchanged
//! bundle (LLR-097). A sibling of `generate.rs`.

use anyhow::Result;

use evidence_core::{Profile, VerifyError, VerifyResult, verify_bundle};

use super::super::args::EXIT_VERIFICATION_FAILURE;
use super::{fail, fail_jsonl};

/// Run `verify_bundle` against the freshly written bundle. A
/// pre-release-tool finding on `dev` is non-blocking (mirrors `verify`'s
/// dev downgrade); a skipped verification is tolerated on `dev` but
/// blocks cert/record (a cert bundle whose verification could not run is
/// not complete); every other finding, and all findings on cert/record,
/// blocks. Emits `GENERATE_FAIL` and returns `Some(exit)` to fail
/// generation, or `None` to proceed to success.
pub(super) fn generator_closure(
    bundle_path: &std::path::Path,
    profile: Profile,
    jsonl_output: bool,
    json_output: bool,
) -> Result<Option<i32>> {
    let blocking: Vec<String> = match verify_bundle(bundle_path) {
        Ok(result) => closure_blocking(&result, profile),
        Err(e) => vec![format!("release verifier could not run: {e}")],
    };
    if blocking.is_empty() {
        return Ok(None);
    }
    let msg = format!(
        "release verifier rejected the generated bundle ({} finding(s)): {}",
        blocking.len(),
        blocking.join("; ")
    );
    if jsonl_output {
        fail_jsonl(profile, msg)?;
    } else {
        fail(json_output, profile, msg)?;
    }
    Ok(Some(EXIT_VERIFICATION_FAILURE))
}

/// The generator-closure decision for a *completed* verification: the
/// findings that block `GENERATE_OK` (empty ⇒ proceed). A pass never
/// blocks; a skipped verification is tolerated on `dev` but blocks
/// cert/record (a cert bundle whose verification could not run is not
/// complete); a failure blocks on every profile except the `dev`
/// pre-release-tool downgrade. The verifier failing to *run at all*
/// (an I/O error from `verify_bundle`) is handled by the caller, not
/// here, so this stays a pure function of `(result, profile)`.
fn closure_blocking(result: &VerifyResult, profile: Profile) -> Vec<String> {
    let cert_or_record = matches!(profile, Profile::Cert | Profile::Record);
    match result {
        VerifyResult::Pass => Vec::new(),
        VerifyResult::Skipped(_) if !cert_or_record => Vec::new(),
        VerifyResult::Skipped(reason) => vec![format!(
            "release verifier was skipped on a {profile} bundle: {reason}"
        )],
        VerifyResult::Fail(errors) => errors
            .iter()
            .filter(|e| verify_error_blocks_generate(e, profile))
            .map(ToString::to_string)
            .collect(),
    }
}

/// A pre-release-tool finding is non-blocking on `dev` only; every other
/// verify finding blocks generation on every profile.
fn verify_error_blocks_generate(e: &VerifyError, profile: Profile) -> bool {
    !matches!(
        (e, profile),
        (VerifyError::PrereleaseToolDetected { .. }, Profile::Dev)
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn prerelease_tool_is_non_blocking_only_on_dev() {
        let prerelease = VerifyError::PrereleaseToolDetected {
            profile: "dev".to_string(),
            engine_crate_version: "0.0.0-pre".to_string(),
        };
        // Non-blocking on dev (mirrors verify's dev downgrade)…
        assert!(!verify_error_blocks_generate(&prerelease, Profile::Dev));
        // …but blocking on cert / record.
        assert!(verify_error_blocks_generate(&prerelease, Profile::Cert));
        assert!(verify_error_blocks_generate(&prerelease, Profile::Record));
    }

    #[test]
    fn every_other_finding_blocks_on_every_profile() {
        let empty_baseline = VerifyError::SourceBaselineEmpty;
        for p in [Profile::Dev, Profile::Cert, Profile::Record] {
            assert!(verify_error_blocks_generate(&empty_baseline, p));
        }
    }

    /// Full Pass/Skipped/Fail × Dev/Cert/Record decision matrix. Pass
    /// never blocks; Skipped blocks only on cert/record; a
    /// (non-pre-release) Fail blocks on every profile.
    #[test]
    fn closure_decision_matrix() {
        let make_pass = || VerifyResult::Pass;
        let make_skipped = || VerifyResult::Skipped("no verification key".to_string());
        let make_fail = || VerifyResult::Fail(vec![VerifyError::SourceBaselineEmpty]);

        // (result factory, profile, should-block). A factory rather
        // than a value because `VerifyResult` isn't `Copy`.
        type Case = (fn() -> VerifyResult, Profile, bool);
        let cases: &[Case] = &[
            (make_pass, Profile::Dev, false),
            (make_pass, Profile::Cert, false),
            (make_pass, Profile::Record, false),
            (make_skipped, Profile::Dev, false),
            (make_skipped, Profile::Cert, true),
            (make_skipped, Profile::Record, true),
            (make_fail, Profile::Dev, true),
            (make_fail, Profile::Cert, true),
            (make_fail, Profile::Record, true),
        ];

        for (make, profile, expect_block) in cases {
            let blocking = closure_blocking(&make(), *profile);
            assert_eq!(
                !blocking.is_empty(),
                *expect_block,
                "profile={profile}: blocking={blocking:?}, expected_block={expect_block}"
            );
        }
    }
}
