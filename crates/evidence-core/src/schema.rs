//! JSON Schemas and validation.
//!
//! The JSON Schema files under `schemas/*.schema.json` describe
//! either (a) the on-disk shape of a bundle's metadata files or
//! (b) the streaming wire format `cargo-evidence` emits on stdout
//! in `--format=jsonl` mode. This module embeds every schema at
//! compile time and exposes **real** Draft 2020-12 validation
//! against the bundle-file group.
//!
//! The wire-format [`Schema::Diagnostic`] variant exists so
//! `cargo evidence schema show diagnostic` can print the embedded
//! source, but it is intentionally excluded from
//! [`Schema::for_filename`] and [`Schema::for_content`] — diagnostic
//! lines are streamed per-event, not committed to disk, so
//! `cargo evidence schema validate <file>` has no file to match.
//!
//! All logic lives in the library so that downstream users of the
//! `evidence` crate (and `cargo evidence schema validate` equally)
//! share one implementation — no duplication.

use serde_json::Value;
use thiserror::Error;

use crate::diagnostic::{DiagnosticCode, Severity};

/// Errors returned by [`validate`].
#[derive(Debug, Error)]
pub enum SchemaError {
    /// The embedded JSON-Schema source for this enum variant failed to
    /// parse. Always a library bug — the `embedded_schemas_compile`
    /// test asserts every bundled schema parses.
    #[error("parsing embedded {schema} schema")]
    ParseSchema {
        /// Name of the schema that failed to parse.
        schema: &'static str,
        /// Underlying serde_json error.
        #[source]
        source: serde_json::Error,
    },
    /// The embedded schema parsed as JSON but isn't a valid Draft
    /// 2020-12 JSON Schema. Also a library bug.
    #[error("compiling {schema} schema: {reason}")]
    CompileSchema {
        /// Name of the schema that failed to compile.
        schema: &'static str,
        /// Human-readable reason from the `jsonschema` crate.
        reason: String,
    },
    /// The instance violates one or more schema constraints. The
    /// string aggregates up to the first eight violations with their
    /// JSON pointers.
    #[error("{0}")]
    InstanceInvalid(String),
}

impl DiagnosticCode for SchemaError {
    fn code(&self) -> &'static str {
        match self {
            SchemaError::ParseSchema { .. } => "SCHEMA_PARSE_FAILED",
            SchemaError::CompileSchema { .. } => "SCHEMA_COMPILE_FAILED",
            SchemaError::InstanceInvalid(_) => "SCHEMA_INSTANCE_INVALID",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }
}

/// Which bundle-file schema to validate against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schema {
    /// `index.json` — the per-bundle manifest (metadata layer).
    Index,
    /// `env.json` — captured build-environment fingerprint.
    Env,
    /// `commands.json` — the per-bundle command-execution log.
    Commands,
    /// `inputs_hashes.json` / `outputs_hashes.json` — path → hash maps.
    Hashes,
    /// `deterministic-manifest.json` — cross-host reproducibility contract.
    DeterministicManifest,
    /// `cargo_metadata.json` — deterministic projection of
    /// `cargo metadata` written when boundary policy enables
    /// `forbid_build_rs` or `forbid_proc_macros`. Verify-time
    /// recheck reads this artifact (LLR-072).
    CargoMetadata,
    /// `diagnostic.schema.json` — wire-format schema for the `--format=jsonl`
    /// streaming output. Not a bundle file; excluded from `for_filename`
    /// and `for_content` on purpose.
    Diagnostic,
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
            Schema::CargoMetadata => SCHEMA_CARGO_METADATA,
            Schema::Diagnostic => SCHEMA_DIAGNOSTIC,
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
            Schema::CargoMetadata => "cargo-metadata",
            Schema::Diagnostic => "diagnostic",
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
        } else if name == "cargo_metadata.json" {
            Some(Schema::CargoMetadata)
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

const SCHEMA_INDEX: &str = include_str!("../schemas/index.schema.json");
const SCHEMA_ENV: &str = include_str!("../schemas/env.schema.json");
const SCHEMA_COMMANDS: &str = include_str!("../schemas/commands.schema.json");
const SCHEMA_HASHES: &str = include_str!("../schemas/hashes.schema.json");
const SCHEMA_DETERMINISTIC_MANIFEST: &str =
    include_str!("../schemas/deterministic-manifest.schema.json");
const SCHEMA_CARGO_METADATA: &str = include_str!("../schemas/cargo_metadata.schema.json");
const SCHEMA_DIAGNOSTIC: &str = include_str!("../schemas/diagnostic.schema.json");

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
pub fn validate(schema: Schema, instance: &Value) -> Result<(), SchemaError> {
    let schema_value: Value =
        serde_json::from_str(schema.source()).map_err(|source| SchemaError::ParseSchema {
            schema: schema.name(),
            source,
        })?;
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(&schema_value)
        .map_err(|e| SchemaError::CompileSchema {
            schema: schema.name(),
            reason: e.to_string(),
        })?;

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
        msg.push_str(&format!("\n  at {}: {}", err.instance_path(), err));
    }
    Err(SchemaError::InstanceInvalid(msg))
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
            Schema::CargoMetadata,
            Schema::Diagnostic,
        ] {
            let value: Value = serde_json::from_str(s.source()).expect("parse");
            jsonschema::options()
                .with_draft(jsonschema::Draft::Draft202012)
                .build(&value)
                .unwrap_or_else(|e| panic!("{} schema does not compile: {}", s.name(), e));
        }
    }

    /// The diagnostic schema is deliberately excluded from
    /// [`Schema::for_filename`] and [`Schema::for_content`] — it
    /// describes a wire-format stream, not a bundle file. Pin the
    /// exclusion here so a future "convenience" PR that tries to add
    /// `diagnostic.json` recognition fails this test and forces the
    /// reviewer to re-read the contract.
    #[test]
    fn diagnostic_schema_is_not_a_bundle_file_target() {
        // Filename heuristic must not recognize diagnostic names.
        assert_eq!(Schema::for_filename("diagnostic.json"), None);
        assert_eq!(Schema::for_filename("diagnostic.schema.json"), None);

        // Content heuristic must not route a realistic diagnostic
        // event (carries a nested `location` object, so the
        // "all-string values" Hashes shape doesn't swallow it) to
        // any bundle-file schema. A hypothetical `schema validate
        // some-diag.json` should fall through to "unknown type",
        // not silently validate against the wrong schema.
        let instance = json!({
            "code": "TRACE_UID_MISSING",
            "severity": "error",
            "message": "HLR-001 missing UID",
            "location": { "file": "cert/trace/hlr.toml", "line": 12 }
        });
        assert_eq!(Schema::for_content(&instance), None);
    }

    #[test]
    fn hashes_schema_rejects_non_hex() {
        // Pins the `^[a-f0-9]{64}$` pattern from hashes.schema.json.
        // A presence-only validator accepts "notahash" as long as it's
        // a string; the pattern constraint must fire to catch typos
        // and truncated digests before they're sealed into SHA256SUMS.
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
