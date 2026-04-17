//! Bundle JSON Schemas and validation.
//!
//! The four JSON Schema files under `schemas/*.schema.json` describe
//! the on-disk shape of a bundle's metadata files. This module embeds
//! them at compile time and exposes **real** Draft 2020-12 validation
//! against them.
//!
//! Historically the CLI's `schema validate` subcommand only
//! presence-checked required fields; regex patterns, enums, and
//! min/max bounds declared in the schema files were never enforced.
//! A reviewer reading the command name would reasonably assume full
//! validation, so the gap was outright misleading. The `validate`
//! function here closes it.
//!
//! All logic lives in the library so that downstream users of the
//! `evidence` crate (and `cargo evidence schema validate` equally)
//! share one implementation — no duplication.

use anyhow::{Context, Result, anyhow};
use serde_json::Value;

/// Which bundle-file schema to validate against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schema {
    Index,
    Env,
    Commands,
    Hashes,
    DeterministicManifest,
}

impl Schema {
    /// Raw JSON-Schema source for this schema.
    pub fn source(self) -> &'static str {
        match self {
            Schema::Index => SCHEMA_INDEX,
            Schema::Env => SCHEMA_ENV,
            Schema::Commands => SCHEMA_COMMANDS,
            Schema::Hashes => SCHEMA_HASHES,
            Schema::DeterministicManifest => SCHEMA_DETERMINISTIC_MANIFEST,
        }
    }

    /// Short textual name ("index", "env", ...). Used for error
    /// messages and `schema show <name>`.
    pub fn name(self) -> &'static str {
        match self {
            Schema::Index => "index",
            Schema::Env => "env",
            Schema::Commands => "commands",
            Schema::Hashes => "hashes",
            Schema::DeterministicManifest => "deterministic-manifest",
        }
    }

    /// Pick the right schema for a filename like `index.json`,
    /// `env.json`, `commands.json`, `*_hashes.json`. Returns `None`
    /// if none of the known shapes match the filename.
    pub fn for_filename(name: &str) -> Option<Self> {
        if name == "index.json" {
            Some(Schema::Index)
        } else if name == "env.json" {
            Some(Schema::Env)
        } else if name == "commands.json" {
            Some(Schema::Commands)
        } else if name == "deterministic-manifest.json" {
            Some(Schema::DeterministicManifest)
        } else if name.contains("hashes") {
            Some(Schema::Hashes)
        } else {
            None
        }
    }

    /// Best-effort content-based detection for files whose name
    /// doesn't fit the usual pattern. Used as a fallback by
    /// `schema validate`.
    pub fn for_content(value: &Value) -> Option<Self> {
        // Both index.json and deterministic-manifest.json carry
        // `schema_version`, but only the index has `bundle_complete`.
        // The manifest has `target_triple` without env-specific
        // fields like `host` or `tools` — that's the cheapest
        // discriminator.
        if value.get("schema_version").is_some() && value.get("bundle_complete").is_some() {
            Some(Schema::Index)
        } else if value.get("schema_version").is_some()
            && value.get("target_triple").is_some()
            && value.get("host").is_none()
        {
            Some(Schema::DeterministicManifest)
        } else if value.get("rustc").is_some() && value.get("cargo").is_some() {
            Some(Schema::Env)
        } else if value.is_array() {
            Some(Schema::Commands)
        } else if value.is_object()
            && value
                .as_object()
                .map(|o| o.values().all(|v| v.is_string()))
                .unwrap_or(false)
        {
            Some(Schema::Hashes)
        } else {
            None
        }
    }
}

// ============================================================================
// Embedded sources
// ============================================================================

const SCHEMA_INDEX: &str = include_str!("../../../schemas/index.schema.json");
const SCHEMA_ENV: &str = include_str!("../../../schemas/env.schema.json");
const SCHEMA_COMMANDS: &str = include_str!("../../../schemas/commands.schema.json");
const SCHEMA_HASHES: &str = include_str!("../../../schemas/hashes.schema.json");
const SCHEMA_DETERMINISTIC_MANIFEST: &str =
    include_str!("../../../schemas/deterministic-manifest.schema.json");

