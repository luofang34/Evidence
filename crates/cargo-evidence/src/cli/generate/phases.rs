//! Phase functions for `cargo evidence generate`.
//!
//! Each phase is a small, single-purpose helper called in order by
//! [`super::cmd_generate`]. Short-circuiting phases (preflight
//! gates, strict-mode trace-validation failure) return
//! `Result<Option<i32>>`; I/O-only phases return `Result<()>`.
//! Visibility is `pub(super)`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use evidence_core::{
    BoundaryConfig, BoundaryPolicy, Dal, EnvFingerprint, EvidenceBuildConfig, EvidenceBuilder,
    EvidencePolicy, Profile,
    git::{check_shallow_clone, git_ls_files, is_dirty_or_unknown},
    load_trace_roots, parse_cargo_test_output_detailed, sign_bundle,
    trace::{
        TraceFiles, generate_traceability_matrix, read_all_trace_files,
        validate_trace_links_with_policy,
    },
};

use super::{fail, split_trace_roots_flag};
use crate::cli::output::emit_json;

/// Per-crate derivations extracted from `boundary.toml`, needed by
/// phases downstream of config construction.
pub(super) struct BoundaryDerived {
    pub(super) in_scope_crates: Vec<String>,
    pub(super) trace_roots: Vec<String>,
    pub(super) dal_map: BTreeMap<String, Dal>,
    pub(super) max_dal: Dal,
    /// Raw policy flags, carried so the policy-implementability
    /// check can fire before the builder is constructed.
    pub(super) policy: BoundaryPolicy,
}

// ============================================================================
// Phase 1 — preflight checks (shallow-clone, cert-dirty)
// ============================================================================

/// Run the two policy gates that block bundle generation before any
/// real work begins. On a gate failure, emit the JSON/text error
/// envelope and return `Ok(Some(EXIT_ERROR))` so the caller can
/// short-circuit; on success return `Ok(None)`. Any other bail
/// (unexpected I/O, tooling error) is propagated as `Err`.
pub(super) fn preflight(profile: Profile, json_output: bool) -> Result<Option<i32>> {
    if let Err(e) = check_shallow_clone() {
        return fail(json_output, profile, e.to_string()).map(Some);
    }
    if matches!(profile, Profile::Cert | Profile::Record) && is_dirty_or_unknown() {
        return fail(
            json_output,
            profile,
            format!(
                "profile '{}' requires clean git tree. Commit or stash changes first.",
                profile
            ),
        )
        .map(Some);
    }
    Ok(None)
}

// ============================================================================
// Phase 2 — boundary config + build config
// ============================================================================

/// Load `boundary.toml` (default on absent/malformed — matches old
/// hand-rolled CLI behavior), merge the `--trace-roots` flag, and
/// produce both the [`EvidenceBuildConfig`] the builder needs and a
/// [`BoundaryDerived`] snapshot the remaining phases consume.
pub(super) fn build_config(
    profile: Profile,
    output_root: PathBuf,
    boundary_path: &Path,
    trace_roots_arg: Option<String>,
) -> (EvidenceBuildConfig, BoundaryDerived) {
    let trace_roots = trace_roots_arg
        .as_deref()
        .map(split_trace_roots_flag)
        .unwrap_or_else(|| load_trace_roots(boundary_path));
    let boundary_config = BoundaryConfig::load_or_default(boundary_path);
    let in_scope_crates = boundary_config.scope.in_scope.clone();
    let dal_map = boundary_config.dal_map();
    let max_dal = dal_map.values().copied().max().unwrap_or_default();
    let policy = boundary_config.policy.clone();
    let strict = matches!(profile, Profile::Cert | Profile::Record);
    let config = EvidenceBuildConfig {
        output_root,
        profile,
        in_scope_crates: in_scope_crates.clone(),
        trace_roots: trace_roots.clone(),
        require_clean_git: strict,
        fail_on_dirty: strict,
        dal_map: dal_map.clone(),
    };
    (
        config,
        BoundaryDerived {
            in_scope_crates,
            trace_roots,
            dal_map,
            max_dal,
            policy,
        },
    )
}

// Phase 2.5 / 2a — boundary policy gates (implementability + real
// enforcement) live in the sibling `policy` module and are reached
// via `phases::enforce_boundary_policy` from the orchestrator.

// ============================================================================
// Phase 2b — initialize the builder (wraps error in the failure envelope)
// ============================================================================

