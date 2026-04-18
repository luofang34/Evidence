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
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;

mod cli;

use cli::args::{CargoCli, Commands, EXIT_ERROR, EvidenceArgs, OutputFormat, SchemaCommands};
use cli::diff::cmd_diff;
use cli::generate::{GenerateArgs, cmd_generate};
use cli::init::cmd_init;
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
            json,
        }) => cmd_trace(validate, backfill_uuids, args.trace_roots, json),
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
