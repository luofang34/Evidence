//! CLI surface for `cargo evidence`.
//!
//! Each subcommand lives in its own module here. `main.rs` does only
//! argument parsing and dispatch; everything user-visible (text vs
//! JSON, warnings, exit codes) belongs in these modules.
//!
//! Module layout:
//!
//! - [`args`]     — clap arg types, exit code constants, profile /
//!                  environment detection.
//! - [`output`]   — stdout/stderr rendering primitives shared by every
//!                  subcommand.
//! - [`generate`] — `cargo evidence generate`.
//! - [`verify`]   — `cargo evidence verify`.
//! - [`diff`]     — `cargo evidence diff`.
//! - [`init`]     — `cargo evidence init`.
//! - [`schema`]   — `cargo evidence schema show|validate`.
//! - [`trace`]    — `cargo evidence trace`.
//!
//! Shared boundary-config loaders live at the module root (below)
//! because both `generate` and `trace` need them and a sibling
//! `boundary.rs` module would create a directed dependency edge
//! between two command modules.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use evidence::{BoundaryPolicy, Dal, DalConfig};

pub mod args;
pub mod diff;
pub mod generate;
pub mod init;
pub mod output;
pub mod schema;
pub mod trace;
pub mod verify;

/// Load the list of `scope.in_scope` crate names from a boundary TOML.
///
/// Also validates and logs the `[policy]` table as a side effect so
/// that configuring `forbid_build_rs` / `forbid_proc_macros` is at
/// least observable today — enforcement is a separate follow-up.
pub fn load_in_scope_crates(path: &Path) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("reading boundary config from {:?}", path))?;
    let config: toml::Value = toml::from_str(&content)?;

    if let Some(policy_val) = config.get("policy") {
        if let Ok(policy) = toml::Value::try_into::<BoundaryPolicy>(policy_val.clone()) {
            log::debug!(
                "boundary policy rules enabled: {:?}",
                policy.enabled_rules()
            );
        }
    }

    if let Some(scope) = config.get("scope") {
        if let Some(in_scope) = scope.get("in_scope") {
            if let Some(arr) = in_scope.as_array() {
                return Ok(arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect());
            }
        }
    }
    Ok(Vec::new())
}

/// Load `scope.trace_roots` from a boundary TOML, falling back to
/// `["cert/trace"]` when the file is absent or malformed.
pub fn load_trace_roots(path: &Path) -> Vec<String> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec!["cert/trace".to_string()],
    };
    let config: toml::Value = match toml::from_str(&content) {
        Ok(c) => c,
        Err(_) => return vec!["cert/trace".to_string()],
    };
    if let Some(scope) = config.get("scope") {
        if let Some(roots) = scope.get("trace_roots") {
            if let Some(arr) = roots.as_array() {
                let v: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                if !v.is_empty() {
                    return v;
                }
            }
        }
    }
    vec!["cert/trace".to_string()]
}

/// Load DAL configuration from boundary TOML. Returns the default
/// (DAL-D everywhere) when the file or section is missing.
pub fn load_dal_config(path: &Path) -> DalConfig {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return DalConfig::default(),
    };
    let config: toml::Value = match toml::from_str(&content) {
        Ok(c) => c,
        Err(_) => return DalConfig::default(),
    };
    if let Some(dal) = config.get("dal") {
        if let Ok(dal_config) = toml::Value::try_into::<DalConfig>(dal.clone()) {
            return dal_config;
        }
    }
    DalConfig::default()
}

/// Resolve per-crate DAL map from the config + scope list.
pub fn resolve_dal_map(
    dal_config: &DalConfig,
    in_scope_crates: &[String],
) -> BTreeMap<String, Dal> {
    in_scope_crates
        .iter()
        .map(|name| {
            let dal = dal_config
                .crate_overrides
                .get(name)
                .copied()
                .unwrap_or(dal_config.default_dal);
            (name.clone(), dal)
        })
        .collect()
}
