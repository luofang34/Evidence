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
    ///
    /// Permanent alias for `--format=json`; not deprecated. When both
    /// `--json` and `--format` are given, `--format` wins.
    #[arg(long, global = true)]
    pub json: bool,

    /// Output format for machine consumers.
    ///
    /// - `human` (default): human-readable text on stdout + stderr.
    /// - `json`: single terminal JSON object on stdout (same as `--json`).
    /// - `jsonl`: streaming JSON-Lines on stdout, one diagnostic per
    ///   line, flushed per event. stderr keeps human progress text.
    ///
    /// The streaming `jsonl` shape is defined by
    /// `schemas/diagnostic.schema.json`
    /// (print with `cargo evidence schema show diagnostic`).
    #[arg(long, value_enum, global = true, default_value_t = OutputFormat::Human)]
    pub format: OutputFormat,
}

/// Selects how `check` interprets its path argument.
///
/// - `Auto` (default): inspect the path. Containing `SHA256SUMS`
///   wins (bundle mode); else containing `Cargo.toml` (source
///   mode); else `CLI_INVALID_ARGUMENT`.
/// - `Source`: force source mode; reject a bundle dir.
/// - `Bundle`: force bundle mode; reject a source tree.
#[derive(Clone, Copy, Default, PartialEq, Eq, ValueEnum, Debug)]
pub enum CheckMode {
    /// Pick mode from the path shape (default).
    #[default]
    Auto,
    /// Force source-tree mode (trace validation + test run).
    Source,
    /// Force bundle mode (delegate to `verify`).
    Bundle,
}

/// Global `--format` choice. See [`EvidenceArgs::format`].
#[derive(Clone, Copy, Default, PartialEq, Eq, ValueEnum, Debug)]
pub enum OutputFormat {
    /// Human-readable text (default).
    #[default]
    Human,
    /// Single pretty-printed JSON document on stdout.
    Json,
    /// Streaming JSON-Lines on stdout, flushed per event.
    Jsonl,
}

impl OutputFormat {
    /// Resolve the effective output format from the possibly-multiple
    /// knobs the user may have set.
    ///
    /// Precedence:
    /// 1. If `--format` is anything other than its default (`Human`),
    ///    honor it (the user was explicit).
    /// 2. Otherwise, if the legacy `--json` boolean is set, treat as
    ///    `Json` (Schema Rule 5: permanent alias).
    /// 3. Otherwise `Human`.
    pub fn resolve(format_flag: OutputFormat, json_flag: bool) -> OutputFormat {
        if format_flag != OutputFormat::Human {
            return format_flag;
        }
        if json_flag {
            return OutputFormat::Json;
        }
        OutputFormat::Human
    }
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

    /// One-shot agent-facing validation (source tree or bundle)
    ///
    /// Auto-detects whether the path is a source tree (has `Cargo.toml`)
    /// or a bundle (has `SHA256SUMS`) and dispatches accordingly.
    /// Source mode emits per-requirement `REQ_PASS` / `REQ_GAP` /
    /// `REQ_SKIP` diagnostics plus the aggregate terminal. Bundle mode
    /// delegates to `verify`. Use `check` as the default; `verify` is
    /// kept as a low-level primitive for CI scripts.
    Check {
        /// Auto-detect mode (default), force source mode, or force
        /// bundle mode. Mode mismatch with the path shape emits
        /// `CLI_INVALID_ARGUMENT` rather than silently running the
        /// wrong pipeline.
        #[arg(long, value_enum, default_value_t = CheckMode::Auto)]
        mode: CheckMode,

        /// Path to check. Defaults to `.` (the current directory).
        path: Option<PathBuf>,
    },

    /// List every diagnostic code the tool can emit (self-describe).
    ///
    /// Agents use this to bootstrap their knowledge of the tool's
    /// observable surface without triggering each code. The JSON
    /// mode is what MCP (PR #50) wraps.
    Rules {
        /// Emit the manifest as a JSON array to stdout. Without
        /// this flag, a human-readable table is printed instead.
        #[arg(long)]
        json: bool,
    },

    /// Run the ratcheting-floors gate (PR #48 / principle 2).
    ///
    /// Reads `cert/floors.toml`, measures every dimension listed in
    /// `[floors]`, and fails with `FLOORS_BELOW_MIN` if any current
    /// measurement is below its committed floor. Exit 0 on pass,
    /// exit 2 on gate failure. Delta ceilings (new-additions-in-diff
    /// checks) land with the CI-wiring commit.
    Floors {
        /// Emit a deterministic JSON array to stdout. Without this
        /// flag, a human-readable table is printed.
        #[arg(long)]
        json: bool,
    },

    /// Trace management utilities
    Trace {
        /// Validate trace links between HLR, LLR, and Tests
        #[arg(long)]
        validate: bool,

        /// Assign UUIDs to entries that are missing them
        #[arg(long)]
        backfill_uuids: bool,

        /// Require every HLR to trace up to a System Requirement.
        ///
        /// When set, an HLR with empty `traces_to` fails Link-phase
        /// validation. Off by default; projects without a SYS layer
        /// keep validating cleanly. The tool's own CI enables this
        /// flag on `tool/trace/` to keep the SYS layer load-bearing.
        #[arg(long)]
        require_hlr_sys_trace: bool,

        /// Resolve each test's `test_selector` against a real
        /// `#[test] fn` in the workspace source.
        ///
        /// Catches the silent-rot failure mode where renaming a
        /// test function leaves `traces_to` UUID-valid but the
        /// selector dangling. Opt-in because the resolver walks
        /// every `.rs` file under the workspace root.
        #[arg(long)]
        check_test_selectors: bool,

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
    /// Wire-format schema for `--format=jsonl` output. Not a bundle
    /// file — `schema validate` will not match it by filename; use
    /// `schema show diagnostic` to read the source.
    Diagnostic,
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
