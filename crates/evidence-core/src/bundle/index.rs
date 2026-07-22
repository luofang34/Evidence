//! `EvidenceIndex` — the struct mirror of `index.json`, the metadata layer.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::policy::{Profile, ResolutionPolicy};

use super::command_failure::ToolCommandFailure;
use super::test_summary::TestSummary;

/// Default for `EvidenceIndex::engine_build_source` when deserializing
/// a legacy bundle that predates the field.
pub(super) fn default_engine_build_source() -> String {
    "unknown".to_string()
}

/// Default for `EvidenceIndex::resolution_policy` when deserializing
/// a legacy bundle that predates the field: `locked_offline`, the
/// safe default (LLR-142). A bundle that never recorded the field
/// could not have been produced under the development online opt-in —
/// that path did not exist — so the default is a true statement about
/// the bundle, not a guess.
pub(super) fn default_resolution_policy() -> ResolutionPolicy {
    ResolutionPolicy::LockedOffline
}

/// Contains metadata about the evidence bundle including schema versions,
/// timestamps, git state, and file references.
///
/// **Determinism design:** `index.json` is part of the metadata layer and is
/// EXCLUDED from SHA256SUMS. The `content_hash` field records the SHA-256 of
/// the SHA256SUMS file itself, which covers only the deterministic content
/// layer. Two runs on the same commit produce identical `content_hash` values
/// even though `timestamp_rfc3339` differs.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EvidenceIndex {
    /// Evidence schema version
    pub schema_version: String,
    /// Boundary config schema version
    pub boundary_schema_version: String,
    /// Trace schema version
    pub trace_schema_version: String,
    /// Active profile.
    ///
    /// Typed [`Profile`] instead of `String` so a typo'd `"deb"`
    /// cannot round-trip through serde at this boundary. `Profile`
    /// serializes / deserializes as `"dev"` / `"cert"` / `"record"`
    /// via `#[serde(rename_all = "lowercase")]`, matching the on-
    /// disk schema byte-for-byte.
    pub profile: Profile,
    /// Bundle creation timestamp (RFC3339)
    pub timestamp_rfc3339: String,
    /// Git commit SHA
    pub git_sha: String,
    /// Git branch name
    pub git_branch: String,
    /// Whether git was dirty at bundle time
    pub git_dirty: bool,
    /// Evidence engine crate version
    pub engine_crate_version: String,
    /// Evidence engine commit SHA or release-version placeholder.
    ///
    /// When `engine_build_source == "git"` this is a 40-char hex SHA
    /// captured either by `build.rs`' `git rev-parse HEAD` or by an
    /// explicit `EVIDENCE_ENGINE_GIT_SHA` override at build time (CI
    /// publish path: `${GITHUB_SHA}`). When
    /// `engine_build_source == "release"` this is `release-v<version>`,
    /// embedded when no git metadata was reachable — typical of
    /// crates.io tarball builds. `"unknown"` only appears in legacy
    /// bundles written before `engine_build_source` existed.
    pub engine_git_sha: String,
    /// Origin of `engine_git_sha`: `"git"` | `"release"` | `"unknown"`.
    ///
    /// Every `EvidenceBuilder` populates this to `"git"` or `"release"`;
    /// `#[serde(default)]` returns `"unknown"` when deserializing a
    /// legacy bundle that predates the field so older fixtures still
    /// load. `verify` cross-checks the pair (source, sha) to catch a
    /// build that e.g. claims `"git"` but embeds a non-40-hex value.
    #[serde(default = "default_engine_build_source")]
    pub engine_build_source: String,
    /// Path to inputs hashes file
    pub inputs_hashes_file: String,
    /// Path to outputs hashes file
    pub outputs_hashes_file: String,
    /// Path to commands file
    pub commands_file: String,
    /// Path to environment fingerprint file
    pub env_fingerprint_file: String,
    /// Trace roots that were scanned
    pub trace_roots: Vec<String>,
    /// Generated trace output files
    pub trace_outputs: Vec<String>,
    /// Whether the bundle is complete
    pub bundle_complete: bool,
    /// SHA-256 of the SHA256SUMS file.
    ///
    /// Covers every byte in the content layer (all files except
    /// `index.json` and `SHA256SUMS` itself, plus `BUNDLE.sig` when
    /// present). Reproducible across runs **on the same host** for
    /// the same commit and inputs; differs across hosts because
    /// `env.json` records host identity (host.os, libc, tools). For
    /// same-recipe identity see `recipe_hash`.
    pub content_hash: String,
    /// SHA-256 of `deterministic-manifest.json` — the recipe
    /// identity hash.
    ///
    /// The committed manifest is the canonical recipe projection
    /// (toolchain, target triple, profile, features, locked
    /// dependency graph, command recipe, source-input digests,
    /// resolution policy). Two bundles sharing this hash declare
    /// the SAME recipe; the hash says nothing about whether their
    /// outputs reproduce — reproduced-output equality is the
    /// `verify::reproduction` comparison's claim, and cross-host
    /// parity is the six-field toolchain projection the CI
    /// determinism gates compare (the full recipe binds
    /// host-defining fields like `target_triple`).
    ///
    /// Serialized as `recipe_hash`. `#[serde(alias)]` accepts the
    /// pre-rename `deterministic_hash` key when reading bundles
    /// written before the rename, so a legacy `index.json` still
    /// deserializes; every new bundle emits only `recipe_hash`.
    #[serde(alias = "deterministic_hash")]
    pub recipe_hash: String,
    /// Parsed test results summary, if cargo test was executed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_summary: Option<TestSummary>,
    /// Captured-subprocess failures from the generate pipeline
    /// (cargo test exiting non-zero, cargo check failing, etc.).
    /// Non-empty ⇒ [`Self::bundle_complete`] is `false`; verify
    /// refuses cert/record bundles carrying recorded failures.
    /// `#[serde(default)]` lets older bundles deserialize as
    /// an empty Vec — they were `bundle_complete: true` by
    /// construction and will continue to validate as such.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_command_failures: Vec<ToolCommandFailure>,
    /// Per-crate DAL assignments. Key is crate name, value is DAL level string.
    /// Empty map for bundles generated before DAL support was added.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dal_map: BTreeMap<String, String>,
    /// Boundary policy flags as captured from `cert/boundary.toml`
    /// at generate time. Verify-time consults this to know which
    /// rules the bundle claimed, so the recheck doesn't depend on
    /// a verifier-local `boundary.toml`. Defaults to all-`false`
    /// (`#[serde(default)]`) so bundles generated before this field
    /// existed still deserialize — they're treated as "no boundary
    /// policy claim made", i.e. the verify-time recheck is skipped
    /// for legacy bundles.
    #[serde(default, skip_serializing_if = "is_default_boundary_policy")]
    pub boundary_policy: crate::policy::BoundaryPolicy,
    /// Dependency-resolution policy the bundle was generated under
    /// (LLR-142). Always serialized — the bundle records its policy
    /// explicitly. `#[serde(default)]` resolves a legacy bundle that
    /// predates the field to `locked_offline`, the safe default.
    /// Verify rejects a bundle pairing `online_opt_in` with a
    /// cert/record profile (`VERIFY_ONLINE_RESOLUTION`).
    #[serde(default = "default_resolution_policy")]
    pub resolution_policy: ResolutionPolicy,
}

