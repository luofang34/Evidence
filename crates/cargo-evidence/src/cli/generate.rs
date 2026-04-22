//! `cargo evidence generate`.
//!
//! Implemented as a linear pipeline of phase functions (see
//! [`phases`]). Each phase is one bundle-building concern (preflight,
//! env capture, source hashing, test capture, trace validation, trace
//! copy, compliance, finalize, emit). [`cmd_generate`] wires them
//! together; the pure helpers in this file (`resolve_profile`,
//! `resolve_output_root`, `split_trace_roots_flag`) are unit-tested
//! below, and the I/O-bound phase bodies are covered end-to-end by
//! the `cargo-evidence evidence generate` integration tests.

mod coverage_phase;
mod envelope;
mod phases;
mod policy;
mod test_outcomes;

use std::path::PathBuf;

use anyhow::Result;
use serde::Serialize;

use evidence_core::diagnostic::{Diagnostic, Severity};
use evidence_core::{EvidencePolicy, Profile};

use super::args::{EXIT_ERROR, EXIT_SUCCESS, EXIT_VERIFICATION_FAILURE, detect_profile};
use super::output::emit_json;

// ============================================================================
// Output envelope (shared with phases.rs)
// ============================================================================

#[derive(Serialize)]
pub(super) struct GenerateOutput {
    pub(super) success: bool,
    pub(super) bundle_path: Option<String>,
    pub(super) profile: String,
    pub(super) git_sha: Option<String>,
    pub(super) error: Option<String>,
}

/// Emit a failure envelope and return EXIT_ERROR.
///
/// Collapses the `if json { emit_json(...) } else { eprintln!(...) }`
/// pattern that preflight / strict trace-validation branches share.
/// Lives here (not in phases.rs) because the pure helpers and the
/// orchestrator both reference it.
pub(super) fn fail(json_output: bool, profile: Profile, msg: impl Into<String>) -> Result<i32> {
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
    Ok(EXIT_ERROR)
}

/// JSONL-mode `fail`: emit a single `GENERATE_FAIL` terminal with the
/// failure message so agents see the outcome + reason.
pub(super) fn fail_jsonl(profile: Profile, msg: impl Into<String>) -> Result<i32> {
    use super::output::emit_jsonl;
    emit_jsonl(&Diagnostic {
        code: "GENERATE_FAIL".to_string(),
        severity: Severity::Error,
        message: format!("generate failed (profile={}): {}", profile, msg.into()),
        location: None,
        fix_hint: None,
        subcommand: Some("generate".to_string()),
        root_cause_uid: None,
    })?;
    Ok(EXIT_ERROR)
}

// ============================================================================
// CLI argument group
// ============================================================================

/// Arguments for the generate command, grouped to avoid clippy::too_many_arguments.
pub struct GenerateArgs {
    /// `--profile` override. `None` means auto-detect via
    /// [`detect_profile`].
    pub profile_arg: Option<String>,
    /// Destination directory for the generated bundle. Required unless
    /// [`write_workspace`](Self::write_workspace) is true.
    pub out_dir: Option<PathBuf>,
    /// Allow writing the bundle inside the tracked workspace tree
    /// (dangerous — will make the tree dirty for the next run).
    pub write_workspace: bool,
    /// Path to `boundary.toml`; `None` uses the workspace default.
    pub boundary: Option<PathBuf>,
    /// Comma-separated trace root list (overrides boundary.toml).
    pub trace_roots_arg: Option<String>,
    /// Path to the HMAC signing key (raw bytes). `None` means do not sign.
    pub sign_key: Option<PathBuf>,
    /// Skip the `cargo test` invocation during generation.
    pub skip_tests: bool,
    /// Structural-coverage level to capture (or `None` to apply
    /// the profile-derived default). See [`crate::cli::args::CoverageChoice`]
    /// + HLR-053.
    pub coverage: Option<crate::cli::args::CoverageChoice>,
    /// Suppress non-error stdout.
    pub quiet: bool,
    /// Emit a JSON envelope on stdout instead of human-readable text.
    pub json_output: bool,
    /// Emit a JSONL stream on stdout (`GENERATE_OK` / `GENERATE_FAIL`
    /// terminal). When true, internal phases run with `quiet=true`
    /// and `json_output=false` so the JSONL stream stays
    /// stdout-strict (Schema Rule 2).
    pub jsonl_output: bool,
}

