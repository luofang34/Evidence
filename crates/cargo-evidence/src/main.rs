//! cargo-evidence - Cargo subcommand for build evidence and reproducibility verification.
//!
//! This binary is a thin parse-and-dispatch shell. Every subcommand
//! lives in its own module under [`cli`]; output rendering — text vs
//! JSON — is centralized in [`cli::output`]. Keep this file short.
//!
//! **anyhow usage**: the `disallowed-types = ["anyhow::Error"]`
//! clippy rule in `clippy.toml` forbids `anyhow::Error` in *library*
//! code (the `evidence` crate), which must return typed thiserror
//! errors so callers can match on failure modes. This binary is the
//! CLI / main-function layer where an untyped error envelope is the
//! right tool — `?`-chaining typed library errors into a single
//! user-facing diagnostic. The crate-level allow below documents
//! that exemption explicitly; do not delete it without a plan to
//! migrate the CLI to a typed top-level error enum.
#![allow(
    clippy::disallowed_types,
    reason = "CLI is the anyhow/main-function layer; library code is typed via thiserror"
)]

use clap::Parser;
use evidence::diagnostic::{Diagnostic, Severity};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;

mod cli;

use cli::args::{CargoCli, Commands, EXIT_ERROR, EvidenceArgs, OutputFormat, SchemaCommands};
use cli::check::cmd_check;
use cli::diff::cmd_diff;
use cli::floors::cmd_floors;
use cli::generate::{GenerateArgs, cmd_generate};
use cli::init::cmd_init;
use cli::output::emit_jsonl;
use cli::rules::cmd_rules;
use cli::schema::{cmd_schema_show, cmd_schema_validate};
use cli::trace::cmd_trace;
use cli::verify::cmd_verify;

fn main() {
    let exit_code = run();
    std::process::exit(exit_code);
}

/// Install the `tracing` subscriber for the library's diagnostic
/// events.
///
/// - Writes to **stderr**: stdout is reserved for primary command
///   output (JSON envelopes, `schema show` emits, etc.). Mixing
///   diagnostics into stdout would break `cargo evidence ... | jq`.
/// - Default filter: `WARN`. Library `tracing::info!` calls (e.g.
///   `"verify: checking bundle at …"`) stay silent on normal runs.
///   Users who want verbose output set `RUST_LOG` (e.g.
///   `RUST_LOG=evidence=info`).
/// - `from_env_lossy` — **not** `try_from_default_env`. Nix's
///   `buildRustPackage` exports `RUST_LOG=""` in the sandbox; the
///   `try_*` path panics on empty, the `lossy` path degrades to
///   the default directive. See memory
///   `project_nix_rust_log_gotcha`.
///
/// Idempotent: installing a second subscriber is a no-op error that
/// we deliberately swallow so test binaries don't collide.
fn init_tracing() {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::WARN.into())
        .from_env_lossy();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .try_init()
        .ok();
}

fn run() -> i32 {
    init_tracing();
    let CargoCli::Evidence(args) = CargoCli::parse();
    match dispatch(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {:#}", e);
            EXIT_ERROR
        }
    }
}