fn is_default_boundary_policy(p: &crate::policy::BoundaryPolicy) -> bool {
    !p.no_out_of_scope_deps && !p.forbid_build_rs && !p.forbid_proc_macros
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
    fn test_evidence_index_fields() {
        let idx = EvidenceIndex {
            schema_version: crate::schema_versions::INDEX.to_string(),
            boundary_schema_version: crate::schema_versions::BOUNDARY.to_string(),
            trace_schema_version: crate::schema_versions::TRACE.to_string(),
            profile: Profile::Cert,
            timestamp_rfc3339: "2024-01-01T00:00:00Z".to_string(),
            git_sha: "abc123".to_string(),
            git_branch: "main".to_string(),
            git_dirty: false,
            engine_crate_version: "0.1.0".to_string(),
            engine_git_sha: "abc123".to_string(),
            engine_build_source: "git".to_string(),
            inputs_hashes_file: "inputs_hashes.json".to_string(),
            outputs_hashes_file: "outputs_hashes.json".to_string(),
            commands_file: "commands.json".to_string(),
            env_fingerprint_file: "env.json".to_string(),
            trace_roots: vec!["cert/trace".to_string()],
            trace_outputs: vec!["trace/matrix.md".to_string()],
            bundle_complete: true,
            content_hash: "deadbeef".repeat(8),
            recipe_hash: "cafebabe".repeat(8),
            test_summary: None,
            tool_command_failures: Vec::new(),
            dal_map: BTreeMap::new(),
            boundary_policy: crate::policy::BoundaryPolicy::default(),
            resolution_policy: ResolutionPolicy::LockedOffline,
        };
        assert!(idx.bundle_complete);
        assert_eq!(idx.profile, Profile::Cert);
        assert_eq!(idx.content_hash.len(), 64);
        assert_eq!(idx.resolution_policy, ResolutionPolicy::LockedOffline);
    }

    /// TEST-159 (a): a bundle written before `resolution_policy`
    /// existed deserializes with the safe `locked_offline` default,
    /// and the field serializes under its snake_case wire label.
    #[test]
    fn resolution_policy_defaults_to_locked_offline_for_legacy_bundles() {
        let legacy = serde_json::json!({
            "schema_version": "1.0.0",
            "boundary_schema_version": "1.0.0",
            "trace_schema_version": "1.0.0",
            "profile": "dev",
            "timestamp_rfc3339": "2024-01-01T00:00:00Z",
            "git_sha": "abc123",
            "git_branch": "main",
            "git_dirty": false,
            "engine_crate_version": "0.1.0",
            "engine_git_sha": "abc123",
            "inputs_hashes_file": "inputs_hashes.json",
            "outputs_hashes_file": "outputs_hashes.json",
            "commands_file": "commands.json",
            "env_fingerprint_file": "env.json",
            "trace_roots": [],
            "trace_outputs": [],
            "bundle_complete": true,
            "content_hash": "deadbeef".repeat(8),
            "deterministic_hash": "cafebabe".repeat(8),
        });
        let idx: EvidenceIndex = serde_json::from_value(legacy).expect("legacy index parses");
        assert_eq!(idx.resolution_policy, ResolutionPolicy::LockedOffline);

        let online: EvidenceIndex = serde_json::from_value(serde_json::json!({
            "schema_version": "1.0.0",
            "boundary_schema_version": "1.0.0",
            "trace_schema_version": "1.0.0",
            "profile": "dev",
            "timestamp_rfc3339": "2024-01-01T00:00:00Z",
            "git_sha": "abc123",
            "git_branch": "main",
            "git_dirty": false,
            "engine_crate_version": "0.1.0",
            "engine_git_sha": "abc123",
            "inputs_hashes_file": "inputs_hashes.json",
            "outputs_hashes_file": "outputs_hashes.json",
            "commands_file": "commands.json",
            "env_fingerprint_file": "env.json",
            "trace_roots": [],
            "trace_outputs": [],
            "bundle_complete": true,
            "content_hash": "deadbeef".repeat(8),
            "deterministic_hash": "cafebabe".repeat(8),
            "resolution_policy": "online_opt_in",
        }))
        .expect("online_opt_in wire label parses");
        assert_eq!(online.resolution_policy, ResolutionPolicy::OnlineOptIn);

        // Round-trip: the typed enum serializes back to the wire label.
        let rendered = serde_json::to_value(&online).expect("serialize");
        assert_eq!(
            rendered["resolution_policy"],
            serde_json::Value::String("online_opt_in".to_string())
        );
    }

    /// A freshly written index serializes the recipe identity hash
    /// under `recipe_hash` — never the legacy key.
    #[test]
    fn recipe_hash_serializes_under_new_name() {
        let idx = EvidenceIndex {
            schema_version: crate::schema_versions::INDEX.to_string(),
            boundary_schema_version: crate::schema_versions::BOUNDARY.to_string(),
            trace_schema_version: crate::schema_versions::TRACE.to_string(),
            profile: Profile::Dev,
            timestamp_rfc3339: "2024-01-01T00:00:00Z".to_string(),
            git_sha: "abc123".to_string(),
            git_branch: "main".to_string(),
            git_dirty: false,
            engine_crate_version: "0.1.0".to_string(),
            engine_git_sha: "abc123".to_string(),
            engine_build_source: "git".to_string(),
            inputs_hashes_file: "inputs_hashes.json".to_string(),
            outputs_hashes_file: "outputs_hashes.json".to_string(),
            commands_file: "commands.json".to_string(),
            env_fingerprint_file: "env.json".to_string(),
            trace_roots: vec![],
            trace_outputs: vec![],
            bundle_complete: true,
            content_hash: "deadbeef".repeat(8),
            recipe_hash: "cafebabe".repeat(8),
            test_summary: None,
            tool_command_failures: Vec::new(),
            dal_map: BTreeMap::new(),
            boundary_policy: crate::policy::BoundaryPolicy::default(),
            resolution_policy: ResolutionPolicy::LockedOffline,
        };
        let rendered = serde_json::to_value(&idx).expect("serialize");
        assert_eq!(
            rendered["recipe_hash"],
            serde_json::Value::String("cafebabe".repeat(8))
        );
        assert!(
            rendered.get("deterministic_hash").is_none(),
            "the legacy key must never be emitted: {rendered}"
        );
    }

    /// A legacy `index.json` carrying the pre-rename
    /// `deterministic_hash` key still deserializes — the value lands
    /// in `recipe_hash` via the serde alias — and re-serializes
    /// under the new name only.
    #[test]
    fn legacy_deterministic_hash_deserializes_via_alias() {
        let legacy = serde_json::json!({
            "schema_version": "1.0.0",
            "boundary_schema_version": "1.0.0",
            "trace_schema_version": "1.0.0",
            "profile": "dev",
            "timestamp_rfc3339": "2024-01-01T00:00:00Z",
            "git_sha": "abc123",
            "git_branch": "main",
            "git_dirty": false,
            "engine_crate_version": "0.1.0",
            "engine_git_sha": "abc123",
            "inputs_hashes_file": "inputs_hashes.json",
            "outputs_hashes_file": "outputs_hashes.json",
            "commands_file": "commands.json",
            "env_fingerprint_file": "env.json",
            "trace_roots": [],
            "trace_outputs": [],
            "bundle_complete": true,
            "content_hash": "deadbeef".repeat(8),
            "deterministic_hash": "cafebabe".repeat(8),
        });
        let idx: EvidenceIndex = serde_json::from_value(legacy).expect("legacy index parses");
        assert_eq!(idx.recipe_hash, "cafebabe".repeat(8));

        let rendered = serde_json::to_value(&idx).expect("serialize");
        assert_eq!(
            rendered["recipe_hash"],
            serde_json::Value::String("cafebabe".repeat(8))
        );
        assert!(rendered.get("deterministic_hash").is_none());
    }
}