/// Construct an [`EvidenceBuilder`] from the caller's config; on
/// builder-setup failure, emit the standard JSON/text envelope via
/// [`fail`] and surface the exit code for early-return. Also prints
/// the "generating bundle in …" / "profile = …" progress banner.
pub(super) fn init_builder(
    config: EvidenceBuildConfig,
    profile: Profile,
    quiet: bool,
    json_output: bool,
) -> Result<Result<EvidenceBuilder, i32>> {
    let builder = match EvidenceBuilder::new(config) {
        Ok(b) => b,
        Err(e) => return fail(json_output, profile, e.to_string()).map(Err),
    };
    if !quiet && !json_output {
        println!("evidence: generating bundle in {:?}", builder.bundle_dir());
        println!("evidence: profile = {}", profile);
    }
    // Pre-release tool → cert/record early warning (SYS-017).
    // The eventual `verify --profile cert` will fail with
    // `VERIFY_PRERELEASE_TOOL`; emit the warning now so the user
    // doesn't learn about it only after the full generate pipeline
    // runs. Dev profile: silent (dev iteration stays fast).
    if evidence_core::env::TOOL_IS_PRERELEASE && matches!(profile, Profile::Cert | Profile::Record)
    {
        tracing::warn!(
            "tool_prerelease = true on profile {}: the bundle this run \
             produces will fail `verify --profile {}` with \
             VERIFY_PRERELEASE_TOOL. Install a release build to produce \
             audit-valid cert evidence.",
            profile,
            profile
        );
    }
    Ok(Ok(builder))
}

// ============================================================================
// Phase 3 — capture env fingerprint
// ============================================================================

/// Capture the current host's `EnvFingerprint` (strict mode for
/// cert/record) and write `env.json` into the bundle dir.
pub(super) fn capture_and_write_env(
    builder: &EvidenceBuilder,
    profile: Profile,
) -> Result<EnvFingerprint> {
    let strict = matches!(profile, Profile::Cert | Profile::Record);
    let env_fp = EnvFingerprint::capture(profile, strict)?;
    let env_path = builder.bundle_dir().join("env.json");
    fs::write(&env_path, serde_json::to_vec_pretty(&env_fp)?)?;
    Ok(env_fp)
}

// ============================================================================
// Phase 4 — hash in-scope source files
// ============================================================================

/// Run `git ls-files` over the in-scope crate prefixes and hash each
/// returned file into the bundle's `inputs_hashes.json`. Strict
/// (cert/record) mode bails on any failure; non-strict mode logs a
/// `warning:` line and continues.
pub(super) fn hash_in_scope_sources(
    builder: &mut EvidenceBuilder,
    prefixes: &[String],
    strict: bool,
    quiet: bool,
    json_output: bool,
) -> Result<()> {
    if prefixes.is_empty() {
        return Ok(());
    }
    let refs: Vec<&str> = prefixes.iter().map(|s| s.as_str()).collect();
    match git_ls_files(&refs) {
        Ok(files) => {
            for f in &files {
                if let Err(e) = builder.hash_input(f) {
                    if strict {
                        return Err(
                            anyhow::Error::new(e).context(format!("hashing source file: {}", f))
                        );
                    }
                    eprintln!("warning: could not hash {}: {}", f, e);
                }
            }
            if !quiet && !json_output {
                println!("evidence: hashed {} source file(s)", files.len());
            }
        }
        Err(e) => {
            if strict {
                return Err(anyhow::Error::new(e).context("listing in-scope source files"));
            }
            eprintln!("warning: could not list source files: {}", e);
        }
    }
    Ok(())
}

// ============================================================================
// Phase 5 — run cargo test and capture
// ============================================================================

