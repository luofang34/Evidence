//! `cargo evidence schema {show|validate}` clap types — split from
//! the parent `args.rs` facade to stay under the workspace 500-line
//! file-size limit.
//!
//! `SchemaCommands` is the nested subcommand variant under
//! `Commands::Schema`; `SchemaName` is the value-enum that backs
//! `schema show <name>`. Both stay in `cargo_evidence::cli::args`
//! via the `pub use` re-export in the parent module.

use std::path::PathBuf;

use clap::{Subcommand, ValueEnum};

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
