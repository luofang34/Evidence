//! `EvidenceIndex` — the struct mirror of `index.json`, the metadata layer.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::policy::Profile;

use super::test_summary::TestSummary;

/// Default for `EvidenceIndex::engine_build_source` when deserializing
/// a legacy bundle that predates the field.
pub(super) fn default_engine_build_source() -> String {
    "unknown".to_string()
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
    /// cross-host equality see `deterministic_hash`.
    pub content_hash: String,
    /// SHA-256 of `deterministic-manifest.json`.
    ///
    /// The committed manifest is a projection of `env.json` down to
    /// fields that are cross-host stable (toolchain, target triple,
    /// source identity). Bundles built from the same commit with the
    /// same `rust-toolchain.toml` on Linux, macOS, and Windows share
    /// this hash. This is the tool's cross-host reproducibility
    /// contract, running alongside the full-content `content_hash`
    /// which stays in `SHA256SUMS` for audit-chain integrity.
    pub deterministic_hash: String,
    /// Parsed test results summary, if cargo test was executed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_summary: Option<TestSummary>,
    /// Per-crate DAL assignments. Key is crate name, value is DAL level string.
    /// Empty map for bundles generated before DAL support was added.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dal_map: BTreeMap<String, String>,
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
            deterministic_hash: "cafebabe".repeat(8),
            test_summary: None,
            dal_map: BTreeMap::new(),
        };
        assert!(idx.bundle_complete);
        assert_eq!(idx.profile, Profile::Cert);
        assert_eq!(idx.content_hash.len(), 64);
    }
}