/// Run `cargo test --workspace` through the builder's `run_capture`,
/// parse the stdout summary, and record it on the builder. `skip_tests`
/// short-circuits. In strict mode any failure to *run* cargo test
/// bails (so cert bundles never silently omit test evidence); in dev
/// mode a failure degrades to a warning.
pub(super) fn run_tests_and_capture(
    builder: &mut EvidenceBuilder,
    skip_tests: bool,
    strict: bool,
    quiet: bool,
    json_output: bool,
) -> Result<()> {
    if skip_tests {
        return Ok(());
    }
    let mut test_cmd = std::process::Command::new("cargo");
    test_cmd.args(["test", "--workspace"]);
    match builder.run_capture(test_cmd, "tests", "cargo_test", "cargo test --workspace") {
        Ok((stdout, _stderr)) => {
            let stdout_str = String::from_utf8_lossy(&stdout);
            // The detailed parser enriches TestSummary with
            // per-test records + captured failure-message blocks.
            // `None` means skipped tests / empty workspace.
            if let Some((summary, outcomes, _errors)) =
                parse_cargo_test_output_detailed(&stdout_str)
            {
                if !quiet && !json_output {
                    println!(
                        "evidence: tests: {} passed, {} failed, {} ignored",
                        summary.passed, summary.failed, summary.ignored
                    );
                }
                builder.set_test_summary(summary);
                if !outcomes.is_empty() {
                    builder.set_test_outcomes(outcomes);
                    builder
                        .write_test_outcomes()
                        .context("writing tests/test_outcomes.jsonl")?;
                }
            }
        }
        Err(e) => {
            // run_capture returns Err only on subprocess spawn
            // failure; non-zero exit goes through the Ok arm
            // and is recorded inside run_capture. Record spawn
            // failures here so verify sees the bundle as
            // incomplete either way.
            builder.record_command_failure(evidence_core::ToolCommandFailure {
                command_name: "cargo test --workspace".to_string(),
                exit_code: -1,
                stderr_tail: e.to_string(),
            });
            if strict {
                return Err(anyhow::Error::new(e).context("running cargo test"));
            }
            tracing::warn!("cargo test could not be spawned: {}", e);
        }
    }
    Ok(())
}

// ============================================================================
// Phase 6 — validate trace links
// ============================================================================

/// Walk every configured `trace_roots` entry and run
/// `validate_trace_links_with_policy`. In strict mode the first
/// validation *failure* emits a JSON failure envelope and returns
/// `Ok(Some(EXIT_ERROR))`; missing-directory warnings never bail.
pub(super) fn validate_trace_links_phase(
    trace_roots: &[String],
    policy: &EvidencePolicy,
    profile: Profile,
    strict: bool,
    quiet: bool,
    json_output: bool,
) -> Result<Option<i32>> {
    for root in trace_roots {
        let root_path = Path::new(root);
        if !root_path.exists() {
            if !quiet && !json_output {
                eprintln!(
                    "warning: trace root '{}' does not exist, skipping validation",
                    root
                );
            }
            continue;
        }
        match read_all_trace_files(root) {
            Ok(TraceFiles {
                sys,
                hlr,
                llr,
                tests,
                ..
            }) => {
                if let Err(e) = validate_trace_links_with_policy(
                    &sys.requirements,
                    &hlr.requirements,
                    &llr.requirements,
                    &tests.tests,
                    &[],
                    &policy.trace,
                ) {
                    if strict {
                        return fail(
                            json_output,
                            profile,
                            format!("Trace validation failed in '{}': {}", root, e),
                        )
                        .map(Some);
                    }
                    eprintln!("warning: trace validation failed in '{}': {}", root, e);
                } else if !quiet && !json_output {
                    println!("evidence: trace links valid in '{}'", root);
                }
            }
            Err(e) => {
                if strict {
                    return Err(anyhow::Error::new(e)
                        .context(format!("reading trace files from '{}'", root)));
                }
                eprintln!("warning: could not read trace files from '{}': {}", root, e);
            }
        }
    }
    Ok(None)
}

// ============================================================================
// Phase 7 — copy trace sources + emit matrix
// ============================================================================

/// Copy `{hlr,llr,tests,derived}.toml` from each trace root into the
/// bundle's `trace/` directory and write the generated `matrix.md`
/// alongside. Returns the matrix paths so they can be registered as
/// bundle `trace_outputs` at finalize time.
pub(super) fn copy_trace_and_build_matrix(
    builder: &EvidenceBuilder,
    trace_roots: &[String],
    quiet: bool,
    json_output: bool,
) -> Result<Vec<PathBuf>> {
    let mut trace_outputs: Vec<PathBuf> = Vec::new();
    for root in trace_roots {
        let root_path = Path::new(root);
        if !root_path.exists() {
            continue;
        }
        if let Ok(trace_files) = read_all_trace_files(root) {
            let bundle_trace_dir = builder.bundle_dir().join("trace");
            for filename in &["hlr.toml", "llr.toml", "tests.toml", "derived.toml"] {
                let src = root_path.join(filename);
                if src.exists() {
                    fs::copy(&src, bundle_trace_dir.join(filename))?;
                }
            }
            let doc_id = &trace_files.hlr.meta.document_id;
            let matrix_md = generate_traceability_matrix(
                &trace_files.hlr,
                &trace_files.llr,
                &trace_files.tests,
                doc_id,
            );
            let matrix_path = bundle_trace_dir.join("matrix.md");
            fs::write(&matrix_path, matrix_md)?;
            trace_outputs.push(matrix_path);
            if !quiet && !json_output {
                println!("evidence: trace data copied from '{}'", root);
            }
        }
    }
    Ok(trace_outputs)
}

