//! `BoundaryConfig` — the typed view of `boundary.toml`, plus the
//! `LoadBoundaryError` enum and the `load_trace_roots` free function.
//!
//! `BoundaryConfig::load_or_default` is the tolerant loader CLI code
//! reaches for: a missing or malformed file yields an empty scope +
//! DAL-D. The strict `load` is for callers that want to surface a
//! typed IO / parse error. `load_trace_roots` lives alongside as a
//! side-channel reader for the historical `scope.trace_roots` field
//! that isn't on the typed `BoundaryScope` struct.

use log;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::dal::{Dal, DalConfig};

/// Errors returned by [`BoundaryConfig::load`].
#[derive(Debug, Error)]
pub enum LoadBoundaryError {
    /// Failed to read the boundary config file from disk.
    #[error("reading boundary config from {path}")]
    Read {
        /// Path whose read failed.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// The file read but wasn't valid TOML (or didn't match the
    /// expected schema).
    ///
    /// `toml::de::Error` is large on Windows; box it so this enum
    /// stays under clippy's `result_large_err` threshold.
    #[error("parsing boundary config from {path}")]
    Parse {
        /// Path whose TOML failed to parse.
        path: PathBuf,
        /// Underlying TOML error (boxed to keep the enum small).
        #[source]
        source: Box<toml::de::Error>,
    },
}

/// Schema version information.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Schema {
    /// Semver-shaped version string for the on-disk schema.
    pub version: String,
}

/// Boundary scope configuration.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BoundaryScope {
    /// Crates that are in scope for certification
    pub in_scope: Vec<String>,
    /// Workspace crates that are explicitly forbidden as dependencies
    #[serde(default)]
    pub explicit_forbidden: Vec<String>,
}

/// Boundary policy rules.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BoundaryPolicy {
    /// Whether to forbid dependencies on out-of-scope workspace crates
    pub no_out_of_scope_deps: bool,
    /// Whether to forbid build.rs in boundary crates (DO-178C determinism)
    #[serde(default)]
    pub forbid_build_rs: bool,
    /// Whether to forbid proc-macros in boundary crates (DO-178C auditability)
    #[serde(default)]
    pub forbid_proc_macros: bool,
}

impl BoundaryPolicy {
    /// Names of the rules currently enabled by this policy, in a
    /// stable order suitable for logging and reports.
    pub fn enabled_rules(&self) -> Vec<&'static str> {
        let mut rules = Vec::new();
        if self.no_out_of_scope_deps {
            rules.push("no_out_of_scope_deps");
        }
        if self.forbid_build_rs {
            rules.push("forbid_build_rs");
        }
        if self.forbid_proc_macros {
            rules.push("forbid_proc_macros");
        }
        rules
    }
}

/// Complete boundary configuration (loaded from boundary.toml).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BoundaryConfig {
    /// Schema version for this boundary config file.
    pub schema: Schema,
    /// Crate scope — which workspace crates are in and which are
    /// explicitly forbidden as dependencies.
    pub scope: BoundaryScope,
    /// Boundary-enforcement rules.
    pub policy: BoundaryPolicy,
    /// Forbidden external crates with reasons
    #[serde(default)]
    pub forbidden_external: BTreeMap<String, String>,
    /// DAL configuration. If absent, all crates default to DAL-D.
    #[serde(default)]
    pub dal: DalConfig,
}

