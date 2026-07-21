//! `BoundaryConfig` — the typed view of `boundary.toml`, plus the
//! `LoadBoundaryError` enum and the `load_trace_roots` free function.
//!
//! `BoundaryConfig::load_or_default` is the tolerant loader CLI code
//! reaches for: a missing or malformed file yields an empty scope and
//! no claimed assurance level (`dal: None`). The strict `load` is for
//! callers that want to surface a typed IO / parse error.
//! `load_trace_roots` lives alongside as a side-channel reader for
//! the historical `scope.trace_roots` field that isn't on the typed
//! `BoundaryScope` struct.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::assurance::{AssuranceLevel, AssuranceSelection, StandardEdition};
use super::dal::DalConfig;
use crate::diagnostic::{DiagnosticCode, Location, Severity};

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

impl DiagnosticCode for LoadBoundaryError {
    fn code(&self) -> &'static str {
        match self {
            LoadBoundaryError::Read { .. } => "BOUNDARY_CONFIG_READ_FAILED",
            LoadBoundaryError::Parse { .. } => "BOUNDARY_CONFIG_PARSE_FAILED",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn location(&self) -> Option<Location> {
        let path = match self {
            LoadBoundaryError::Read { path, .. } | LoadBoundaryError::Parse { path, .. } => {
                path.clone()
            }
        };
        Some(Location {
            file: Some(path),
            ..Location::default()
        })
    }
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

/// Explicitly declared controlled inputs that the source baseline must
/// capture even when they are not git-tracked — e.g. generated code an
/// in-scope crate compiles against. Each entry is a workspace-relative
/// path; generation fails closed if a declared input is absent on disk,
/// because a controlled input that cannot be hashed cannot back the
/// evidence result.
#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct BoundaryInputs {
    /// Workspace-relative paths of required controlled inputs.
    #[serde(default)]
    pub required: Vec<String>,
}

/// Boundary policy rules.
#[derive(Debug, Default, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct BoundaryPolicy {
    /// Whether to forbid dependencies on out-of-scope workspace crates
    #[serde(default)]
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

    /// Every rule the user has *enabled* in `boundary.toml` but whose
    /// enforcement is not wired up in this release.
    ///
    /// The whole point of this method is to turn a silent
    /// false-confidence bug into a loud preflight error. The flags
    /// on [`BoundaryPolicy`] are declarative config: a user writes
    /// `forbid_build_rs = true` and expects the tool to reject a
    /// bundle containing an in-scope crate with a `build.rs`.
    /// Today nothing checks any of them — the bundle is produced,
    /// verified, and stamped cert-ready regardless. A certifier
    /// reading such a bundle has no way to know the flag was
    /// inert, because the `BoundaryPolicy` struct is not recorded
    /// anywhere in the bundle.
    ///
    /// The CLI's generate preflight calls this and refuses to
    /// produce a bundle when the returned list is non-empty. As
    /// each rule's real enforcement lands, that rule disappears
    /// from this list and the preflight stops refusing it. This is
    /// the single source of truth — do not hard-code the same names
    /// at call sites.
    ///
    /// Order matches [`enabled_rules`](Self::enabled_rules) for
    /// stable diagnostics.
    pub fn unimplemented_enabled_rules(&self) -> Vec<&'static str> {
        // When a rule gets real enforcement, delete its branch here
        // and add a test covering the enforcement. `enabled_rules`
        // stays untouched.
        //
        // `no_out_of_scope_deps` — enforced in
        // `evidence_core::boundary_check::check_no_out_of_scope_deps`.
        // `forbid_build_rs` — enforced in
        // `evidence_core::boundary_check::check_no_build_rs`.
        // `forbid_proc_macros` — enforced in
        // `evidence_core::boundary_check::check_no_proc_macros`.
        Vec::new()
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
    /// Explicitly declared required controlled inputs (see
    /// [`BoundaryInputs`]). Defaults to empty.
    #[serde(default)]
    pub inputs: BoundaryInputs,
    /// DAL configuration. Absent ⇒ no assurance level is claimed:
    /// crates resolve to `unclassified` and cert/record evaluation
    /// fails closed (LLR-109).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dal: Option<DalConfig>,
}

