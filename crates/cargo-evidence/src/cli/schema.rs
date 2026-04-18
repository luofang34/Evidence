//! `cargo evidence schema show|validate`.
//!
//! Thin wrapper around [`evidence::schema`]: all validation logic
//! (including the real Draft 2020-12 JSON-Schema enforcement) lives
//! in the library so downstream consumers of the `evidence` crate
//! get the same guarantees as the CLI.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use evidence::schema::{Schema, validate};

use super::args::{EXIT_ERROR, EXIT_SUCCESS, EXIT_VERIFICATION_FAILURE, SchemaName};

/// `cargo evidence schema show <name>` handler: print a bundle file's
/// JSON Schema source to stdout.
pub fn cmd_schema_show(schema: SchemaName) -> Result<i32> {
    let source = match schema {
        SchemaName::Index => Schema::Index,
        SchemaName::Env => Schema::Env,
        SchemaName::Commands => Schema::Commands,
        SchemaName::Hashes => Schema::Hashes,
        SchemaName::DeterministicManifest => Schema::DeterministicManifest,
    }
    .source();
    println!("{}", source);
    Ok(EXIT_SUCCESS)
}

/// `cargo evidence schema validate <file>` handler: validate a JSON
/// file against the correct schema (picked by filename, with
/// content-based fallback). Returns
/// [`EXIT_VERIFICATION_FAILURE`]
/// — not [`EXIT_ERROR`] — on a schema
/// violation so CI can distinguish "tool broke" from "input bad".
pub fn cmd_schema_validate(file: PathBuf) -> Result<i32> {
    // Read the file
    let content = fs::read_to_string(&file).with_context(|| format!("reading {:?}", file))?;

    // Parse as JSON
    let value: serde_json::Value =
        serde_json::from_str(&content).with_context(|| format!("parsing {:?} as JSON", file))?;

    // Determine which schema to use: filename first, falling back to
    // content-based detection for files that don't follow the usual
    // `<name>.json` / `*_hashes.json` convention.
    let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let schema = match Schema::for_filename(file_name).or_else(|| Schema::for_content(&value)) {
        Some(s) => s,
        None => {
            eprintln!("error: could not determine schema type for {:?}", file);
            eprintln!("hint: rename file to index.json, env.json, commands.json, or *_hashes.json");
            return Ok(EXIT_ERROR);
        }
    };

    match validate(schema, &value) {
        Ok(()) => {
            println!(
                "validate: PASS - {:?} is valid {} schema",
                file,
                schema.name()
            );
            Ok(EXIT_SUCCESS)
        }
        Err(e) => {
            eprintln!("validate: FAIL - {}", e);
            Ok(EXIT_VERIFICATION_FAILURE)
        }
    }
}