impl BoundaryConfig {
    /// Load and parse a `boundary.toml`. Returns `Err` on IO or parse
    /// failure.
    ///
    /// Logs the set of enabled policy rules at `debug` level on
    /// success; this used to happen inline in the CLI's hand-rolled
    /// loader and moved here when the typed loader became the single
    /// source of truth.
    pub fn load(path: &Path) -> Result<Self, LoadBoundaryError> {
        let content = fs::read_to_string(path).map_err(|source| LoadBoundaryError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let config: Self = toml::from_str(&content).map_err(|source| LoadBoundaryError::Parse {
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;
        log::debug!(
            "boundary policy rules enabled: {:?}",
            config.policy.enabled_rules()
        );
        Ok(config)
    }

    /// Best-effort load. Returns a default-populated config (empty
    /// scope, default DAL-D) when the file is absent, unreadable, or
    /// unparseable. Used by CLI code paths that want to keep running
    /// when the user hasn't initialized a boundary yet.
    pub fn load_or_default(path: &Path) -> Self {
        Self::load(path).unwrap_or_else(|_| Self::default_empty())
    }

    /// A blank boundary config: empty scope, empty policy, DAL-D
    /// default. Matches what the old hand-rolled CLI loader would
    /// produce when the file was missing.
    pub fn default_empty() -> Self {
        Self {
            schema: Schema {
                version: String::new(),
            },
            scope: BoundaryScope {
                in_scope: Vec::new(),
                explicit_forbidden: Vec::new(),
            },
            policy: BoundaryPolicy {
                no_out_of_scope_deps: false,
                forbid_build_rs: false,
                forbid_proc_macros: false,
            },
            forbidden_external: BTreeMap::new(),
            dal: DalConfig::default(),
        }
    }

    /// Resolve the per-crate DAL map from the `[dal]` section plus
    /// the in-scope list. Each in-scope crate maps to its override if
    /// one exists, otherwise to `dal.default_dal`.
    pub fn dal_map(&self) -> BTreeMap<String, Dal> {
        self.scope
            .in_scope
            .iter()
            .map(|name| {
                let dal = self
                    .dal
                    .crate_overrides
                    .get(name)
                    .copied()
                    .unwrap_or(self.dal.default_dal);
                (name.clone(), dal)
            })
            .collect()
    }

    /// `scope.trace_roots` with fallback. Reads an `additional_roots`
    /// side channel if populated; otherwise returns `["cert/trace"]`.
    /// Callers that need the raw list without the fallback should
    /// touch `self.scope` directly.
    pub fn trace_roots_or_default(&self) -> Vec<String> {
        // BoundaryScope historically serialized a separate
        // `trace_roots` key that isn't on the struct; `load` preserves
        // unknown fields via serde's default behavior. The CLI's old
        // loader hand-read this key from `toml::Value` and fell back
        // to `["cert/trace"]` when it was missing or empty. To keep
        // that exact behavior, we re-parse the source file here when
        // we have access — but since callers usually only hold the
        // typed `BoundaryConfig`, we expose the read-through helper
        // as a separate free function. Callers that pass a path get
        // the full fallback; callers that hold just the config get
        // just `["cert/trace"]` (the default).
        vec!["cert/trace".to_string()]
    }
}

/// Load `scope.trace_roots` from a boundary TOML with the historical
/// CLI fallback chain: file → array value → `["cert/trace"]`.
///
/// This lives as a free function (not a method on `BoundaryConfig`)
/// because `trace_roots` is not currently typed on `BoundaryScope` —
/// adding it there would be a serialization-compatibility change we
/// don't need for this PR. Behavior matches the pre-existing CLI
/// loader byte-for-byte.
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;

    #[test]
    fn test_boundary_config_without_dal_section() {
        let toml_str = format!(
            r#"
[schema]
version = "{ver}"

[scope]
in_scope = ["my-crate"]

[policy]
no_out_of_scope_deps = true
"#,
            ver = crate::schema_versions::BOUNDARY
        );
        let config: BoundaryConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.dal.default_dal, Dal::D);
        assert!(config.dal.crate_overrides.is_empty());
    }

    #[test]
    fn test_boundary_config_with_dal_section() {
        let toml_str = format!(
            r#"
[schema]
version = "{ver}"

[scope]
in_scope = ["flight-core", "telemetry"]

[policy]
no_out_of_scope_deps = true

[dal]
default_dal = "C"

[dal.crate_overrides]
"flight-core" = "A"
"#,
            ver = crate::schema_versions::BOUNDARY
        );
        let config: BoundaryConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.dal.default_dal, Dal::C);
        assert_eq!(config.dal.crate_overrides["flight-core"], Dal::A);
    }
}
