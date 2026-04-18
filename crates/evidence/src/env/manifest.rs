//! `DeterministicManifest` — the cross-host reproducibility contract.

use serde::{Deserialize, Serialize};

use crate::policy::Profile;

/// Cross-host reproducibility contract.
///
/// A committed, SHA-256-hashed projection of `EnvFingerprint` that
/// contains only fields which are stable across hosts sharing the
/// same commit and toolchain. Bundles built from the same commit
/// with the same `rust-toolchain.toml` on Linux, macOS, and
/// Windows produce byte-identical `DeterministicManifest` JSON —
/// and therefore a shared `deterministic_hash` in `index.json`.
///
/// Scope note: **target_triple is intentionally excluded**. See the
/// `deterministic_manifest()` method on `EnvFingerprint` for the
/// full rationale; short version is that default-target native builds
/// on the three CI hosts produce three different targets, and target
/// parity requires cross-compile plumbing that's out of scope today.
/// `env.target_triple` still flows into `content_hash`.
///
/// This runs alongside, not in place of, `content_hash`. Full
/// content (including host identity and target triple) stays in
/// `SHA256SUMS`, so the integrity chain is unbroken and
/// `sha256sum -c` still attests to every recorded byte.
///
/// Serialized as `deterministic-manifest.json` inside the bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeterministicManifest {
    /// Schema version for this manifest.
    pub schema_version: String,
    /// Active profile (dev/cert/record). Typed [`Profile`] so a typo
    /// can't survive serde at this boundary; wire format unchanged.
    pub profile: Profile,
    /// rustc version string.
    pub rustc: String,
    /// cargo version string.
    pub cargo: String,
    /// LLVM version derived from rustc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llvm_version: Option<String>,
    /// SHA-256 of `Cargo.lock` if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cargo_lock_hash: Option<String>,
    /// Raw contents of `rust-toolchain.toml` if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust_toolchain_toml: Option<String>,
    /// Value of the `RUSTFLAGS` env var.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rustflags: Option<String>,
    /// Source commit SHA.
    pub git_sha: String,
    /// Source branch name.
    pub git_branch: String,
    /// Source tree dirty status.
    pub git_dirty: bool,
}
