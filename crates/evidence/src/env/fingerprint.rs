//! `EnvFingerprint` — the full build-environment struct written to `env.json`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::capture::{EnvCaptureError, env_fingerprint};
use super::host::Host;
use super::manifest::DeterministicManifest;

/// Complete build environment fingerprint.
///
/// Captures all relevant environment information for reproducibility
/// verification and evidence generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvFingerprint {
    /// Active profile name
    pub profile: String,
    /// rustc version string
    pub rustc: String,
    /// cargo version string
    pub cargo: String,
    /// Current git commit SHA
    pub git_sha: String,
    /// Current git branch
    pub git_branch: String,
    /// Whether git working directory is dirty
    pub git_dirty: bool,
    /// Whether running in a Nix shell
    pub in_nix_shell: bool,
    /// Map of tool name to availability
    pub tools: BTreeMap<String, bool>,
    /// NAV_* environment variables
    pub nav_env: BTreeMap<String, String>,
    /// LLVM version from rustc (for platform capsule)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llvm_version: Option<String>,
    /// Host platform description (per-OS shape).
    ///
    /// Replaces the former `host_os` / `host_arch` / `libc_version`
    /// trio; `target_triple` remains a sibling field because it
    /// describes the Rust build target, not the host.
    pub host: Host,
    /// SHA-256 of Cargo.lock if present in the workspace root
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cargo_lock_hash: Option<String>,
    /// Contents of rust-toolchain.toml if present in the workspace root
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust_toolchain_toml: Option<String>,
    /// Value of the RUSTFLAGS environment variable if set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rustflags: Option<String>,
    /// Host target triple from `rustc -vV`
    pub target_triple: String,
}

impl EnvFingerprint {
    /// Capture the current build environment for a given profile.
    ///
    /// When `strict` is true (cert/record profiles), critical tools (rustc,
    /// cargo) must be detectable or an error is raised. This satisfies
    /// cert-mode strict error handling requirements.
    pub fn capture(profile: &str, strict: bool) -> Result<Self, EnvCaptureError> {
        env_fingerprint(profile, strict)
    }

    /// Project this fingerprint onto the cross-host-stable subset
    /// used for `deterministic_hash` — the scope of the tool's
    /// reproducibility contract.
    ///
    /// **Scope: "same commit + same toolchain."** Any two bundles
    /// that agree on these ten fields represent the same logical
    /// build from a source + toolchain perspective.
    ///
    /// Intentionally NOT in the manifest (but still in `env.json`
    /// and therefore still in `content_hash`):
    ///
    /// - `host.*`, `tools`, `nav_env`, `in_nix_shell` — per-host
    ///   state. Belongs to content_hash, not to identity.
    /// - `target_triple` — semantically identity-defining, but
    ///   practically host-variable. Native `cargo build` on Linux /
    ///   macOS / Windows defaults to the host triple, so a CI matrix
    ///   that runs native builds on all three hosts would produce
    ///   three different target triples and the parity test could
    ///   never pass without cross-compile plumbing. We keep target
    ///   triple fully recorded in `env.json` (it's in `content_hash`
    ///   for audit), and downstream consumers that need strict build
    ///   identity should compare `deterministic_hash` **and**
    ///   `env.target_triple` together.
    pub fn deterministic_manifest(&self) -> DeterministicManifest {
        DeterministicManifest {
            schema_version: crate::schema_versions::DETERMINISTIC_MANIFEST.to_string(),
            profile: self.profile.clone(),
            rustc: self.rustc.clone(),
            cargo: self.cargo.clone(),
            llvm_version: self.llvm_version.clone(),
            cargo_lock_hash: self.cargo_lock_hash.clone(),
            rust_toolchain_toml: self.rust_toolchain_toml.clone(),
            rustflags: self.rustflags.clone(),
            git_sha: self.git_sha.clone(),
            git_branch: self.git_branch.clone(),
            git_dirty: self.git_dirty,
        }
    }
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
    fn test_env_fingerprint_fields() {
        let fp = EnvFingerprint {
            profile: "test".to_string(),
            rustc: "rustc 1.70.0".to_string(),
            cargo: "cargo 1.70.0".to_string(),
            git_sha: "abc123".to_string(),
            git_branch: "main".to_string(),
            git_dirty: false,
            in_nix_shell: false,
            tools: BTreeMap::new(),
            nav_env: BTreeMap::new(),
            llvm_version: Some("16.0.0".to_string()),
            host: Host::Linux {
                arch: "x86_64".to_string(),
                libc: Some("glibc 2.31".to_string()),
                kernel: Some("5.15.0-89-generic".to_string()),
            },
            cargo_lock_hash: None,
            rust_toolchain_toml: None,
            rustflags: None,
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
        };
        assert_eq!(fp.profile, "test");
        assert!(!fp.git_dirty);
        assert_eq!(fp.target_triple, "x86_64-unknown-linux-gnu");
        assert!(matches!(fp.host, Host::Linux { .. }));
    }
}
