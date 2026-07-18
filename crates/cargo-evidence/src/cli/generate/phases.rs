//! Phase functions for `cargo evidence generate`. Short-circuiting
//! phases return `Result<Option<i32>>`; I/O-only phases return
//! `Result<()>`. Visibility is `pub(super)`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use evidence_core::{
    BoundaryConfig, BoundaryPolicy, Dal, EnvFingerprint, EvidenceBuildConfig, EvidenceBuilder,
    Profile, build_input_plan_blocking,
    git::{check_shallow_clone, is_dirty_or_unknown},
    load_trace_roots, parse_nextest_libtest_json,
    trace::{generate_traceability_matrix, read_all_trace_files},
};

use super::{fail, split_trace_roots_flag};
use crate::cli::output::emit_json;

/// Per-crate derivations extracted from `boundary.toml`, needed by
/// phases downstream of config construction.
pub(super) struct BoundaryDerived {
    pub(super) in_scope_crates: Vec<String>,
    /// Workspace-relative paths of controlled inputs declared required
    /// in `boundary.toml`'s `[inputs]` section — hashed even when not
    /// git-tracked (generated code), failing closed if absent.
    pub(super) required_inputs: Vec<String>,
    pub(super) trace_roots: Vec<String>,
    pub(super) dal_map: BTreeMap<String, Dal>,
    pub(super) max_dal: Dal,
    /// Raw policy flags, carried so the policy-implementability
    /// check can fire before the builder is constructed.
    pub(super) policy: BoundaryPolicy,
    /// Auxiliary MC/DC tool reference, propagated from
    /// `boundary.toml`'s `[dal]` section so the DAL-A
    /// qualification gate can read it without re-loading the
    /// config. `None` ⇒ no external MC/DC evidence claimed.
    pub(super) auxiliary_mcdc_tool: Option<evidence_core::AuxiliaryMcdcTool>,
}

// Phase 1 — preflight checks (shallow-clone, cert-dirty)

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

// Phase 2 — boundary config + build config

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
    let required_inputs = boundary_config.inputs.required.clone();
    let dal_map = boundary_config.dal_map();
    let max_dal = dal_map.values().copied().max().unwrap_or_default();
    let policy = boundary_config.policy.clone();
    let auxiliary_mcdc_tool = boundary_config.dal.auxiliary_mcdc_tool.clone();
    let strict = matches!(profile, Profile::Cert | Profile::Record);
    let config = EvidenceBuildConfig {
        output_root,
        profile,
        in_scope_crates: in_scope_crates.clone(),
        trace_roots: trace_roots.clone(),
        require_clean_git: strict,
        fail_on_dirty: strict,
        dal_map: dal_map.clone(),
        boundary_policy: policy.clone(),
    };
    (
        config,
        BoundaryDerived {
            in_scope_crates,
            required_inputs,
            trace_roots,
            dal_map,
            max_dal,
            policy,
            auxiliary_mcdc_tool,
        },
    )
}

// Phase 2.5 / 2a — boundary policy gates (implementability + real
// enforcement) live in the sibling `policy` module and are reached
// via `phases::enforce_boundary_policy` from the orchestrator.

// Phase 2b — initialize the builder (wraps error in the failure envelope)

/// Construct an [`EvidenceBuilder`]; builder-setup failure emits
/// the JSON/text failure envelope via [`fail`] and surfaces the
/// exit code for early-return. Emits cert/record tool-provenance
/// warnings (prerelease + release-source-engine) before returning.
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
    // Cert/record early warnings — surface tool-provenance
    // weaknesses before the full pipeline runs. Dev profile
    // stays silent for fast iteration.
    if matches!(profile, Profile::Cert | Profile::Record) {
        if evidence_core::env::TOOL_IS_PRERELEASE {
            tracing::warn!(
                "tool_prerelease = true on profile {}: `verify --profile {}` \
                 will fail with VERIFY_PRERELEASE_TOOL. Install a release build \
                 for audit-valid cert evidence.",
                profile,
                profile
            );
        }
        if evidence_core::env::TOOL_BUILD_SOURCE_IS_RELEASE {
            tracing::warn!(
                code = "ENV_ENGINE_RELEASE_PROVENANCE",
                "engine_build_source=release on profile {}: engine_git_sha is a \
                 `release-v<version>` fallback. For cert-grade evidence install \
                 with `cargo install --git https://github.com/luofang34/Evidence cargo-evidence`.",
                profile,
            );
        }
    }
    Ok(Ok(builder))
}

