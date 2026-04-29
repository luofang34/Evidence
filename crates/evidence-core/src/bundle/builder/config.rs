//! `EvidenceBuildConfig` — the input struct passed to
//! [`crate::bundle::EvidenceBuilder::new`]. Pulled out of the
//! parent `builder.rs` so the orchestrator stays under the
//! workspace 500-line limit.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::policy::{BoundaryPolicy, Dal, Profile};

/// Configuration for evidence bundle generation.
#[derive(Debug, Clone)]
pub struct EvidenceBuildConfig {
    /// Output directory for bundles
    pub output_root: PathBuf,
    /// Active profile (type-safe enum, not a free-form string)
    pub profile: Profile,
    /// Crates in scope for certification
    pub in_scope_crates: Vec<String>,
    /// Trace roots to scan
    pub trace_roots: Vec<String>,
    /// Whether to require clean git
    pub require_clean_git: bool,
    /// Whether to fail on dirty git
    pub fail_on_dirty: bool,
    /// Resolved per-crate DAL map (crate_name -> Dal).
    pub dal_map: BTreeMap<String, Dal>,
    /// Boundary policy flags as captured from `cert/boundary.toml`.
    /// Recorded into `index.json` so verify-time can replay the
    /// rules the bundle claimed without consulting the verifier's
    /// local config. Defaults to all-`false` for callers that don't
    /// (yet) plumb the policy through — equivalent to "no rules
    /// claimed", verify skips the recheck.
    #[doc(alias = "policy")]
    pub boundary_policy: BoundaryPolicy,
}
