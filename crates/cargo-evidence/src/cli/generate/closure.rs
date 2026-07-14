//! Generator closure — run the release verifier against the just-written
//! bundle and refuse `GENERATE_OK` if it would reject it. A generated
//! bundle is not "complete" when the same tool rejects the unchanged
//! bundle (issue #140 / LLR-097). Pulled out of `generate.rs` so the
//! orchestrator stays under the 500-line limit.

use anyhow::Result;

use evidence_core::{Profile, VerifyError, VerifyResult, verify_bundle};

use super::super::args::EXIT_VERIFICATION_FAILURE;
use super::{fail, fail_jsonl};

/// Run `verify_bundle` against the freshly written bundle. A
/// pre-release-tool finding on `dev` is non-blocking (mirrors `verify`'s
/// dev downgrade); every other finding, and all findings on cert/record,
/// blocks. Emits `GENERATE_FAIL` and returns `Some(exit)` to fail
/// generation, or `None` to proceed to success.
pub(super) fn generator_closure(
    bundle_path: &std::path::Path,
    profile: Profile,
    jsonl_output: bool,
    json_output: bool,
) -> Result<Option<i32>> {
    let blocking: Vec<String> = match verify_bundle(bundle_path) {
        Ok(VerifyResult::Pass) | Ok(VerifyResult::Skipped(_)) => return Ok(None),
        Ok(VerifyResult::Fail(errors)) => errors
            .iter()
            .filter(|e| verify_error_blocks_generate(e, profile))
            .map(ToString::to_string)
            .collect(),
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
}
