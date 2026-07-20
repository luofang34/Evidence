//! JSON-envelope builders for `cargo evidence generate` output.
//!
//! Split out of `phases.rs` so the success-envelope `success`-
//! field logic is unit-testable without driving the full
//! generate pipeline. The `success` field reflects "did every
//! captured subprocess exit 0" — NOT "did the bundle assemble
//! cleanly." The two are distinct signals: dev profile with a
//! failing `cargo test` still assembles a bundle (exit 0), but
//! `success: false` tells scripts the captured commands aren't
//! complete evidence.

use std::path::Path;

use anyhow::Result;
use evidence_core::diagnostic::{Diagnostic, Severity};
use evidence_core::{EnvFingerprint, Profile};

use super::GenerateOutput;
use crate::cli::output::{emit_json, emit_jsonl};

/// Emit a failure envelope and return EXIT_ERROR.
///
/// Collapses the `if json { emit_json(...) } else { eprintln!(...) }`
/// pattern that preflight / strict trace-validation branches share.
pub(crate) fn fail(json_output: bool, profile: Profile, msg: impl Into<String>) -> Result<i32> {
    let msg = msg.into();
    if json_output {
        emit_json(&GenerateOutput {
            success: false,
            bundle_path: None,
            profile: profile.to_string(),
            git_sha: None,
            error: Some(msg),
        })?;
    } else {
        eprintln!("error: {}", msg);
    }
    Ok(crate::cli::args::EXIT_ERROR)
}

/// JSONL-mode `fail`: emit a single `GENERATE_FAIL` terminal with the
/// failure message so agents see the outcome + reason.
pub(crate) fn fail_jsonl(profile: Profile, msg: impl Into<String>) -> Result<i32> {
    emit_jsonl(&Diagnostic {
        code: "GENERATE_FAIL".to_string(),
        severity: Severity::Error,
        message: format!("generate failed (profile={}): {}", profile, msg.into()),
        location: None,
        fix_hint: None,
        subcommand: Some("generate".to_string()),
        root_cause_uid: None,
    })?;
    Ok(crate::cli::args::EXIT_ERROR)
}

/// Build the JSON success envelope for a completed generate run.
/// `success = recorded_failures == 0`; when non-zero,
/// `error` carries a short message so `--json` consumers can
/// render the failure count without parsing the bundle.
pub(super) fn build_success_envelope(
    bundle_path: &Path,
    profile: Profile,
    env_fp: &EnvFingerprint,
    recorded_failures: usize,
) -> GenerateOutput {
    let success = recorded_failures == 0;
    GenerateOutput {
        success,
        bundle_path: Some(bundle_path.display().to_string()),
        profile: profile.to_string(),
        git_sha: Some(env_fp.git_sha.clone()),
        error: if success {
            None
        } else {
            Some(format!(
                "{} captured command(s) exited non-zero; bundle_complete=false",
                recorded_failures
            ))
        },
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
    use std::path::PathBuf;

    fn sample_env() -> EnvFingerprint {
        EnvFingerprint {
            profile: Profile::Dev,
            rustc: "rustc 1.85.0".to_string(),
            cargo: "cargo 1.85.0".to_string(),
            git_sha: "aabbccdd11223344aabbccdd11223344aabbccdd".to_string(),
            git_branch: "main".to_string(),
            git_dirty: false,
            in_nix_shell: false,
            tools: BTreeMap::new(),
            nav_env: BTreeMap::new(),
            llvm_version: None,
            host: evidence_core::Host::Linux {
                arch: "x86_64".to_string(),
                libc: None,
                kernel: None,
            },
            cargo_lock_hash: None,
            rust_toolchain_toml: None,
            rustflags: None,
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            tool_prerelease: false,
        }
    }

    /// Zero recorded failures → `success: true`, `error: None`.
    /// The old (pre-cleanup) behavior; pinned here so a future
    /// regression on the happy path fires loud.
    #[test]
    fn zero_failures_envelope_is_success_true() {
        let env = sample_env();
        let out = build_success_envelope(&PathBuf::from("/tmp/bundle"), Profile::Dev, &env, 0);
        assert!(out.success, "0 failures ⇒ success: true");
        assert!(out.error.is_none(), "0 failures ⇒ error: None");
    }

    /// Non-zero recorded failures on dev profile → `success: false`,
    /// `error: Some(...)`. Exit code STILL 0 (dev allows incomplete
    /// bundles for local iteration) but the envelope reflects the
    /// incomplete state so `jq .success` in a CI script catches it
    /// without parsing the bundle.
    #[test]
    fn recorded_failures_flip_success_to_false_on_dev() {
        let env = sample_env();
        let out = build_success_envelope(&PathBuf::from("/tmp/bundle"), Profile::Dev, &env, 3);
        assert!(!out.success, "recorded failures ⇒ success: false");
        let err = out.error.expect("error message present");
        assert!(
            err.contains("3 captured"),
            "error message should carry the count, got {err}",
        );
        assert!(
            err.contains("bundle_complete=false"),
            "error message should name the on-disk signal, got {err}",
        );
    }

    /// Same flip on cert profile — envelope shape is profile-
    /// agnostic. Exit-code propagation is separate logic in
    /// `cmd_generate` (EXIT_VERIFICATION_FAILURE on cert/record
    /// with recorded failures).
    #[test]
    fn recorded_failures_flip_success_to_false_on_cert() {
        let env = sample_env();
        let out = build_success_envelope(&PathBuf::from("/tmp/bundle"), Profile::Cert, &env, 1);
        assert!(!out.success);
        assert_eq!(out.profile, "cert");
    }
}