// ============================================================================
// Phase 8 — write per-crate compliance reports
// ============================================================================

/// Generate `compliance/<crate>.json` for each crate in `dal_map`.
/// Run before finalize so the files are included in `SHA256SUMS`.
/// `builder.tests_passed()` is the authoritative verdict (reads the
/// recorded TestSummary's `failed == 0`).
pub(super) fn write_compliance_reports(
    builder: &EvidenceBuilder,
    dal_map: &BTreeMap<String, Dal>,
    trace_roots: &[String],
    quiet: bool,
    json_output: bool,
) -> Result<()> {
    if dal_map.is_empty() {
        return Ok(());
    }
    let compliance_dir = builder.bundle_dir().join("compliance");
    fs::create_dir_all(&compliance_dir)?;
    let tests_passed = builder.tests_passed();
    let has_test_results = tests_passed.is_some();
    let has_per_test_outcomes = builder.has_test_outcomes();
    let has_trace_data = trace_roots.iter().any(|r| Path::new(r).exists());
    for (crate_name, dal) in dal_map {
        let crate_evidence = evidence_core::CrateEvidence {
            has_trace_data,
            trace_validation_passed: true,
            has_test_results,
            tests_passed,
            has_coverage_data: false,
            has_per_test_outcomes,
        };
        let report = evidence_core::generate_compliance_report(crate_name, *dal, &crate_evidence);
        let report_path = compliance_dir.join(format!("{}.json", crate_name));
        fs::write(&report_path, serde_json::to_string_pretty(&report)?)?;
        if !quiet && !json_output {
            println!(
                "evidence: compliance report for '{}' (DAL-{}): {}/{} objectives met",
                crate_name, dal, report.summary.met, report.summary.applicable
            );
        }
    }
    Ok(())
}

// ============================================================================
// Phase 9 — finalize bundle + optional HMAC signing
// ============================================================================

/// Finalize the bundle (writes `SHA256SUMS`, `index.json`, closes the
/// builder) and, if `sign_key` is set, sign the envelope and drop
/// `BUNDLE.sig` next to it. Returns the bundle directory path.
pub(super) fn finalize_and_sign(
    builder: EvidenceBuilder,
    trace_outputs: Vec<PathBuf>,
    sign_key: Option<PathBuf>,
    quiet: bool,
    json_output: bool,
) -> Result<PathBuf> {
    let bundle_path = builder.finalize(trace_outputs)?;
    if let Some(key_path) = sign_key {
        let key_bytes = fs::read(&key_path)
            .with_context(|| format!("reading signing key from {:?}", key_path))?;
        sign_bundle(&bundle_path, &key_bytes)?;
        if !quiet && !json_output {
            println!("evidence: HMAC signature written to BUNDLE.sig");
        }
    }
    Ok(bundle_path)
}

// ============================================================================
// Phase 10 — emit the success envelope
// ============================================================================

/// Emit the success envelope — JSON (one document, stdout) or a
/// `bundle created at …` line. `recorded_failures` drives the
/// `success` field: `success == 0` ⇔ `bundle_complete == true`
/// ⇔ the envelope's `success: true`. See
/// [`super::envelope::build_success_envelope`] for the shape.
pub(super) fn emit_success_envelope(
    json_output: bool,
    quiet: bool,
    bundle_path: &Path,
    profile: Profile,
    env_fp: &EnvFingerprint,
    recorded_failures: usize,
) -> Result<()> {
    if json_output {
        let out = super::envelope::build_success_envelope(
            bundle_path,
            profile,
            env_fp,
            recorded_failures,
        );
        emit_json(&out)?;
    } else if !quiet {
        println!("evidence: bundle created at {:?}", bundle_path);
    }
    Ok(())
}
