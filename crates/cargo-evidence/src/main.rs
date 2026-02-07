//! cargo-evidence - Cargo subcommand for build evidence and reproducibility verification
//!
//! This tool provides commands for generating, verifying, and managing
//! build evidence bundles.

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use evidence::{
    backfill_uuids, env::in_nix_shell, git::git_ls_files, sign_bundle,
    trace::{read_all_trace_files, validate_trace_links, TraceFiles},
    verify_bundle_with_key, EnvFingerprint, EvidenceBuildConfig, EvidenceBuilder, EvidenceIndex,
    Profile, VerifyResult,
};

// ============================================================================
// Embedded Schemas
// ============================================================================

const SCHEMA_INDEX: &str = include_str!("../../../schemas/index.schema.json");
const SCHEMA_ENV: &str = include_str!("../../../schemas/env.schema.json");
const SCHEMA_COMMANDS: &str = include_str!("../../../schemas/commands.schema.json");
const SCHEMA_HASHES: &str = include_str!("../../../schemas/hashes.schema.json");

// ============================================================================
// Exit Codes
// ============================================================================

const EXIT_SUCCESS: i32 = 0;
const EXIT_ERROR: i32 = 1;
const EXIT_VERIFICATION_FAILURE: i32 = 2;

// ============================================================================
// CLI Parsing
// ============================================================================