// ============================================================================
// Pure helpers (unit-tested below)
// ============================================================================

/// Resolve the requested profile: honor the `--profile` flag if given,
/// otherwise fall through to auto-detection. Separated so the
/// precedence rule is unit-testable without spinning up an
/// `EnvFingerprint` capture or a git query.
fn resolve_profile(profile_arg: Option<&str>) -> Result<Profile> {
    match profile_arg {
        Some(p) => Ok(p.parse::<Profile>()?),
        None => Ok(detect_profile()),
    }
}

/// Decide where the bundle should be written. Explicit `--out-dir`
/// wins; `--write-workspace` falls back to `./evidence` (which will
/// dirty the tree — see memory `project_outdir_dirties_tree`). `None`
/// means neither was provided — the caller must surface the
/// "missing --out-dir" error.
fn resolve_output_root(out_dir: Option<PathBuf>, write_workspace: bool) -> Option<PathBuf> {
    if let Some(dir) = out_dir {
        return Some(dir);
    }
    if write_workspace {
        return Some(PathBuf::from("evidence"));
    }
    None
}

/// Split a comma-separated `--trace-roots` flag into individual paths
/// with whitespace trimmed. Empty segments (e.g. trailing commas) are
/// preserved as empty strings so we don't silently discard what the
/// user typed — validation of "does this path exist" happens later.
pub(super) fn split_trace_roots_flag(s: &str) -> Vec<String> {
    s.split(',').map(|t| t.trim().to_string()).collect()
}

// ============================================================================
// Orchestrator
// ============================================================================

