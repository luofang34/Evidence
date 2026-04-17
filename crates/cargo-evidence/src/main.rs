//! cargo-evidence - Cargo subcommand for build evidence and reproducibility verification.
//!
//! This binary is a thin parse-and-dispatch shell. Every subcommand
//! lives in its own module under [`cli`]; output rendering — text vs
//! JSON — is centralized in [`cli::output`]. Keep this file short.

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
