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
use evidence_core::{Profile, env::in_nix_shell};

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

/// `--coverage` level for `cargo evidence generate`.
///
/// Controls the structural-coverage phase that runs between
/// `cargo test` and `finalize` when enabled. See HLR-053.
/// Captured percentages are engineering metrics only — no
/// percentage alone closes a DO-178C A-7 objective (LLR-108).
///
/// Default is resolved at runtime by profile: `Dev` → `None`
/// (fast iteration), `Cert`/`Record` → `Branch` (the broadest
/// approximation the tool captures for named-claim profiles).
/// Passing `--coverage` explicitly always wins.
#[derive(Clone, Copy, Default, PartialEq, Eq, ValueEnum, Debug)]
pub enum CoverageChoice {
    /// Do not run `cargo llvm-cov`; bundle carries no coverage
    /// artifact. Dev-profile default.
    #[default]
    None,
    /// Line / statement coverage only (A-7 Obj-5 dimension,
    /// engineering metric).
    Line,
    /// LLVM branch coverage — an approximation of decision
    /// coverage (A-7 Obj-6 dimension). Cert/record default.
    Branch,
    /// Emit both `line` and `branch` measurements from the
    /// same instrumented test pass.
    All,
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
        /// Path to the ed25519 signing (private) key file (32-byte
        /// hex, 64 chars + optional trailing newline). Defaults to
        /// `cert/signing.key` and `$EVIDENCE_SIGNING_KEY_PATH` in
        /// that order; cert/record profiles fail if no key resolves.
        #[arg(long)]
        signing_key: Option<PathBuf>,

        /// Skip running cargo test during evidence generation
        #[arg(long)]
        skip_tests: bool,

        /// Inventory + hash the build's deliverables into
        /// `outputs_hashes.json` even when `--skip-tests` is set.
        ///
        /// The inventory phase runs its own
        /// `cargo build --message-format=json`, so it does not need the
        /// test phase. This exercises the output-attestation path
        /// without the full test suite. No effect without `--skip-tests`
        /// (a full generate always inventories).
        #[arg(long)]
        inventory_outputs: bool,