impl BoundaryConfig {
    /// Load and parse a `boundary.toml`. Returns `Err` on IO or parse
    /// failure.
    ///
    /// Logs the set of enabled policy rules at `debug` level on
    /// success; the typed loader is the single source of truth for
    /// that log line.
    pub fn load(path: &Path) -> Result<Self, LoadBoundaryError> {
        let content = fs::read_to_string(path).map_err(|source| LoadBoundaryError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let config: Self = toml::from_str(&content).map_err(|source| LoadBoundaryError::Parse {
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;
        tracing::debug!(
            "boundary policy rules enabled: {:?}",
            config.policy.enabled_rules()
        );
        Ok(config)
    }

    /// Best-effort load. Returns a default-populated config (empty
    /// scope, no claimed assurance level) when the file is absent,
    /// unreadable, or unparseable. Used by CLI code paths that want
    /// to keep running when the user hasn't initialized a boundary
    /// yet.
    pub fn load_or_default(path: &Path) -> Self {
        Self::load(path).unwrap_or_else(|_| Self::default_empty())
    }

    /// A blank boundary config: empty scope, empty policy, no `[dal]`
    /// section. This is the shape `load_or_default` returns when the
    /// file is absent or unparseable — nothing is claimed, so a
    /// missing config cannot silently become a DAL claim; development
    /// surfaces name the result `unclassified` (LLR-109).
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
            inputs: BoundaryInputs::default(),
            dal: None,
        }
    }

    /// Resolve the per-crate assurance-level map from the `[dal]`
    /// section plus the in-scope list. Each in-scope crate maps to
    /// its override if one exists, otherwise to `dal.default_dal`;
    /// a crate with neither is `unclassified` — an explicit
    /// non-claim, never a silent DAL-D (LLR-109).
    pub fn dal_map(&self) -> BTreeMap<String, AssuranceLevel> {
        self.scope
            .in_scope
            .iter()
            .map(|name| {
                let claimed = self
                    .dal
                    .as_ref()
                    .and_then(|dal| dal.crate_overrides.get(name).copied().or(dal.default_dal));
                let level = claimed.map_or(AssuranceLevel::Unclassified, AssuranceLevel::from_dal);
                (name.clone(), level)
            })
            .collect()
    }

    /// The explicit assurance selection this config claims, if any
    /// (LLR-109). `Some` only when all of these hold:
    ///
    /// - a `[dal]` section is present,
    /// - it declares an explicit `default_dal`,
    /// - `scope.in_scope` is non-empty.
    ///
    /// The selection's level is the highest rigor in scope (max over
    /// the per-crate map). Cert/record evaluation requires `Some`
    /// and fails closed otherwise; development surfaces use
    /// [`AssuranceSelection::unclassified`] on `None`.
    pub fn assurance_selection(&self) -> Option<AssuranceSelection> {
        let dal = self.dal.as_ref()?;
        dal.default_dal?;
        if self.scope.in_scope.is_empty() {
            return None;
        }
        // `default_dal` is Some, so every in-scope crate resolves to
        // a DAL level and the map is non-empty.
        let level = self.dal_map().values().copied().max()?;
        Some(AssuranceSelection {
            standard: StandardEdition::Do178c,
            level,
        })
    }

    /// `scope.trace_roots` with fallback. Reads an `additional_roots`
    /// side channel if populated; otherwise returns `["cert/trace"]`.
    /// Callers that need the raw list without the fallback should
    /// touch `self.scope` directly.
    pub fn trace_roots_or_default(&self) -> Vec<String> {
        // `trace_roots` lives in the source file as a side-channel
        // key, not on the typed struct; `load` preserves unknown
        // fields via serde's default behavior. A caller holding a
        // path re-parses the source file for the full fallback
        // (missing or empty key → `["cert/trace"]`); a caller
        // holding just the typed `BoundaryConfig` gets
        // `["cert/trace"]` (the default).
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
// Tests live in the sibling `boundary/tests.rs` via `#[path]` so
// this file stays under the 500-line workspace limit.
#[cfg(test)]
#[path = "boundary/tests.rs"]
mod tests;