#[derive(Parser)]
#[command(name = "cargo")]
#[command(bin_name = "cargo")]
enum CargoCli {
    /// Build evidence and reproducibility verification
    Evidence(EvidenceArgs),
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct EvidenceArgs {
    #[command(subcommand)]
    command: Option<Commands>,

    // Default to generate if no subcommand given
    /// Build profile [dev, cert, record] (auto-detected if not specified)
    #[arg(long, global = true)]
    profile: Option<String>,

    /// Output directory for bundles (required unless --write-workspace)
    #[arg(long, global = true)]
    out_dir: Option<PathBuf>,

    /// Allow writing to workspace (dangerous, for xtask integration)
    #[arg(long, global = true)]
    write_workspace: bool,

    /// Path to boundary.toml
    #[arg(long, global = true)]
    boundary: Option<PathBuf>,

    /// Comma-separated list of trace root directories
    #[arg(long, global = true)]
    trace_roots: Option<String>,

    /// Suppress non-error output
    #[arg(long, short, global = true)]
    quiet: bool,

    /// Output results as JSON
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a new evidence bundle for the current build (default command)
    Generate {
        /// Build profile [dev, cert, record] (auto-detected if not specified)
        #[arg(long)]
        profile: Option<String>,

        /// Output directory for bundles (required unless --write-workspace)
        #[arg(long)]
        out_dir: Option<PathBuf>,

        /// Allow writing to workspace (dangerous, for xtask integration)
        #[arg(long)]
        write_workspace: bool,

        /// Path to boundary.toml
        #[arg(long)]
        boundary: Option<PathBuf>,

        /// Comma-separated list of trace root directories
        #[arg(long)]
        trace_roots: Option<String>,

        /// Path to HMAC signing key file (raw bytes)
        #[arg(long)]
        sign_key: Option<PathBuf>,

        /// Suppress non-error output
        #[arg(long, short)]
        quiet: bool,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Verify an evidence bundle
    Verify {
        /// Path to the evidence bundle directory
        bundle_path: PathBuf,

        /// Fail on any warning
        #[arg(long)]
        strict: bool,

        /// Path to HMAC verification key file (raw bytes)
        #[arg(long)]
        verify_key: Option<PathBuf>,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show differences between two evidence bundles
    Diff {
        /// First evidence bundle
        bundle_a: PathBuf,

        /// Second evidence bundle
        bundle_b: PathBuf,

        /// Output diff as JSON
        #[arg(long)]
        json: bool,
    },

    /// Initialize evidence tracking for a project
    Init {
        /// Overwrite existing files
        #[arg(long)]
        force: bool,
    },

    /// Manage and validate evidence schemas
    Schema {
        #[command(subcommand)]
        command: SchemaCommands,
    },

    /// Trace management utilities
    Trace {
        /// Validate trace links between HLR, LLR, and Tests
        #[arg(long)]
        validate: bool,

        /// Assign UUIDs to entries that are missing them
        #[arg(long)]
        backfill_uuids: bool,

        /// Comma-separated list of trace root directories
        #[arg(long)]
        trace_roots: Option<String>,
    },
}

#[derive(Subcommand)]
enum SchemaCommands {
    /// Print schema to stdout
    Show {
        /// Schema name (index, env, commands, hashes)
        schema: SchemaName,
    },

    /// Validate a JSON file against its schema
    Validate {
        /// Path to the JSON file to validate
        file: PathBuf,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum SchemaName {
    Index,
    Env,
    Commands,
    Hashes,
}

// ============================================================================
// Profile Detection
// ============================================================================

/// Three-tier auto-detection for build profile
fn detect_profile() -> Profile {
    if std::env::var("NAV_RECORD").is_ok() {
        Profile::Record
    } else if in_nix_shell() && is_ci() {
        Profile::Cert
    } else {
        Profile::Dev
    }
}

/// Check if running in CI environment
fn is_ci() -> bool {
    std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok()
}

/// Check for shallow clone
fn check_shallow_clone() -> Result<()> {
    if Path::new(".git/shallow").exists() {
        bail!(
            "Shallow clone detected. Evidence generation requires full repository history.\n\
             Run: git fetch --unshallow"
        );
    }
    Ok(())
}

/// Check if git is dirty
fn is_git_dirty() -> bool {
    use std::process::Command;
    Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

// ============================================================================
// Generate Command
// ============================================================================

#[derive(Serialize)]
struct GenerateOutput {
    success: bool,
    bundle_path: Option<String>,
    profile: String,
    git_sha: Option<String>,
    error: Option<String>,
}

/// Arguments for the generate command, grouped to avoid clippy::too_many_arguments.
struct GenerateArgs {
    profile_arg: Option<String>,
    out_dir: Option<PathBuf>,
    write_workspace: bool,
    boundary: Option<PathBuf>,
    trace_roots_arg: Option<String>,
    sign_key: Option<PathBuf>,
    quiet: bool,
    json_output: bool,
}

fn cmd_generate(args: GenerateArgs) -> Result<i32> {
    let GenerateArgs {
        profile_arg,
        out_dir,
        write_workspace,
        boundary,
        trace_roots_arg,
        sign_key,
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
        if json_output {
            let output = GenerateOutput {
                success: false,
                bundle_path: None,
                profile: profile.to_string(),
                git_sha: None,
                error: Some(e.to_string()),
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            eprintln!("error: {}", e);
        }
        return Ok(EXIT_ERROR);
    }

    // In Cert/Record profile: error if git is dirty
    if matches!(profile, Profile::Cert | Profile::Record) && is_git_dirty() {
        let err_msg = format!(
            "profile '{}' requires clean git tree. Commit or stash changes first.",
            profile
        );
        if json_output {
            let output = GenerateOutput {
                success: false,
                bundle_path: None,
                profile: profile.to_string(),
                git_sha: None,
                error: Some(err_msg.clone()),
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            eprintln!("error: {}", err_msg);
        }
        return Ok(EXIT_ERROR);
    }

    // Determine output directory
    let output_root = if let Some(dir) = out_dir {
        dir
    } else if write_workspace {
        PathBuf::from("evidence")
    } else {
        let err_msg = "--out-dir is required unless --write-workspace is specified";
        if json_output {
            let output = GenerateOutput {
                success: false,
                bundle_path: None,
                profile: profile.to_string(),
                git_sha: None,
                error: Some(err_msg.to_string()),
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            eprintln!("error: {}", err_msg);
        }
        return Ok(EXIT_ERROR);
    };

    // Resolve boundary config path
    let boundary_path = boundary
        .clone()
        .unwrap_or_else(|| PathBuf::from("cert/boundary.toml"));

    // Parse trace roots (CLI flag > boundary.toml > hardcoded default)
    let trace_roots: Vec<String> = trace_roots_arg
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
        .unwrap_or_else(|| load_trace_roots(&boundary_path));

    // Load boundary config if provided
    let in_scope_crates = if boundary_path.exists() {
        load_in_scope_crates(&boundary_path)?
    } else {
        Vec::new()
    };

    // Build config (clone fields we need later)
    let source_prefixes = in_scope_crates.clone();
    let trace_root_list = trace_roots.clone();
    let config = EvidenceBuildConfig {
        output_root: output_root.clone(),
        profile: profile.to_string(),
        in_scope_crates,
        trace_roots,
        skip_tests: false,
        require_clean_git: matches!(profile, Profile::Cert | Profile::Record),
        fail_on_dirty: matches!(profile, Profile::Cert | Profile::Record),
    };

    // Create builder
    let mut builder = match EvidenceBuilder::new(config) {
        Ok(b) => b,
        Err(e) => {
            if json_output {
                let output = GenerateOutput {
                    success: false,
                    bundle_path: None,
                    profile: profile.to_string(),
                    git_sha: None,
                    error: Some(e.to_string()),
                };
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                eprintln!("error: {}", e);
            }
            return Ok(EXIT_ERROR);
        }
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
                            return Err(e.context(format!("hashing source file: {}", f)));
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
                    return Err(e.context("listing in-scope source files"));
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

    // Validate trace links before finalize
    for root in &trace_root_list {
        let root_path = Path::new(root);
        if !root_path.exists() {
            if !quiet && !json_output {
                eprintln!("warning: trace root '{}' does not exist, skipping validation", root);
            }
            continue;
        }
        match read_all_trace_files(root) {
            Ok(TraceFiles { hlr, llr, tests, .. }) => {
                if let Err(e) = validate_trace_links(
                    &hlr.requirements,
                    &llr.requirements,
                    &tests.tests,
                ) {
                    if strict {
                        let err_msg = format!("Trace validation failed in '{}': {}", root, e);
                        if json_output {
                            let output = GenerateOutput {
                                success: false,
                                bundle_path: None,
                                profile: profile.to_string(),
                                git_sha: None,
                                error: Some(err_msg.clone()),
                            };
                            println!("{}", serde_json::to_string_pretty(&output)?);
                        } else {
                            eprintln!("error: {}", err_msg);
                        }
                        return Ok(EXIT_ERROR);
                    }
                    eprintln!("warning: trace validation failed in '{}': {}", root, e);
                } else if !quiet && !json_output {
                    println!("evidence: trace links valid in '{}'", root);
                }
            }
            Err(e) => {
                if strict {
                    return Err(e.context(format!("reading trace files from '{}'", root)));
                }
                eprintln!("warning: could not read trace files from '{}': {}", root, e);
            }
        }
    }

    // Finalize bundle
    let bundle_path = builder.finalize("0.0.1", "0.0.3", vec![])?;

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
        let output = GenerateOutput {
            success: true,
            bundle_path: Some(bundle_path.display().to_string()),
            profile: profile.to_string(),
            git_sha: Some(env_fp.git_sha),
            error: None,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if !quiet {
        println!("evidence: bundle created at {:?}", bundle_path);
    }

    Ok(EXIT_SUCCESS)
}

fn load_in_scope_crates(path: &Path) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("reading boundary config from {:?}", path))?;
    let config: toml::Value = toml::from_str(&content)?;

    if let Some(scope) = config.get("scope") {
        if let Some(in_scope) = scope.get("in_scope") {
            if let Some(arr) = in_scope.as_array() {
                return Ok(arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect());
            }
        }
    }
    Ok(Vec::new())
}

/// Load trace_roots from boundary.toml, falling back to ["cert/trace"].
fn load_trace_roots(path: &Path) -> Vec<String> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec!["cert/trace".to_string()],
    };
    let config: toml::Value = match toml::from_str(&content) {
        Ok(c) => c,
        Err(_) => return vec!["cert/trace".to_string()],
    };
    if let Some(scope) = config.get("scope") {
        if let Some(roots) = scope.get("trace_roots") {
            if let Some(arr) = roots.as_array() {
                let v: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                if !v.is_empty() {
                    return v;
                }
            }
        }
    }
    vec!["cert/trace".to_string()]
}

// ============================================================================
// Verify Command
// ============================================================================

#[derive(Serialize)]
struct VerifyOutput {
    success: bool,
    bundle_path: String,
    checks: Vec<VerifyCheck>,
    error: Option<String>,
}

#[derive(Serialize)]
struct VerifyCheck {
    name: String,
    status: String,
    message: Option<String>,
}

fn cmd_verify(
    bundle_path: PathBuf,
    _strict: bool,
    verify_key: Option<PathBuf>,
    json_output: bool,
) -> Result<i32> {
    let mut checks = Vec::new();

    // Check bundle exists
    if !bundle_path.exists() {
        let err_msg = format!("bundle not found: {:?}", bundle_path);
        if json_output {
            let output = VerifyOutput {
                success: false,
                bundle_path: bundle_path.display().to_string(),
                checks: vec![VerifyCheck {
                    name: "bundle_exists".to_string(),
                    status: "fail".to_string(),
                    message: Some(err_msg.clone()),
                }],
                error: Some(err_msg),
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            eprintln!("error: {}", err_msg);
        }
        return Ok(EXIT_VERIFICATION_FAILURE);
    }

    checks.push(VerifyCheck {
        name: "bundle_exists".to_string(),
        status: "pass".to_string(),
        message: None,
    });

    // Load verify key if provided
    let key_bytes = match &verify_key {
        Some(path) => Some(
            fs::read(path).with_context(|| format!("reading verify key from {:?}", path))?,
        ),
        None => None,
    };

    // Run verification
    match verify_bundle_with_key(&bundle_path, key_bytes.as_deref()) {
        Ok(VerifyResult::Pass) => {
            checks.push(VerifyCheck {
                name: "bundle_integrity".to_string(),
                status: "pass".to_string(),
                message: None,
            });
            checks.push(VerifyCheck {
                name: "sha256sums".to_string(),
                status: "pass".to_string(),
                message: None,
            });

            if json_output {
                let output = VerifyOutput {
                    success: true,
                    bundle_path: bundle_path.display().to_string(),
                    checks,
                    error: None,
                };
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("verify: PASS - bundle {:?}", bundle_path);
            }
            Ok(EXIT_SUCCESS)
        }
        Ok(VerifyResult::Fail(errors)) => {
            let reason = errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ");
            checks.push(VerifyCheck {
                name: "bundle_integrity".to_string(),
                status: "fail".to_string(),
                message: Some(reason.clone()),
            });

            if json_output {
                let output = VerifyOutput {
                    success: false,
                    bundle_path: bundle_path.display().to_string(),
                    checks,
                    error: Some(reason.clone()),
                };
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                eprintln!("verify: FAIL - {}", reason);
            }
            Ok(EXIT_VERIFICATION_FAILURE)
        }
        Ok(VerifyResult::Skipped(reason)) => {
            checks.push(VerifyCheck {
                name: "bundle_integrity".to_string(),
                status: "skipped".to_string(),
                message: Some(reason.clone()),
            });

            if json_output {
                let output = VerifyOutput {
                    success: true,
                    bundle_path: bundle_path.display().to_string(),
                    checks,
                    error: None,
                };
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("verify: SKIPPED - {}", reason);
            }
            Ok(EXIT_SUCCESS)
        }
        Err(e) => {
            if json_output {
                let output = VerifyOutput {
                    success: false,
                    bundle_path: bundle_path.display().to_string(),
                    checks,
                    error: Some(e.to_string()),
                };
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                eprintln!("verify: ERROR - {}", e);
            }
            Ok(EXIT_VERIFICATION_FAILURE)
        }
    }
}

// ============================================================================
// Diff Command
// ============================================================================

#[derive(Serialize)]
struct DiffOutput {
    bundle_a: String,
    bundle_b: String,
    inputs_diff: HashDiff,
    outputs_diff: HashDiff,
    metadata_diff: MetadataDiff,
}

#[derive(Serialize, Default)]
struct HashDiff {
    added: Vec<String>,
    removed: Vec<String>,
    changed: Vec<ChangedFile>,
}

#[derive(Serialize)]
struct ChangedFile {
    path: String,
    hash_a: String,
    hash_b: String,
}

#[derive(Serialize, Default)]
struct MetadataDiff {
    profile: Option<StringChange>,
    git_sha: Option<StringChange>,
    git_branch: Option<StringChange>,
    git_dirty: Option<BoolChange>,
}

#[derive(Serialize)]
struct StringChange {
    a: String,
    b: String,
}

#[derive(Serialize)]
struct BoolChange {
    a: bool,
    b: bool,
}

fn cmd_diff(bundle_a: PathBuf, bundle_b: PathBuf, json_output: bool) -> Result<i32> {
    // Load both indexes
    let index_a = load_index(&bundle_a)?;
    let index_b = load_index(&bundle_b)?;

    // Load hash files
    let inputs_a = load_hashes(&bundle_a.join("inputs_hashes.json"))?;
    let inputs_b = load_hashes(&bundle_b.join("inputs_hashes.json"))?;
    let outputs_a = load_hashes(&bundle_a.join("outputs_hashes.json"))?;
    let outputs_b = load_hashes(&bundle_b.join("outputs_hashes.json"))?;

    // Compute diffs
    let inputs_diff = compute_hash_diff(&inputs_a, &inputs_b);
    let outputs_diff = compute_hash_diff(&outputs_a, &outputs_b);

    // Compute metadata diff
    let mut metadata_diff = MetadataDiff::default();

    if index_a.profile != index_b.profile {
        metadata_diff.profile = Some(StringChange {
            a: index_a.profile.clone(),
            b: index_b.profile.clone(),
        });
    }
    if index_a.git_sha != index_b.git_sha {
        metadata_diff.git_sha = Some(StringChange {
            a: index_a.git_sha.clone(),
            b: index_b.git_sha.clone(),
        });
    }
    if index_a.git_branch != index_b.git_branch {
        metadata_diff.git_branch = Some(StringChange {
            a: index_a.git_branch.clone(),
            b: index_b.git_branch.clone(),
        });
    }
    if index_a.git_dirty != index_b.git_dirty {
        metadata_diff.git_dirty = Some(BoolChange {
            a: index_a.git_dirty,
            b: index_b.git_dirty,
        });
    }

    let diff_output = DiffOutput {
        bundle_a: bundle_a.display().to_string(),
        bundle_b: bundle_b.display().to_string(),
        inputs_diff,
        outputs_diff,
        metadata_diff,
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&diff_output)?);
    } else {
        println!(
            "Comparing bundles:\n  A: {:?}\n  B: {:?}\n",
            bundle_a, bundle_b
        );

        // Metadata changes
        println!("=== Metadata ===");
        if let Some(ref c) = diff_output.metadata_diff.profile {
            println!("  profile: {} -> {}", c.a, c.b);
        }
        if let Some(ref c) = diff_output.metadata_diff.git_sha {
            println!("  git_sha: {}... -> {}...", &c.a[..8.min(c.a.len())], &c.b[..8.min(c.b.len())]);
        }
        if let Some(ref c) = diff_output.metadata_diff.git_branch {
            println!("  git_branch: {} -> {}", c.a, c.b);
        }
        if let Some(ref c) = diff_output.metadata_diff.git_dirty {
            println!("  git_dirty: {} -> {}", c.a, c.b);
        }

        // Inputs diff
        println!("\n=== Inputs ===");
        print_hash_diff(&diff_output.inputs_diff);

        // Outputs diff
        println!("\n=== Outputs ===");
        print_hash_diff(&diff_output.outputs_diff);
    }

    Ok(EXIT_SUCCESS)
}

fn load_index(bundle: &Path) -> Result<EvidenceIndex> {
    let path = bundle.join("index.json");
    let content = fs::read_to_string(&path).with_context(|| format!("reading {:?}", path))?;
    serde_json::from_str(&content).with_context(|| "parsing index.json")
}

fn load_hashes(path: &Path) -> Result<BTreeMap<String, String>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let content = fs::read_to_string(path).with_context(|| format!("reading {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("parsing {:?}", path))
}

fn compute_hash_diff(a: &BTreeMap<String, String>, b: &BTreeMap<String, String>) -> HashDiff {
    let mut diff = HashDiff::default();

    // Files in A but not in B (removed)
    for key in a.keys() {
        if !b.contains_key(key) {
            diff.removed.push(key.clone());
        }
    }

    // Files in B but not in A (added)
    for key in b.keys() {
        if !a.contains_key(key) {
            diff.added.push(key.clone());
        }
    }

    // Files in both but with different hashes
    for (key, hash_a) in a {
        if let Some(hash_b) = b.get(key) {
            if hash_a != hash_b {
                diff.changed.push(ChangedFile {
                    path: key.clone(),
                    hash_a: hash_a.clone(),
                    hash_b: hash_b.clone(),
                });
            }
        }
    }

    diff
}

fn print_hash_diff(diff: &HashDiff) {
    if diff.added.is_empty() && diff.removed.is_empty() && diff.changed.is_empty() {
        println!("  (no changes)");
        return;
    }

    for f in &diff.added {
        println!("  + {}", f);
    }
    for f in &diff.removed {
        println!("  - {}", f);
    }
    for f in &diff.changed {
        println!("  ~ {} (hash changed)", f.path);
    }
}

// ============================================================================
// Init Command
// ============================================================================

const BOUNDARY_TEMPLATE: &str = r#"# Navigate Certification Boundary Configuration
# Schema version: 0.0.1

[schema]
version = "0.0.1"

[scope]
# Crates that are in scope for certification
in_scope = [
    # Add your certifiable crates here
    # "my-crate",
]

# Trace root directories (relative to workspace root)
trace_roots = ["cert/trace"]

# Workspace crates explicitly forbidden as dependencies
explicit_forbidden = []

[policy]
# Forbid dependencies on out-of-scope workspace crates
no_out_of_scope_deps = true

# Forbid build.rs in boundary crates (future)
forbid_build_rs = false

# Forbid proc-macros in boundary crates (future)
forbid_proc_macros = false

[forbidden_external]
# External crates that are forbidden with reasons
# "crate_name" = "reason"
"#;

const PROFILE_DEV: &str = r#"# Development Profile
# Relaxed checks for local development

[profile]
name = "dev"
description = "Development profile with relaxed checks"

[checks]
require_clean_git = false
require_coverage = false
allow_all_features = true
offline_required = false

[evidence]
include_timestamps = true
strict_hash_validation = false
fail_on_dirty = false
"#;

const PROFILE_CERT: &str = r#"# Certification Profile
# Strict checks for certification builds

[profile]
name = "cert"
description = "Certification profile with strict compliance checks"

[checks]
require_clean_git = true
require_coverage = true
allow_all_features = false
offline_required = true

[evidence]
include_timestamps = false
strict_hash_validation = true
fail_on_dirty = true
"#;

const PROFILE_RECORD: &str = r#"# Recording Profile
# Captures evidence without full enforcement

[profile]
name = "record"
description = "Recording profile for evidence capture"

[checks]
require_clean_git = true
require_coverage = false
allow_all_features = true
offline_required = false

[evidence]
include_timestamps = true
strict_hash_validation = false
fail_on_dirty = true
"#;

fn cmd_init(force: bool) -> Result<i32> {
    let cert_dir = PathBuf::from("cert");
    let profiles_dir = cert_dir.join("profiles");

    // Check if cert directory exists and not forcing
    if cert_dir.exists() && !force {
        eprintln!(
            "error: cert/ directory already exists. Use --force to overwrite."
        );
        return Ok(EXIT_ERROR);
    }

    // Create directories
    fs::create_dir_all(&profiles_dir)?;
    fs::create_dir_all(cert_dir.join("trace"))?;

    // Write boundary.toml
    let boundary_path = cert_dir.join("boundary.toml");
    if !boundary_path.exists() || force {
        fs::write(&boundary_path, BOUNDARY_TEMPLATE)?;
        println!("created: {:?}", boundary_path);
    }

    // Write profile configs
    let profiles = [
        ("dev.toml", PROFILE_DEV),
        ("cert.toml", PROFILE_CERT),
        ("record.toml", PROFILE_RECORD),
    ];

    for (name, content) in profiles {
        let path = profiles_dir.join(name);
        if !path.exists() || force {
            fs::write(&path, content)?;
            println!("created: {:?}", path);
        }
    }

    // Create example trace files
    let hlr_example = r#"# High-Level Requirements
# Schema version: 0.0.3

[[hlr]]
uid = "HLR-001"
title = "Example Requirement"
description = "This is an example high-level requirement."
owner = "team@example.com"
verification_methods = ["test", "review"]
"#;

    let llr_example = r#"# Low-Level Requirements
# Schema version: 0.0.3

[[llr]]
uid = "LLR-001"
title = "Example Implementation Requirement"
description = "This is an example low-level requirement."
owner = "developer@example.com"
derives_from = ["HLR-001"]
verification_methods = ["test"]
"#;

    let trace_dir = cert_dir.join("trace");
    let hlr_path = trace_dir.join("hlr.toml");
    let llr_path = trace_dir.join("llr.toml");

    if !hlr_path.exists() || force {
        fs::write(&hlr_path, hlr_example)?;
        println!("created: {:?}", hlr_path);
    }

    if !llr_path.exists() || force {
        fs::write(&llr_path, llr_example)?;
        println!("created: {:?}", llr_path);
    }

    println!("\nInitialized evidence tracking in cert/");
    println!("\nNext steps:");
    println!("  1. Edit cert/boundary.toml to define in-scope crates");
    println!("  2. Add requirements to cert/trace/hlr.toml and llr.toml");
    println!("  3. Run: cargo evidence generate --out-dir evidence");

    Ok(EXIT_SUCCESS)
}

// ============================================================================
// Schema Command
// ============================================================================

fn cmd_schema_show(schema: SchemaName) -> Result<i32> {
    let content = match schema {
        SchemaName::Index => SCHEMA_INDEX,
        SchemaName::Env => SCHEMA_ENV,
        SchemaName::Commands => SCHEMA_COMMANDS,
        SchemaName::Hashes => SCHEMA_HASHES,
    };
    println!("{}", content);
    Ok(EXIT_SUCCESS)
}

fn cmd_schema_validate(file: PathBuf) -> Result<i32> {
    // Read the file
    let content = fs::read_to_string(&file).with_context(|| format!("reading {:?}", file))?;

    // Parse as JSON
    let value: serde_json::Value =
        serde_json::from_str(&content).with_context(|| format!("parsing {:?} as JSON", file))?;

    // Determine which schema to use based on file name
    let file_name = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    let (schema_name, schema_content) = if file_name == "index.json" {
        ("index", SCHEMA_INDEX)
    } else if file_name == "env.json" {
        ("env", SCHEMA_ENV)
    } else if file_name == "commands.json" {
        ("commands", SCHEMA_COMMANDS)
    } else if file_name.contains("hashes") {
        ("hashes", SCHEMA_HASHES)
    } else {
        // Try to auto-detect based on content
        if value.get("schema_version").is_some() && value.get("bundle_complete").is_some() {
            ("index", SCHEMA_INDEX)
        } else if value.get("rustc").is_some() && value.get("cargo").is_some() {
            ("env", SCHEMA_ENV)
        } else if value.is_array() {
            ("commands", SCHEMA_COMMANDS)
        } else if value.is_object()
            && value
                .as_object()
                .map(|o| o.values().all(|v| v.is_string()))
                .unwrap_or(false)
        {
            ("hashes", SCHEMA_HASHES)
        } else {
            eprintln!("error: could not determine schema type for {:?}", file);
            eprintln!("hint: rename file to index.json, env.json, commands.json, or *_hashes.json");
            return Ok(EXIT_ERROR);
        }
    };

    // Parse the schema
    let _schema: serde_json::Value = serde_json::from_str(schema_content)?;

    // Basic validation: check required fields for each schema type
    let validation_result = match schema_name {
        "index" => validate_index_schema(&value),
        "env" => validate_env_schema(&value),
        "commands" => validate_commands_schema(&value),
        "hashes" => validate_hashes_schema(&value),
        _ => Ok(()),
    };

    match validation_result {
        Ok(()) => {
            println!("validate: PASS - {:?} is valid {} schema", file, schema_name);
            Ok(EXIT_SUCCESS)
        }
        Err(e) => {
            eprintln!("validate: FAIL - {}", e);
            Ok(EXIT_VERIFICATION_FAILURE)
        }
    }
}

fn validate_index_schema(value: &serde_json::Value) -> Result<()> {
    let required = [
        "schema_version",
        "boundary_schema_version",
        "trace_schema_version",
        "profile",
        "timestamp_rfc3339",
        "git_sha",
        "git_branch",
        "git_dirty",
        "engine_crate_version",
        "engine_git_sha",
        "inputs_hashes_file",
        "outputs_hashes_file",
        "commands_file",
        "env_fingerprint_file",
        "trace_roots",
        "trace_outputs",
        "bundle_complete",
        "content_hash",
    ];

    for field in required {
        if value.get(field).is_none() {
            bail!("missing required field: {}", field);
        }
    }
    Ok(())
}

fn validate_env_schema(value: &serde_json::Value) -> Result<()> {
    let required = [
        "profile",
        "rustc",
        "cargo",
        "git_sha",
        "git_branch",
        "git_dirty",
        "in_nix_shell",
        "tools",
        "nav_env",
    ];

    for field in required {
        if value.get(field).is_none() {
            bail!("missing required field: {}", field);
        }
    }
    Ok(())
}

fn validate_commands_schema(value: &serde_json::Value) -> Result<()> {
    if !value.is_array() {
        bail!("commands schema requires an array");
    }

    let arr = value.as_array().unwrap();
    for (i, item) in arr.iter().enumerate() {
        if item.get("argv").is_none() {
            bail!("command[{}] missing required field: argv", i);
        }
        if item.get("cwd").is_none() {
            bail!("command[{}] missing required field: cwd", i);
        }
        if item.get("exit_code").is_none() {
            bail!("command[{}] missing required field: exit_code", i);
        }
    }
    Ok(())
}

fn validate_hashes_schema(value: &serde_json::Value) -> Result<()> {
    if !value.is_object() {
        bail!("hashes schema requires an object");
    }

    let obj = value.as_object().unwrap();
    for (key, val) in obj {
        if !val.is_string() {
            bail!("hash value for '{}' must be a string", key);
        }
        let hash = val.as_str().unwrap();
        if hash.len() != 64 {
            bail!(
                "hash for '{}' must be 64 hex characters, got {}",
                key,
                hash.len()
            );
        }
        if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("hash for '{}' contains non-hex characters", key);
        }
    }
    Ok(())
}

// ============================================================================
// Trace Command
// ============================================================================

fn cmd_trace(do_validate: bool, do_backfill: bool, trace_roots_arg: Option<String>) -> Result<i32> {
    if !do_backfill && !do_validate {
        eprintln!("error: specify an action, e.g. --validate or --backfill-uuids");
        return Ok(EXIT_ERROR);
    }

    let roots: Vec<String> = trace_roots_arg
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
        .unwrap_or_else(|| load_trace_roots(Path::new("cert/boundary.toml")));

    // Validate trace links
    if do_validate {
        let mut all_valid = true;
        for root in &roots {
            let root_path = Path::new(root);
            if !root_path.exists() {
                eprintln!("warning: trace root '{}' does not exist, skipping", root);
                continue;
            }
            let TraceFiles { hlr, llr, tests, .. } = read_all_trace_files(root)?;
            match validate_trace_links(&hlr.requirements, &llr.requirements, &tests.tests) {
                Ok(()) => println!("trace: validation passed for '{}'", root),
                Err(e) => {
                    eprintln!("trace: validation FAILED for '{}': {}", root, e);
                    all_valid = false;
                }
            }
        }
        if !all_valid {
            return Ok(EXIT_ERROR);
        }
    }

    // Backfill UUIDs
    if do_backfill {
        let mut total = 0;
        for root in &roots {
            let root_path = Path::new(root);
            if !root_path.exists() {
                eprintln!("warning: trace root '{}' does not exist, skipping", root);
                continue;
            }
            let n = backfill_uuids(root)?;
            if n > 0 {
                println!("trace: assigned {} UUID(s) in {}", n, root);
            }
            total += n;
        }
        if total == 0 {
            println!("trace: all entries already have UUIDs");
        } else {
            println!("trace: assigned {} UUID(s) total", total);
        }
    }

    Ok(EXIT_SUCCESS)
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let exit_code = run();
    std::process::exit(exit_code);
}

fn run() -> i32 {
    let CargoCli::Evidence(args) = CargoCli::parse();

    let result = match args.command {
        Some(Commands::Generate {
            profile,
            out_dir,
            write_workspace,
            boundary,
            trace_roots,
            sign_key,
            quiet,
            json,
        }) => cmd_generate(GenerateArgs {
            profile_arg: profile,
            out_dir,
            write_workspace,
            boundary,
            trace_roots_arg: trace_roots,
            sign_key,
            quiet,
            json_output: json,
        }),
        Some(Commands::Verify {
            bundle_path,
            strict,
            verify_key,
            json,
        }) => cmd_verify(bundle_path, strict, verify_key, json),
        Some(Commands::Diff {
            bundle_a,
            bundle_b,
            json,
        }) => cmd_diff(bundle_a, bundle_b, json),
        Some(Commands::Init { force }) => cmd_init(force),
        Some(Commands::Schema { command }) => match command {
            SchemaCommands::Show { schema } => cmd_schema_show(schema),
            SchemaCommands::Validate { file } => cmd_schema_validate(file),
        },
        Some(Commands::Trace {
            validate,
            backfill_uuids,
            trace_roots,
        }) => cmd_trace(validate, backfill_uuids, trace_roots.or(args.trace_roots.clone())),
        None => {
            // Default to generate command with global args
            cmd_generate(GenerateArgs {
                profile_arg: args.profile,
                out_dir: args.out_dir,
                write_workspace: args.write_workspace,
                boundary: args.boundary,
                trace_roots_arg: args.trace_roots,
                sign_key: None, // no sign key from global args
                quiet: args.quiet,
                json_output: args.json,
            })
        }
    };

    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {:#}", e);
            EXIT_ERROR
        }
    }
}