fn dispatch(args: EvidenceArgs) -> anyhow::Result<i32> {
    // Which subcommand is this, stripped to a stable lowercase name
    // suitable for the `subcommand` field on a `CLI_SUBCOMMAND_ERROR`
    // terminal. `None` on the implicit-generate path.
    let subcommand_name: &str = match &args.command {
        Some(Commands::Generate { .. }) | None => "generate",
        Some(Commands::Verify { .. }) => "verify",
        Some(Commands::Check { .. }) => "check",
        Some(Commands::Diff { .. }) => "diff",
        Some(Commands::Init { .. }) => "init",
        Some(Commands::Schema { .. }) => "schema",
        Some(Commands::Trace { .. }) => "trace",
        Some(Commands::Rules { .. }) => "rules",
        Some(Commands::Floors { .. }) => "floors",
    };

    // Guard rail for subcommands that don't yet stream JSONL natively.
    // `verify` and `check` both emit JSONL directly; every other
    // subcommand under `--format=jsonl` would silently mix human /
    // JSON text on stdout (Schema Rule 2 violation). Hard-error
    // instead: emit a `CLI_UNSUPPORTED_FORMAT` finding +
    // `CLI_SUBCOMMAND_ERROR` terminal and return exit 1.
    //
    // TODO(jsonl): add subcommand names here as they gain JSONL
    // support.
    if args.format == OutputFormat::Jsonl && !matches!(subcommand_name, "verify" | "check") {
        return emit_unsupported_jsonl_terminal(subcommand_name);
    }

    match args.command {
        Some(Commands::Generate {
            sign_key,
            skip_tests,
        }) => cmd_generate(GenerateArgs {
            profile_arg: args.profile,
            out_dir: args.out_dir,
            write_workspace: args.write_workspace,
            boundary: args.boundary,
            trace_roots_arg: args.trace_roots,
            sign_key,
            skip_tests,
            quiet: args.quiet,
            json_output: args.json,
        }),
        Some(Commands::Verify {
            bundle_path,
            strict,
            verify_key,
            json,
        }) => {
            // Per-subcommand `--json` is kept for backward compat
            // even though the global `--json` (on `EvidenceArgs`)
            // already reaches this subcommand via `global = true`.
            // Logical-OR both into the format resolver so either
            // position works; an explicit `--format` always wins.
            let format = OutputFormat::resolve(args.format, args.json || json);
            cmd_verify(bundle_path, strict, verify_key, format)
        }
        Some(Commands::Check { mode, path }) => cmd_check(mode, path),
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
            require_hlr_sys_trace,
            check_test_selectors,
            json,
        }) => cmd_trace(
            validate,
            backfill_uuids,
            require_hlr_sys_trace,
            check_test_selectors,
            args.trace_roots,
            json,
        ),
        Some(Commands::Rules { json }) => {
            // `rules` emits a single blob (JSON array or human table),
            // not a JSONL stream, so it's already filtered out of the
            // JSONL dispatch guard above. Honour the per-subcommand
            // `--json` flag, or the global `--json` (via `args.json`).
            cmd_rules(json || args.json)
        }
        Some(Commands::Floors { json, config }) => {
            // Same blob-not-stream shape as `rules`.
            cmd_floors(json || args.json, config)
        }
        // No subcommand given — default to generate with global args.
        None => cmd_generate(GenerateArgs {
            profile_arg: args.profile,
            out_dir: args.out_dir,
            write_workspace: args.write_workspace,
            boundary: args.boundary,
            trace_roots_arg: args.trace_roots,
            sign_key: None,
            skip_tests: false,
            quiet: args.quiet,
            json_output: args.json,
        }),
    }
}

/// Stream the two-event JSONL envelope for an unwired `--format=jsonl`
/// subcommand: a `CLI_UNSUPPORTED_FORMAT` finding explaining what
/// happened, then a `CLI_SUBCOMMAND_ERROR` terminal carrying the
/// subcommand name so agents can route by it.
///
/// Exit code is [`EXIT_ERROR`] (1) — matches the `_ERROR` terminal
/// suffix per Schema Rule 1.
fn emit_unsupported_jsonl_terminal(subcommand: &str) -> anyhow::Result<i32> {
    emit_jsonl(&Diagnostic {
        code: "CLI_UNSUPPORTED_FORMAT".to_string(),
        severity: Severity::Error,
        message: format!(
            "subcommand '{}' does not support --format=jsonl yet; only 'verify' streams JSONL natively",
            subcommand
        ),
        location: None,
        fix_hint: None,
        subcommand: Some(subcommand.to_string()),
        root_cause_uid: None,
    })?;
    emit_jsonl(&Diagnostic {
        code: "CLI_SUBCOMMAND_ERROR".to_string(),
        severity: Severity::Error,
        message: format!(
            "subcommand '{}' aborted: --format=jsonl not supported",
            subcommand
        ),
        location: None,
        fix_hint: None,
        subcommand: Some(subcommand.to_string()),
        root_cause_uid: None,
    })?;
    Ok(EXIT_ERROR)
}
