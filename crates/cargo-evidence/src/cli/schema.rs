//! `cargo evidence schema show|validate`.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use super::args::{EXIT_ERROR, EXIT_SUCCESS, EXIT_VERIFICATION_FAILURE, SchemaName};

// ============================================================================
// Embedded Schemas
// ============================================================================

const SCHEMA_INDEX: &str = include_str!("../../../../schemas/index.schema.json");
const SCHEMA_ENV: &str = include_str!("../../../../schemas/env.schema.json");
const SCHEMA_COMMANDS: &str = include_str!("../../../../schemas/commands.schema.json");
const SCHEMA_HASHES: &str = include_str!("../../../../schemas/hashes.schema.json");

pub fn cmd_schema_show(schema: SchemaName) -> Result<i32> {
    let content = match schema {
        SchemaName::Index => SCHEMA_INDEX,
        SchemaName::Env => SCHEMA_ENV,
        SchemaName::Commands => SCHEMA_COMMANDS,
        SchemaName::Hashes => SCHEMA_HASHES,
    };
    println!("{}", content);
    Ok(EXIT_SUCCESS)
}

pub fn cmd_schema_validate(file: PathBuf) -> Result<i32> {
    // Read the file
    let content = fs::read_to_string(&file).with_context(|| format!("reading {:?}", file))?;

    // Parse as JSON
    let value: serde_json::Value =
        serde_json::from_str(&content).with_context(|| format!("parsing {:?} as JSON", file))?;

    // Determine which schema to use based on file name
    let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");

    let schema_name = if file_name == "index.json" {
        "index"
    } else if file_name == "env.json" {
        "env"
    } else if file_name == "commands.json" {
        "commands"
    } else if file_name.contains("hashes") {
        "hashes"
    } else {
        // Try to auto-detect based on content
        if value.get("schema_version").is_some() && value.get("bundle_complete").is_some() {
            "index"
        } else if value.get("rustc").is_some() && value.get("cargo").is_some() {
            "env"
        } else if value.is_array() {
            "commands"
        } else if value.is_object()
            && value
                .as_object()
                .map(|o| o.values().all(|v| v.is_string()))
                .unwrap_or(false)
        {
            "hashes"
        } else {
            eprintln!("error: could not determine schema type for {:?}", file);
            eprintln!("hint: rename file to index.json, env.json, commands.json, or *_hashes.json");
            return Ok(EXIT_ERROR);
        }
    };

    // Basic validation: check required fields for each schema type
    let validation_result = match schema_name {
        "index" => validate_index_schema(&value),
        "env" => validate_env_schema(&value),
        "commands" => validate_commands_schema(&value),
        "hashes" => validate_hashes_schema(&value),
        _ => Ok(()),
    };

    match validation_result {
        Ok(()) => {
            println!(
                "validate: PASS - {:?} is valid {} schema",
                file, schema_name
            );
            Ok(EXIT_SUCCESS)
        }
        Err(e) => {
            eprintln!("validate: FAIL - {}", e);
            Ok(EXIT_VERIFICATION_FAILURE)
        }
    }
}

fn validate_index_schema(value: &serde_json::Value) -> Result<()> {
    let required = [
        "schema_version",
        "boundary_schema_version",
        "trace_schema_version",
        "profile",
        "timestamp_rfc3339",
        "git_sha",
        "git_branch",
        "git_dirty",
        "engine_crate_version",
        "engine_git_sha",
        "inputs_hashes_file",
        "outputs_hashes_file",
        "commands_file",
        "env_fingerprint_file",
        "trace_roots",
        "trace_outputs",
        "bundle_complete",
        "content_hash",
    ];

    for field in required {
        if value.get(field).is_none() {
            bail!("missing required field: {}", field);
        }
    }
    Ok(())
}

fn validate_env_schema(value: &serde_json::Value) -> Result<()> {
    let required = [
        "profile",
        "rustc",
        "cargo",
        "git_sha",
        "git_branch",
        "git_dirty",
        "in_nix_shell",
        "tools",
        "nav_env",
        "host",
        "target_triple",
    ];

    for field in required {
        if value.get(field).is_none() {
            bail!("missing required field: {}", field);
        }
    }

    // `host` is a tagged enum; every valid shape has `os` and `arch`.
    let host = value
        .get("host")
        .ok_or_else(|| anyhow::anyhow!("missing required field: host"))?;
    let host_os = host
        .get("os")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("host.os must be a string"))?;
    if !matches!(host_os, "linux" | "macos" | "windows") {
        bail!(
            "host.os must be one of linux|macos|windows, got {}",
            host_os
        );
    }
    if host.get("arch").and_then(|v| v.as_str()).is_none() {
        bail!("host.arch must be a string");
    }
    Ok(())
}

fn validate_commands_schema(value: &serde_json::Value) -> Result<()> {
    let arr = value
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("commands schema requires an array"))?;
    for (i, item) in arr.iter().enumerate() {
        if item.get("argv").is_none() {
            bail!("command[{}] missing required field: argv", i);
        }
        if item.get("cwd").is_none() {
            bail!("command[{}] missing required field: cwd", i);
        }
        if item.get("exit_code").is_none() {
            bail!("command[{}] missing required field: exit_code", i);
        }
    }
    Ok(())
}

fn validate_hashes_schema(value: &serde_json::Value) -> Result<()> {
    let obj = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("hashes schema requires an object"))?;
    for (key, val) in obj {
        let hash = val
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("hash value for '{}' must be a string", key))?;
        if hash.len() != 64 {
            bail!(
                "hash for '{}' must be 64 hex characters, got {}",
                key,
                hash.len()
            );
        }
        if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("hash for '{}' contains non-hex characters", key);
        }
    }
    Ok(())
}
