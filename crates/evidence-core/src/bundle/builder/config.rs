//! `EvidenceBuildConfig` — the input struct passed to
//! [`crate::bundle::EvidenceBuilder::new`]. Pulled out of the
//! parent `builder.rs` so the orchestrator stays under the
//! workspace 500-line limit.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::policy::{AssuranceLevel, BoundaryPolicy, Profile, ResolutionPolicy};

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
    /// Resolved per-crate assurance-level map (crate_name -> level).
    /// Serialized into `index.json` `dal_map` via the level's
    /// `Display` label (`A`–`D`, `QM`, `unclassified`).
    pub dal_map: BTreeMap<String, AssuranceLevel>,
    /// Boundary policy flags as captured from `cert/boundary.toml`.
    /// Recorded into `index.json` so verify-time can replay the
    /// rules the bundle claimed without consulting the verifier's
    /// local config. Defaults to all-`false` for callers that don't
    /// (yet) plumb the policy through — equivalent to "no rules
    /// claimed", verify skips the recheck.
    #[doc(alias = "policy")]
    pub boundary_policy: BoundaryPolicy,
    /// Dependency-resolution policy every cargo subprocess in the
    /// pipeline runs under (LLR-139). Recorded into `index.json` as
    /// `resolution_policy` so verify can reject an online-resolution
    /// cert/record bundle (LLR-142). Library callers that do not
    /// plumb a policy should pass [`ResolutionPolicy::LOCKED_OFFLINE`];
    /// the online path exists only through the CLI's development
    /// `--online` opt-in.
    pub resolution_policy: ResolutionPolicy,
}
