//! CLI argument types, exit code constants, and environment detection.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use evidence::{Profile, env::in_nix_shell};

// ============================================================================
// Exit Codes
// ============================================================================

pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_ERROR: i32 = 1;
pub const EXIT_VERIFICATION_FAILURE: i32 = 2;

// ============================================================================
// CLI Parsing
// ============================================================================

#[derive(Parser)]
#[command(name = "cargo")]
#[command(bin_name = "cargo")]
pub enum CargoCli {
    /// Build evidence and reproducibility verification
    Evidence(EvidenceArgs),
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct EvidenceArgs {
    #[command(subcommand)]
    pub command: Option<Commands>,

    // Default to generate if no subcommand given
    /// Build profile [dev, cert, record] (auto-detected if not specified)
    #[arg(long, global = true)]
    pub profile: Option<String>,

    /// Output directory for bundles (required unless --write-workspace)
    #[arg(long, global = true)]
    pub out_dir: Option<PathBuf>,

    /// Allow writing to workspace (dangerous, for xtask integration)
    #[arg(long, global = true)]
    pub write_workspace: bool,

    /// Path to boundary.toml
    #[arg(long, global = true)]
    pub boundary: Option<PathBuf>,

    /// Comma-separated list of trace root directories
    #[arg(long, global = true)]
    pub trace_roots: Option<String>,

    /// Suppress non-error output
    #[arg(long, short, global = true)]
    pub quiet: bool,

    /// Output results as JSON
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Generate a new evidence bundle for the current build (default command)
    Generate {
        /// Path to HMAC signing key file (raw bytes)
        #[arg(long)]
        sign_key: Option<PathBuf>,

        /// Skip running cargo test during evidence generation
        #[arg(long)]
        skip_tests: bool,
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

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum SchemaCommands {
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
pub enum SchemaName {
    Index,
    Env,
    Commands,
    Hashes,
}

// ============================================================================
// Environment Detection
// ============================================================================

/// Three-tier auto-detection for build profile.
///
/// `NAV_RECORD` → Record; IN_NIX_SHELL + CI → Cert; otherwise Dev.
pub fn detect_profile() -> Profile {
    if std::env::var("NAV_RECORD").is_ok() {
        Profile::Record
    } else if in_nix_shell() && is_ci() {
        Profile::Cert
    } else {
        Profile::Dev
    }
}

/// True when running inside a CI environment.
pub fn is_ci() -> bool {
    std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok()
}

/// Refuse to run when the working copy is a shallow clone. Evidence
/// generation needs full history to resolve git SHAs reliably.
pub fn check_shallow_clone() -> Result<()> {
    if Path::new(".git/shallow").exists() {
        bail!(
            "Shallow clone detected. Evidence generation requires full repository history.\n\
             Run: git fetch --unshallow"
        );
    }
    Ok(())
}

/// Best-effort "is the working tree dirty" check. Defaults to `true`
/// on error so that safety-critical profiles fail-closed when git is
/// unreachable.
pub fn is_git_dirty() -> bool {
    use std::process::Command;
    Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(true)
}