        /// Structural coverage level to capture via cargo-llvm-cov.
        ///
        /// Runs an instrumented test pass between the plain
        /// `cargo test` phase and `finalize`, writing
        /// `coverage/coverage_summary.json` (typed) +
        /// `coverage/lcov.info` (raw passthrough) into the
        /// bundle. If the flag is omitted, the effective level
        /// is profile-derived — `none` on dev, `branch` on
        /// cert/record. See HLR-053.
        #[arg(long, value_enum)]
        coverage: Option<CoverageChoice>,
    },

    /// Verify an evidence bundle
    Verify {
        /// Path to the evidence bundle directory
        bundle_path: PathBuf,

        /// Fail on any warning
        #[arg(long)]
        strict: bool,

        /// Path to the ed25519 verifying (public) key file (32-byte
        /// hex, 64 chars + optional trailing newline). Defaults to
        /// `cert/signing.pub` and `$EVIDENCE_VERIFY_KEY_PATH` in
        /// that order.
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

        /// Emit agent-context scaffold (root `CLAUDE.md` +
        /// `.claude/settings.json`) alongside the `cert/` tree.
        /// Defaults to enabled; pass `--no-agent-context` to skip.
        /// Existing files are preserved either way.
        #[arg(long, conflicts_with = "no_agent_context")]
        with_agent_context: bool,

        /// Skip the agent-context scaffold; only the `cert/`
        /// tree is written. Mutually exclusive with
        /// `--with-agent-context`.
        #[arg(long, conflicts_with = "with_agent_context")]
        no_agent_context: bool,
    },

    /// Manage the project's ed25519 signing keypair (lifecycle: create / rotate).
    ///
    /// First run writes `cert/signing.key` (private; gitignored)
    /// and `cert/signing.pub` (public; commit). Refuses if either
    /// file already exists. To rotate an existing keypair, pass
    /// `--rotate --reason <text>`; the rotation appends one line
    /// to `cert/KEY-ROTATION-LOG` so the transition is reviewable.
    Keygen {
        /// Replace an existing keypair. Refuses unless both
        /// `signing.key` and `signing.pub` already exist.
        #[arg(long)]
        rotate: bool,

        /// Reason for rotation. Required with `--rotate`; recorded
        /// alongside the new public-key fingerprint and timestamp
        /// in `cert/KEY-ROTATION-LOG`.
        #[arg(long)]
        reason: Option<String>,

        /// Override the directory holding `signing.key`,
        /// `signing.pub`, and `KEY-ROTATION-LOG`. Defaults to
        /// `cert/`.
        #[arg(long)]
        out_dir: Option<PathBuf>,
    },

    /// Manage and validate evidence schemas
    Schema {
        #[command(subcommand)]
        command: SchemaCommands,
    },

    /// Audit downstream rigor adoption (trace + floors + boundary + CI + merge-style + override-docs)
    ///
    /// Runs a checklist against the current workspace and renders the
    /// result. Default: human-readable `[✓]` / `[⚠]` / `[✗]` table +
    /// `DOCTOR_OK` / `DOCTOR_FAIL` footer. `--json` streams one
    /// JSONL `Diagnostic` per check + terminal for agents / CI.
    ///
    /// `generate --profile cert` / `record` invokes doctor internally
    /// before bundle assembly and escalates warnings to blockers —
    /// cert-profile bundles can't be produced while any finding stands.
    Doctor {
        /// Emit streaming JSONL on stdout (one diagnostic per line + terminal).
        /// Without this flag, a human-readable summary is printed instead.
        #[arg(long)]
        json: bool,
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
    /// mode is what MCP wraps.
    Rules {
        /// Emit the manifest as a JSON array to stdout. Without
        /// this flag, a human-readable table is printed instead.
        #[arg(long)]
        json: bool,
    },

    /// Run the ratcheting-floors gate (principle 2).
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

        /// Path to the floors config. Defaults to
        /// `cert/floors.toml` under the current directory. Used by
        /// integration tests that exercise tampered floor values
        /// without clobbering the committed file.
        #[arg(long)]
        config: Option<PathBuf>,
    },

    /// Return the per-module trace + boundary + floors slice an agent
    /// needs before editing a source file.
    ///
    /// Selector resolution order (priority on ambiguity, first match
    /// wins): file > crate > module > workspace.
    ///
    /// - Positional argument: workspace-relative file path under
    ///   `crates/<crate>/...`, a workspace crate name, or a Rust
    ///   module path (`evidence_core::trace`).
    /// - `--crate <name>` and `--module <path>` are equivalent
    ///   alternative entry points to disambiguate when a name could
    ///   match more than one kind.
    /// - With no arguments, returns the workspace overview (root
    ///   `CLAUDE.md` pointer + workspace-wide floors).
    ///
    /// Output is text (default), `--json` (single blob), or
    /// `--format=jsonl` (one report line + one diagnostic per warning
    /// + a `CONTEXT_OK` / `CONTEXT_FAIL` / `CONTEXT_ERROR` terminal).
    Context {
        /// File / crate / module selector. Mutually compatible with
        /// `--crate` / `--module`; positional wins when given.
        selector: Option<String>,

        /// Disambiguate as a workspace crate name.
        #[arg(long = "crate")]
        crate_flag: Option<String>,

        /// Disambiguate as a Rust module path
        /// (e.g. `evidence_core::trace`).
        #[arg(long = "module")]
        module_flag: Option<String>,

        /// Emit a single pretty-printed JSON blob on stdout.
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
        /// flag on `cert/trace/` to keep the SYS layer load-bearing.
        #[arg(long)]
        require_hlr_sys_trace: bool,

        /// Enforce the `HlrEntry.surfaces` ⇔ `KNOWN_SURFACES`
        /// bijection (HLR-038).
        ///
        /// When set, every `surfaces` claim must be in
        /// `KNOWN_SURFACES`, and every `KNOWN_SURFACES` entry must
        /// be claimed by at least one HLR. Off by default; external
        /// projects without surface catalog coverage keep validating
        /// cleanly. The tool's own CI enables this flag.
        #[arg(long)]
        require_hlr_surface_bijection: bool,

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

mod schema;

pub use schema::{SchemaCommands, SchemaName};

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
