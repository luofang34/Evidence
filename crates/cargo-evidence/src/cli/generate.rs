//! `cargo evidence generate`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use evidence::{
    BoundaryConfig, EnvFingerprint, EvidenceBuildConfig, EvidenceBuilder, EvidencePolicy, Profile,
    git::{check_shallow_clone, git_ls_files, is_dirty_or_unknown},
    load_trace_roots, parse_cargo_test_output, sign_bundle,
    trace::{
        TraceFiles, generate_traceability_matrix, read_all_trace_files,
        validate_trace_links_with_policy,
    },
};

use super::args::{EXIT_ERROR, EXIT_SUCCESS, detect_profile};
use super::output::emit_json;

#[derive(Serialize)]
struct GenerateOutput {
    success: bool,
    bundle_path: Option<String>,
    profile: String,
    git_sha: Option<String>,
    error: Option<String>,
}

/// Emit a failure envelope and return EXIT_ERROR.
///
/// Collapses the `if json { emit_json(GenerateOutput{error: …}) } else
/// { eprintln!("error: {msg}") }` pattern that repeated five times in
/// cmd_generate's preflight branches. Every error path now shapes
/// the JSON envelope identically instead of open-coding it.
fn fail(json_output: bool, profile: Profile, msg: impl Into<String>) -> Result<i32> {
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

/// Arguments for the generate command, grouped to avoid clippy::too_many_arguments.
pub struct GenerateArgs {
    pub profile_arg: Option<String>,
    pub out_dir: Option<PathBuf>,
    pub write_workspace: bool,
    pub boundary: Option<PathBuf>,
    pub trace_roots_arg: Option<String>,
    pub sign_key: Option<PathBuf>,
    pub skip_tests: bool,
    pub quiet: bool,
    pub json_output: bool,
}

pub fn cmd_generate(args: GenerateArgs) -> Result<i32> {
    let GenerateArgs {
        profile_arg,
        out_dir,
        write_workspace,
        boundary,
        trace_roots_arg,
        sign_key,
        skip_tests,
        quiet,
        json_output,
    } = args;
    // Determine profile
    let profile = match &profile_arg {
        Some(p) => p.parse::<Profile>()?,
        None => detect_profile(),
    };

    // Check shallow clone
    if let Err(e) = check_shallow_clone() {
        return fail(json_output, profile, e.to_string());
    }

    // In Cert/Record profile: error if git is dirty
    if matches!(profile, Profile::Cert | Profile::Record) && is_dirty_or_unknown() {
        return fail(
            json_output,
            profile,
            format!(
                "profile '{}' requires clean git tree. Commit or stash changes first.",
                profile
            ),
        );
    }

    // Determine output directory
    let output_root = if let Some(dir) = out_dir {
        dir
    } else if write_workspace {
        PathBuf::from("evidence")
    } else {
        return fail(
            json_output,
            profile,
            "--out-dir is required unless --write-workspace is specified",
        );
    };

    // Resolve boundary config path
    let boundary_path = boundary
        .clone()
        .unwrap_or_else(|| PathBuf::from("cert/boundary.toml"));

    // Parse trace roots (CLI flag > boundary.toml > hardcoded default)
    let trace_roots: Vec<String> = trace_roots_arg
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
        .unwrap_or_else(|| load_trace_roots(&boundary_path));

    // Load boundary + DAL config. A missing/malformed file yields
    // a default-populated BoundaryConfig (empty scope, DAL-D),
    // matching the behavior of the old hand-rolled CLI loaders.
    let boundary_config = BoundaryConfig::load_or_default(&boundary_path);
    let in_scope_crates = boundary_config.scope.in_scope.clone();
    let dal_map = boundary_config.dal_map();
    // Derive max DAL before dal_map is moved into config
    let max_dal = dal_map.values().copied().max().unwrap_or_default();

    // Build config (clone fields we need later)
    let source_prefixes = in_scope_crates.clone();
    let trace_root_list = trace_roots.clone();
    let dal_map_for_compliance = dal_map.clone();
    let config = EvidenceBuildConfig {
        output_root: output_root.clone(),
        profile,
        in_scope_crates,
        trace_roots,
        require_clean_git: matches!(profile, Profile::Cert | Profile::Record),
        fail_on_dirty: matches!(profile, Profile::Cert | Profile::Record),
        dal_map,
    };

    // Create builder
    let mut builder = match EvidenceBuilder::new(config) {
        Ok(b) => b,
        Err(e) => return fail(json_output, profile, e.to_string()),
    };

    if !quiet && !json_output {
        println!("evidence: generating bundle in {:?}", builder.bundle_dir());
        println!("evidence: profile = {}", profile);
    }

    // Write environment fingerprint (strict mode for cert/record profiles)
    let strict = matches!(profile, Profile::Cert | Profile::Record);
    let env_fp = EnvFingerprint::capture(&profile.to_string(), strict)?;
    let env_path = builder.bundle_dir().join("env.json");
    fs::write(&env_path, serde_json::to_vec_pretty(&env_fp)?)?;

    // Hash source files from in-scope crates
    if !source_prefixes.is_empty() {
        let prefixes: Vec<&str> = source_prefixes.iter().map(|s| s.as_str()).collect();
        match git_ls_files(&prefixes) {
            Ok(files) => {
                for f in &files {
                    if let Err(e) = builder.hash_input(f) {
                        if strict {
                            return Err(anyhow::Error::new(e)
                                .context(format!("hashing source file: {}", f)));
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
    }

    // Write inputs
    builder.write_inputs()?;

    // Write outputs (populated by command captures)
    builder.write_outputs()?;

    // Write commands (populated by run_capture calls)
    builder.write_commands()?;

    // Run cargo test and capture results (unless --skip-tests)
    if !skip_tests {
        let mut test_cmd = std::process::Command::new("cargo");
        test_cmd.args(["test", "--workspace"]);
        match builder.run_capture(test_cmd, "tests", "cargo_test", "cargo test --workspace") {
            Ok((stdout, _stderr)) => {
                let stdout_str = String::from_utf8_lossy(&stdout);
                if let Some(summary) = parse_cargo_test_output(&stdout_str) {
                    if !quiet && !json_output {
                        println!(
                            "evidence: tests: {} passed, {} failed, {} ignored",
                            summary.passed, summary.failed, summary.ignored
                        );
                    }
                    builder.set_test_summary(summary);
                }
            }
            Err(e) => {
                if strict {
                    return Err(anyhow::Error::new(e).context("running cargo test"));
                }
                eprintln!("warning: cargo test failed: {}", e);
            }
        }
    }

    // Re-write outputs and commands after test capture added new files
    builder.write_outputs()?;
    builder.write_commands()?;

    // Validate trace links before finalize
    // Derive trace policy from DAL (use highest DAL across all in-scope crates, or default)
    let evidence_policy = EvidencePolicy::for_dal(max_dal);
    for root in &trace_root_list {
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
                hlr, llr, tests, ..
            }) => {
                if let Err(e) = validate_trace_links_with_policy(
                    &hlr.requirements,
                    &llr.requirements,
                    &tests.tests,
                    &[], // derived entries (TODO: wire from derived.toml)
                    &evidence_policy.trace,
                ) {
                    if strict {
                        return fail(
                            json_output,
                            profile,
                            format!("Trace validation failed in '{}': {}", root, e),
                        );
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

    // Copy trace data into bundle and generate traceability matrix
    let mut trace_outputs: Vec<PathBuf> = Vec::new();
    for root in &trace_root_list {
        let root_path = Path::new(root);
        if !root_path.exists() {
            continue;
        }
        if let Ok(trace_files) = read_all_trace_files(root) {
            let bundle_trace_dir = builder.bundle_dir().join("trace");
            // Copy source TOML files into bundle
            for filename in &["hlr.toml", "llr.toml", "tests.toml", "derived.toml"] {
                let src = root_path.join(filename);
                if src.exists() {
                    fs::copy(&src, bundle_trace_dir.join(filename))?;
                }
            }
            // Generate traceability matrix (infallible: deterministic string concat)
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

    // Generate per-crate compliance reports (before finalize so they're included in SHA256SUMS)
    if !dal_map_for_compliance.is_empty() {
        let compliance_dir = builder.bundle_dir().join("compliance");
        fs::create_dir_all(&compliance_dir)?;

        // `builder.tests_passed()` is the authoritative verdict —
        // it reads the recorded TestSummary's `failed == 0`. Using
        // `!skip_tests` here was a placeholder that conflated "the
        // user didn't pass --skip-tests" with "the tests passed",
        // which is exactly backwards: a run that actually attempted
        // tests and saw failures would still register as passed in
        // the compliance report.
        let tests_passed = builder.tests_passed();
        let has_test_results = tests_passed.is_some();

        for (crate_name, dal) in &dal_map_for_compliance {
            let crate_evidence = evidence::CrateEvidence {
                has_trace_data: trace_root_list.iter().any(|r| Path::new(r).exists()),
                trace_validation_passed: true,
                has_test_results,
                tests_passed,
                has_coverage_data: false,
            };
            let report = evidence::generate_compliance_report(crate_name, *dal, &crate_evidence);
            let report_path = compliance_dir.join(format!("{}.json", crate_name));
            fs::write(&report_path, serde_json::to_string_pretty(&report)?)?;
            if !quiet && !json_output {
                println!(
                    "evidence: compliance report for '{}' (DAL-{}): {}/{} objectives met",
                    crate_name, dal, report.summary.met, report.summary.applicable
                );
            }
        }
    }

    // Finalize bundle. Schema versions flow from
    // `evidence::schema_versions` inside `finalize` — no magic strings
    // at the call site.
    let bundle_path = builder.finalize(trace_outputs)?;

    // HMAC signing if key provided
    if let Some(key_path) = sign_key {
        let key_bytes = fs::read(&key_path)
            .with_context(|| format!("reading signing key from {:?}", key_path))?;
        sign_bundle(&bundle_path, &key_bytes)?;
        if !quiet && !json_output {
            println!("evidence: HMAC signature written to BUNDLE.sig");
        }
    }

    if json_output {
        emit_json(&GenerateOutput {
            success: true,
            bundle_path: Some(bundle_path.display().to_string()),
            profile: profile.to_string(),
            git_sha: Some(env_fp.git_sha),
            error: None,
        })?;
    } else if !quiet {
        println!("evidence: bundle created at {:?}", bundle_path);
    }

    Ok(EXIT_SUCCESS)
}