/// `cargo evidence generate` handler: the default subcommand. Runs
/// preflight checks (profile, shallow clone, dirty tree, boundary
/// config), collects env/git/test fingerprints, and writes the bundle
/// to `args.out_dir`. Returns
/// [`EXIT_SUCCESS`] or
/// [`EXIT_ERROR`].
pub fn cmd_generate(args: GenerateArgs) -> Result<i32> {
    let GenerateArgs {
        profile_arg,
        out_dir,
        write_workspace,
        boundary,
        trace_roots_arg,
        sign_key,
        skip_tests,
        coverage,
        quiet,
        json_output,
        jsonl_output,
    } = args;

    // JSONL mode is stdout-strict (Schema Rule 2): no internal
    // phase helper may print human or JSON-envelope text. Force
    // quiet + clear json_output so they stay silent, then this
    // orchestrator owns the stream.
    let (quiet, json_output) = if jsonl_output {
        (true, false)
    } else {
        (quiet, json_output)
    };
    let fail_dispatch = |profile: Profile, msg: String| -> Result<i32> {
        if jsonl_output {
            fail_jsonl(profile, msg)
        } else {
            fail(json_output, profile, msg)
        }
    };

    let profile = resolve_profile(profile_arg.as_deref())?;
    // Doctor precheck gates cert / record profile bundle generation —
    // downstream projects can't produce audit evidence without
    // passing the rigor checklist (trace validity, floors config,
    // boundary config). Dev profile skips the precheck so iteration
    // stays fast.
    //
    // Runs BEFORE `preflight` because doctor findings are actionable
    // ("add cert/boundary.toml") while preflight's dirty-tree / git-
    // state errors are stop-ship. A first-time downstream user with
    // no boundary config should see "boundary missing" before "dirty
    // tree", not after. See LLR-048 / cli/doctor.rs::precheck_doctor.
    if matches!(profile, Profile::Cert | Profile::Record) {
        let workspace = std::env::current_dir()?;
        if let Err(e) = super::doctor::precheck_doctor(&workspace) {
            return fail_dispatch(profile, e.to_string());
        }
    }
    if let Some(code) = phases::preflight(profile, json_output)? {
        return Ok(code);
    }
    let Some(output_root) = resolve_output_root(out_dir, write_workspace) else {
        return fail_dispatch(
            profile,
            "--out-dir is required unless --write-workspace is specified".to_string(),
        );
    };

    let boundary_path = boundary.unwrap_or_else(|| PathBuf::from("cert/boundary.toml"));
    let (config, derived) =
        phases::build_config(profile, output_root, &boundary_path, trace_roots_arg);
    if let Some(code) = policy::enforce_boundary_policy(&derived, profile, json_output)? {
        return Ok(code);
    }
    let strict = matches!(profile, Profile::Cert | Profile::Record);
    let mut builder = match phases::init_builder(config, profile, quiet, json_output)? {
        Ok(b) => b,
        Err(code) => return Ok(code),
    };
    let env_fp = phases::capture_and_write_env(&builder, profile)?;
    phases::hash_in_scope_sources(
        &mut builder,
        &derived.in_scope_crates,
        strict,
        quiet,
        json_output,
    )?;
    builder.write_inputs()?;
    builder.write_outputs()?;
    builder.write_commands()?;

    phases::run_tests_and_capture(&mut builder, skip_tests, strict, quiet, json_output)?;
    builder.write_outputs()?;
    builder.write_commands()?;

    // Phase 5b — structural coverage via cargo-llvm-cov. When
    // the effective choice is `none` this returns `Skipped`
    // without spawning anything. Cert/record + missing binary
    // short-circuits to GENERATE_FAIL.
    let effective_coverage = coverage_phase::resolve_choice(coverage, profile);
    let coverage_outcome = coverage_phase::run_coverage_phase(
        &builder,
        effective_coverage,
        profile,
        quiet,
        jsonl_output,
    )?;
    if matches!(
        coverage_outcome,
        coverage_phase::CoverageOutcome::LlvmCovMissingCert
    ) {
        return fail_dispatch(
            profile,
            "cargo-llvm-cov missing; cert/record profiles require structural coverage".to_string(),
        );
    }

    let policy = EvidencePolicy::for_dal(derived.max_dal);
    if let Some(code) = phases::validate_trace_links_phase(
        &derived.trace_roots,
        &policy,
        profile,
        strict,
        quiet,
        json_output,
    )? {
        return Ok(code);
    }

    // Phase 6b — enrich stored test outcomes with per-test →
    // LLR back-links, then write `tests/test_outcomes.jsonl`.
    // Runs after trace validation so LLR data is available.
    test_outcomes::enrich_and_write_test_outcomes(&mut builder, &derived.trace_roots)?;

    let trace_outputs =
        phases::copy_trace_and_build_matrix(&builder, &derived.trace_roots, quiet, json_output)?;
    phases::write_compliance_reports(
        &builder,
        &derived.dal_map,
        &derived.trace_roots,
        quiet,
        json_output,
    )?;

    // Snapshot tool_command_failures before the builder is
    // moved into finalize_and_sign. Non-empty + Cert|Record
    // profile → propagate the non-zero exit; dev profile still
    // returns 0 so local iteration on a half-broken workspace
    // produces an inspectable bundle.
    let recorded_failures = builder.tool_command_failures().len();

    let bundle_path =
        phases::finalize_and_sign(builder, trace_outputs, sign_key, quiet, json_output)?;
    if !jsonl_output {
        phases::emit_success_envelope(
            json_output,
            quiet,
            &bundle_path,
            profile,
            &env_fp,
            recorded_failures,
        )?;
    }

    if recorded_failures > 0 && matches!(profile, Profile::Cert | Profile::Record) {
        if jsonl_output {
            super::output::emit_jsonl(&Diagnostic {
                code: "GENERATE_FAIL".to_string(),
                severity: Severity::Error,
                message: format!(
                    "profile={}: {} captured command(s) exited non-zero; \
                     bundle_complete=false",
                    profile, recorded_failures
                ),
                location: Some(evidence_core::Location {
                    file: Some(bundle_path),
                    ..evidence_core::Location::default()
                }),
                fix_hint: None,
                subcommand: Some("generate".to_string()),
                root_cause_uid: None,
            })?;
        } else {
            tracing::warn!(
                "{} captured command(s) exited non-zero; cert/record bundle marked \
                 bundle_complete=false — generate returning non-zero exit to signal",
                recorded_failures
            );
        }
        return Ok(EXIT_VERIFICATION_FAILURE);
    }

    if jsonl_output {
        super::output::emit_jsonl(&Diagnostic {
            code: "GENERATE_OK".to_string(),
            severity: Severity::Info,
            message: format!(
                "generate produced bundle at {} (profile={})",
                bundle_path.display(),
                profile
            ),
            location: Some(evidence_core::Location {
                file: Some(bundle_path),
                ..evidence_core::Location::default()
            }),
            fix_hint: None,
            subcommand: Some("generate".to_string()),
            root_cause_uid: None,
        })?;
    }
    Ok(EXIT_SUCCESS)
}

