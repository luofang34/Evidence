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
// The `cli::*` modules are binary-internal implementation detail,
// not a public API. Per-item doc comments on every CLI arg struct
// and subcommand handler would be boilerplate without a consumer to
// serve. Doc enforcement stays on the `evidence` library crate via
// workspace lints.
#![allow(
    missing_docs,
    reason = "CLI internals; docs are enforced on the `evidence` library"
)]

use clap::Parser;

mod cli;

use cli::args::{CargoCli, Commands, EXIT_ERROR, EvidenceArgs, SchemaCommands};
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

fn run() -> i32 {
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