// ============================================================================
// Validation
// ============================================================================

/// Validate `instance` against the given bundle schema.
///
/// Compiles the embedded Draft 2020-12 schema on every call. If the
/// schema is invalid, that's a library bug and bubbles up as `Err`.
/// On a valid schema, the call returns `Ok(())` iff every constraint
/// — required fields, regex patterns, enums, min/max, `oneOf`
/// branches, additional-property rules — is satisfied. On failure the
/// error collects up to the first eight violations with their JSON
/// pointers so the caller can present an actionable diagnosis.
pub fn validate(schema: Schema, instance: &Value) -> Result<()> {
    let schema_value: Value = serde_json::from_str(schema.source())
        .with_context(|| format!("parsing embedded {} schema", schema.name()))?;
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(&schema_value)
        .map_err(|e| anyhow!("compiling {} schema: {}", schema.name(), e))?;

    let errors: Vec<_> = validator.iter_errors(instance).take(8).collect();
    if errors.is_empty() {
        return Ok(());
    }

    let mut msg = format!(
        "{} instance fails {} schema constraint(s):",
        schema.name(),
        errors.len()
    );
    for err in &errors {
        msg.push_str(&format!("\n  at {}: {}", err.instance_path, err));
    }
    Err(anyhow!("{}", msg))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn schema_for_filename_recognizes_known_names() {
        assert_eq!(Schema::for_filename("index.json"), Some(Schema::Index));
        assert_eq!(Schema::for_filename("env.json"), Some(Schema::Env));
        assert_eq!(
            Schema::for_filename("commands.json"),
            Some(Schema::Commands)
        );
        assert_eq!(
            Schema::for_filename("deterministic-manifest.json"),
            Some(Schema::DeterministicManifest)
        );
        assert_eq!(
            Schema::for_filename("inputs_hashes.json"),
            Some(Schema::Hashes)
        );
        assert_eq!(
            Schema::for_filename("outputs_hashes.json"),
            Some(Schema::Hashes)
        );
        assert_eq!(Schema::for_filename("wat.json"), None);
    }

    #[test]
    fn embedded_schemas_compile() {
        // A library bug (bad JSON Schema in the source tree) would
        // surface here first, not on the user's first invocation.
        for s in [
            Schema::Index,
            Schema::Env,
            Schema::Commands,
            Schema::Hashes,
            Schema::DeterministicManifest,
        ] {
            let value: Value = serde_json::from_str(s.source()).expect("parse");
            jsonschema::options()
                .with_draft(jsonschema::Draft::Draft202012)
                .build(&value)
                .unwrap_or_else(|e| panic!("{} schema does not compile: {}", s.name(), e));
        }
    }

    #[test]
    fn hashes_schema_rejects_non_hex() {
        // hashes.schema.json declares a `^[a-f0-9]{64}$` pattern; the
        // old presence-only validator would have accepted "notahash"
        // as long as it was a string. Real validation catches this.
        let instance = json!({ "file.txt": "notahash" });
        let err = validate(Schema::Hashes, &instance).expect_err("should reject");
        let msg = err.to_string();
        assert!(
            msg.contains("fails") && msg.contains("constraint"),
            "error should mention constraint failure, got: {}",
            msg
        );
    }

    #[test]
    fn hashes_schema_accepts_valid_hex() {
        let instance = json!({
            "file.txt": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        });
        validate(Schema::Hashes, &instance).expect("valid hex should pass");
    }

    #[test]
    fn env_schema_rejects_bad_host_os() {
        // env.schema.json constrains `host.os` to the three-variant
        // enum via `oneOf`. Presence check would have passed any
        // string; real validation rejects "freebsd".
        let instance = json!({
            "profile": "dev",
            "rustc": "rustc 1.85.1",
            "cargo": "cargo 1.85.1",
            "git_sha": "abc123",
            "git_branch": "main",
            "git_dirty": false,
            "in_nix_shell": false,
            "tools": {},
            "nav_env": {},
            "host": { "os": "freebsd", "arch": "x86_64" },
            "target_triple": "x86_64-unknown-freebsd",
        });
        validate(Schema::Env, &instance).expect_err("unknown host.os must fail");
    }
}