// Phase 3 — capture env fingerprint

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

// Phase 4 — hash in-scope source files

/// Resolve each in-scope Cargo package name to its manifest directory,
/// enumerate that directory plus the workspace-control inputs via
/// `git ls-files`, and hash every resolved file into the bundle's
/// `inputs_hashes.json`. Package identity is resolved through
/// `cargo metadata` — never treated as a repository path.
///
/// Strict (cert/record) mode fails closed on any unresolved package,
/// path escape, empty in-scope unit, or zero-input total, so a cert
/// bundle can never record an empty source baseline. Non-strict mode
/// degrades to a `warning:` line and continues.
pub(super) fn hash_in_scope_sources(
    builder: &mut EvidenceBuilder,
    prefixes: &[String],
    required_inputs: &[String],
    strict: bool,
    quiet: bool,
    json_output: bool,
) -> Result<()> {
    if prefixes.is_empty() && required_inputs.is_empty() {
        return Ok(());
    }
    match build_input_plan_blocking(prefixes, required_inputs) {
        Ok(plan) => {
            for entry in &plan {
                if let Err(e) = builder.hash_input(&entry.path) {
                    if strict {
                        return Err(anyhow::Error::new(e)
                            .context(format!("hashing source file: {}", entry.path)));
                    }
                    tracing::warn!("could not hash {}: {}", entry.path, e);
                }
            }
            if !quiet && !json_output {
                println!("evidence: hashed {} source input(s)", plan.len());
            }
        }
        Err(e) => {
            if strict {
                return Err(anyhow::Error::new(e).context("resolving in-scope source inputs"));
            }
            tracing::warn!("could not resolve source inputs: {}", e);
        }
    }
    Ok(())
}

// Phase 5 — run nextest and capture

