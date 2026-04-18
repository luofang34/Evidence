//! CLI argument types, exit code constants, and environment detection.
//!
//! The clap-derive types in this module (`CargoCli`, `EvidenceArgs`,
//! `Commands`, `SchemaCommands`, `SchemaName`) carry their user-facing
//! documentation in `#[arg(help = ...)]` / `/// ...` on each field or
//! variant — `--help` output is the real surface. A redundant layer of
//! rustdoc `//!` prose on the struct header would restate the same
//! text, which is why each type is tagged with a narrow
//! `#[allow(missing_docs, …)]` rather than carrying an extra
//! struct-level doc comment.
//!
//! **clap vs. hand-rolled parser**: clap is the workspace's CLI
//! framework today. It fits the cargo-subcommand ergonomic sweet spot
//! (derived `--help`, global args, subcommand nesting) and is the de
//! facto Rust standard, so the qualification / review audience is
//! already familiar with it. The cost — ~150KB binary overhead and a
//! proc-macro chain in the tool-qualification (DO-330 TQL-5) audit
//! surface — is acceptable while the tool is pre-1.0. If / when we
//! approach formal tool qualification and the proc-macro surface
//! becomes a load-bearing audit cost, the CLI shell is small enough
//! that swapping in a minimal parser (`lexopt` or `pico-args`) would
//! be a single-PR change. Not worth the churn now.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use evidence::{Profile, env::in_nix_shell};

// ============================================================================
// Exit Codes
// ============================================================================

/// Process exit code for a successful run.
pub const EXIT_SUCCESS: i32 = 0;
/// Process exit code for a CLI / generation / I/O error — anything
/// that prevented the command from producing a result.
pub const EXIT_ERROR: i32 = 1;
/// Process exit code reserved for `verify` when the bundle parsed but
/// failed integrity / policy checks. Kept distinct from [`EXIT_ERROR`]
/// so CI can react differently to "tool crashed" vs "bundle broken".
pub const EXIT_VERIFICATION_FAILURE: i32 = 2;

// ============================================================================
// CLI Parsing
// ============================================================================

#[derive(Parser)]
#[command(name = "cargo")]
#[command(bin_name = "cargo")]
#[allow(
    missing_docs,
    reason = "clap-derive: variant help is carried by `#[command]` / clap itself"
)]
pub enum CargoCli {
    /// Build evidence and reproducibility verification
    Evidence(EvidenceArgs),
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[allow(
    missing_docs,
    reason = "clap-derive: field help is carried by `#[arg(help = ...)]`"
)]
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
#[allow(
    missing_docs,
    reason = "clap-derive: variant help is carried by `///` doc comments already present on each variant"
)]
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
#[allow(
    missing_docs,
    reason = "clap-derive: variant help is carried by `///` doc comments already present on each variant"
)]
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
#[allow(
    missing_docs,
    reason = "clap-derive ValueEnum: variant names are themselves the `--schema <name>` surface"
)]
pub enum SchemaName {
    Index,
    Env,
    Commands,
    Hashes,
    /// Alias for deterministic-manifest.json.
    #[value(name = "deterministic-manifest", alias = "manifest")]
    DeterministicManifest,
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