// ============================================================================
// Unit tests for pure helpers
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;

    // ---- resolve_profile --------------------------------------------------

    #[test]
    fn resolve_profile_parses_explicit_flag() {
        assert_eq!(resolve_profile(Some("dev")).unwrap(), Profile::Dev);
        assert_eq!(resolve_profile(Some("cert")).unwrap(), Profile::Cert);
        assert_eq!(resolve_profile(Some("record")).unwrap(), Profile::Record);
    }

    #[test]
    fn resolve_profile_rejects_garbage() {
        assert!(resolve_profile(Some("nonsense")).is_err());
        assert!(resolve_profile(Some("")).is_err());
    }

    #[test]
    fn resolve_profile_none_falls_through_to_detect() {
        // detect_profile reads env vars (NAV_RECORD, IN_NIX_SHELL, CI)
        // whose values vary by host. The contract we're pinning is
        // "None doesn't error", not which profile detect returned.
        assert!(resolve_profile(None).is_ok());
    }

    // ---- resolve_output_root ---------------------------------------------

    #[test]
    fn resolve_output_root_prefers_explicit_out_dir() {
        let explicit = PathBuf::from("/tmp/explicit");
        let got = resolve_output_root(Some(explicit.clone()), false);
        assert_eq!(got, Some(explicit));
    }

    #[test]
    fn resolve_output_root_explicit_wins_over_workspace() {
        let explicit = PathBuf::from("/tmp/explicit");
        let got = resolve_output_root(Some(explicit.clone()), true);
        assert_eq!(
            got,
            Some(explicit),
            "--out-dir must override --write-workspace"
        );
    }

    #[test]
    fn resolve_output_root_workspace_falls_back_to_evidence_dir() {
        let got = resolve_output_root(None, true);
        assert_eq!(got, Some(PathBuf::from("evidence")));
    }

    #[test]
    fn resolve_output_root_none_when_neither_given() {
        assert_eq!(resolve_output_root(None, false), None);
    }

    // ---- split_trace_roots_flag ------------------------------------------

    #[test]
    fn split_trace_roots_flag_splits_on_commas() {
        let got = split_trace_roots_flag("cert/hlr,cert/llr,cert/tests");
        assert_eq!(got, vec!["cert/hlr", "cert/llr", "cert/tests"]);
    }

    #[test]
    fn split_trace_roots_flag_trims_whitespace() {
        let got = split_trace_roots_flag("  a ,\t b \t, c  ");
        assert_eq!(got, vec!["a", "b", "c"]);
    }

    #[test]
    fn split_trace_roots_flag_preserves_empty_segments() {
        let got = split_trace_roots_flag("a,,b,");
        assert_eq!(got, vec!["a", "", "b", ""]);
    }

    #[test]
    fn split_trace_roots_flag_single_entry() {
        assert_eq!(split_trace_roots_flag("only"), vec!["only"]);
    }

    #[test]
    fn split_trace_roots_flag_empty_string_yields_one_empty() {
        assert_eq!(split_trace_roots_flag(""), vec![""]);
    }
}