/// Run `cargo nextest run --workspace` under `libtest-json-plus`
/// through the builder's `run_capture`, parse the machine-readable
/// event stream, and record the per-test outcomes + summary on the
/// builder. Machine-readable output preserves per-binary identity, so
/// LLR `test_selector`s resolve to executed results — the identity that
/// plain libtest text loses as `__unknown_binary__`.
///
/// `skip_tests` short-circuits. In strict mode any failure to *run*
/// nextest bails (so cert bundles never silently omit test evidence);
/// in dev mode a spawn failure degrades to a warning.
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
    test_cmd.args([
        "nextest",
        "run",
        "--workspace",
        // Record every test's outcome even when some fail — evidence
        // generation must not stop at the first failure (nextest's
        // default), or the bundle would omit results for the rest.
        "--no-fail-fast",
        "--message-format",
        "libtest-json-plus",
    ]);
    // The libtest-json format is gated behind this env in current
    // nextest; NO_COLOR keeps the JSON stream free of ANSI escapes.
    test_cmd.env("NEXTEST_EXPERIMENTAL_LIBTEST_JSON", "1");
    test_cmd.env("NO_COLOR", "1");
    match builder.run_capture(
        test_cmd,
        "tests",
        "cargo_test",
        "cargo nextest run --workspace",
    ) {
        Ok((stdout, _stderr)) => {
            let stdout_str = String::from_utf8_lossy(&stdout);
            let run = parse_nextest_libtest_json(&stdout_str);
            // The suite-level summary and the per-test records are two
            // independent tallies of the same stream; if they disagree
            // the capture dropped an event. Fail closed in strict mode
            // (a cert bundle must not ship inconsistent test evidence);
            // warn on dev.
            let discrepancies = run.reconcile();
            if !discrepancies.is_empty() {
                let detail = discrepancies
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                if strict {
                    return Err(anyhow::anyhow!(
                        "nextest summary does not reconcile with per-test records \
                         ({detail}); captured test evidence is inconsistent"
                    ));
                }
                tracing::warn!("nextest summary/record reconciliation mismatch: {detail}");
            }
            if !quiet && !json_output {
                println!(
                    "evidence: tests: {} passed, {} failed, {} ignored",
                    run.summary.passed, run.summary.failed, run.summary.ignored
                );
            }
            builder.set_test_summary(run.summary);
            if !run.records.is_empty() {
                // Write is deferred to `enrich_and_write_test_outcomes`,
                // which runs after the trace phase loads LLR data and
                // populates the per-test → LLR back-links.
                builder.set_test_outcomes(run.records);
            }
        }
        Err(e) => {
            // run_capture returns Err only on subprocess spawn
            // failure; non-zero exit goes through the Ok arm and is
            // recorded inside run_capture. Record spawn failures here
            // so verify sees the bundle as incomplete either way.
            builder.record_command_failure(evidence_core::ToolCommandFailure {
                command_name: "cargo nextest run --workspace".to_string(),
                exit_code: -1,
                stderr_tail: e.to_string(),
            });
            if strict {
                return Err(anyhow::Error::new(e).context("running cargo nextest"));
            }
            tracing::warn!("cargo nextest could not be spawned: {}", e);
        }
    }
    Ok(())
}

// Phase 5b lives in the sibling `phases/output_inventory.rs` module,
// re-exported below as `inventory_and_hash_outputs`.
#[path = "phases/output_inventory.rs"]
mod output_inventory;
pub(super) use output_inventory::inventory_and_hash_outputs;

// Phase 6 lives in sibling `phases/trace_validation.rs` via
// `#[path]` so this file stays under the 500-line limit.
// Re-exported below as `validate_trace_links_phase` +
// `TraceValidationResult`.
#[path = "phases/trace_validation.rs"]
mod trace_validation;
pub(super) use trace_validation::validate_trace_links_phase;

// Phase 6b — enrich test outcomes with LLR back-links + write.
// See sibling `test_outcomes.rs`.

// Phase 7 — copy trace sources + emit matrix

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

// Phase 8 — write per-crate compliance reports

/// Generate `compliance/<crate>.json` for each crate in `dal_map`.
/// Run before finalize so the files are included in `SHA256SUMS`.
/// `builder.tests_passed()` is the authoritative verdict (reads the
/// recorded TestSummary's `failed == 0`).
pub(super) fn write_compliance_reports(
    builder: &EvidenceBuilder,
    dal_map: &BTreeMap<String, Dal>,
    trace_roots: &[String],
    trace_validation_passed: bool,
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
    let coverage_statement_percent = builder.coverage_statement_percent();
    let coverage_branch_percent = builder.coverage_branch_percent();
    let has_coverage_data =
        coverage_statement_percent.is_some() || coverage_branch_percent.is_some();
    let has_trace_data = trace_roots.iter().any(|r| Path::new(r).exists());
    for (crate_name, dal) in dal_map {
        let crate_evidence = evidence_core::CrateEvidence {
            has_trace_data,
            trace_validation_passed,
            has_test_results,
            tests_passed,
            has_coverage_data,
            has_per_test_outcomes,
            coverage_statement_percent,
            coverage_branch_percent,
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

// Phase 9 — finalize bundle + optional ed25519 signing
// (key resolution + anchor consistency live in the sibling
// `finalize.rs` to keep this file under the workspace size cap).

pub(super) use super::finalize::finalize_and_sign;

// Phase 10 — emit the success envelope

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
