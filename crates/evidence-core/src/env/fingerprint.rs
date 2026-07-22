//! `EnvFingerprint` — the full build-environment struct written to `env.json`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::policy::Profile;

use super::capture::{EnvCaptureError, env_fingerprint};
use super::host::Host;
use super::manifest::{RecipeInputs, RecipeManifest};

/// Complete build environment fingerprint.
///
/// Captures all relevant environment information for reproducibility
/// verification and evidence generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvFingerprint {
    /// Active profile. Typed [`Profile`] so a typo'd string can't
    /// round-trip through serde; wire format is unchanged
    /// (`"dev"` / `"cert"` / `"record"`).
    pub profile: Profile,
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
    /// `true` when the tool binary that produced this bundle was a
    /// pre-release build (semver suffix containing `-` per §9).
    /// Drives `VERIFY_PRERELEASE_TOOL` under cert/record profiles —
    /// cert bundles from pre-release tools are not valid audit
    /// evidence. Default `false` for backwards compat: older
    /// bundles lacking the field deserialize as release-grade
    /// (which they trivially were — the flag didn't exist yet).
    #[serde(default)]
    pub tool_prerelease: bool,
}

impl EnvFingerprint {
    /// Capture the current build environment for a given profile.
    ///
    /// When `strict` is true (cert/record profiles), critical tools (rustc,
    /// cargo) must be detectable or an error is raised. This satisfies
    /// cert-mode strict error handling requirements.
    pub fn capture(profile: Profile, strict: bool) -> Result<Self, EnvCaptureError> {
        env_fingerprint(profile, strict)
    }

    /// Project this fingerprint plus the recorded build inputs onto
    /// the canonical recipe identity hashed as `recipe_hash` — the
    /// scope of the tool's same-recipe contract.
    ///
    /// **What the hash proves:** any two bundles whose recipe
    /// manifests agree declare the same build recipe — same
    /// toolchain, target, profile, feature selection, locked
    /// dependency graph, command recipe, source inputs, and
    /// resolution policy. **What it does not prove:** that the
    /// bundles' outputs reproduce. Reproduced-output equality is a
    /// separate comparison over input digests, recipe fields, and
    /// output digests; see
    /// [`crate::verify::compare_reproduction`].
    ///
    /// Intentionally NOT in the manifest (but still in `env.json`
    /// and therefore still in `content_hash`):
    ///
    /// - `host.*`, `tools`, `nav_env`, `in_nix_shell` — per-host
    ///   state. Belongs to content_hash, not to recipe identity.
    /// - `tool_prerelease` — a property of the tool binary, not of
    ///   the recipe the binary executed.
    ///
    /// `git_sha` / `git_branch` / `git_dirty` stay in the hashed
    /// projection as source metadata (parity with the pre-rename
    /// manifest, which also hashed them); the reproduction
    /// comparison excludes them from the recipe plane, and the
    /// cross-time CI gate compares the six-field toolchain
    /// projection rather than the raw hash, so per-commit git noise
    /// does not move either gate.
    pub fn recipe_manifest(&self, inputs: &RecipeInputs) -> RecipeManifest {
        RecipeManifest {
            schema_version: crate::schema_versions::DETERMINISTIC_MANIFEST.to_string(),
            profile: self.profile,
            rustc: self.rustc.clone(),
            cargo: self.cargo.clone(),
            llvm_version: self.llvm_version.clone(),
            cargo_lock_hash: self.cargo_lock_hash.clone(),
            rust_toolchain_toml: self.rust_toolchain_toml.clone(),
            rustflags: self.rustflags.clone(),
            target_triple: self.target_triple.clone(),
            features: inputs.features.clone(),
            locked_graph_hash: inputs.locked_graph_hash.clone(),
            command_recipe_hash: inputs.command_recipe_hash.clone(),
            inputs_hash: inputs.inputs_hash.clone(),
            resolution_policy: inputs.resolution_policy,
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
            profile: Profile::Dev,
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
            tool_prerelease: false,
        };
        assert_eq!(fp.profile, Profile::Dev);
        assert!(!fp.git_dirty);
        assert_eq!(fp.target_triple, "x86_64-unknown-linux-gnu");
        assert!(matches!(fp.host, Host::Linux { .. }));
    }

    /// Absence of `tool_prerelease` in a bundle's env.json must
    /// round-trip as `false`. Pre-PR-#60 bundles lack the field;
    /// their verify behavior must be identical to a release-grade
    /// build (which they trivially were — the flag didn't exist).
    #[test]
    fn tool_prerelease_absent_field_defaults_to_false() {
        // Minimal env.json shape with every required field but
        // WITHOUT `tool_prerelease`. Simulates a bundle produced
        // by a release of the tool before this field was added.
        let json = r#"{
            "profile": "dev",
            "rustc": "rustc 1.95.0",
            "cargo": "cargo 1.95.0",
            "git_sha": "abc",
            "git_branch": "main",
            "git_dirty": false,
            "in_nix_shell": false,
            "tools": {},
            "nav_env": {},
            "host": {"os":"linux","arch":"x86_64"},
            "target_triple": "x86_64-unknown-linux-gnu"
        }"#;
        let fp: EnvFingerprint = serde_json::from_str(json).expect("parses");
        assert!(
            !fp.tool_prerelease,
            "absent tool_prerelease must deserialize as false"
        );
    }

    /// Explicit `tool_prerelease: true` round-trips as `true`.
    #[test]
    fn tool_prerelease_explicit_true_roundtrips() {
        let json = r#"{
            "profile": "cert",
            "rustc": "rustc 1.95.0",
            "cargo": "cargo 1.95.0",
            "git_sha": "abc",
            "git_branch": "main",
            "git_dirty": false,
            "in_nix_shell": false,
            "tools": {},
            "nav_env": {},
            "host": {"os":"linux","arch":"x86_64"},
            "target_triple": "x86_64-unknown-linux-gnu",
            "tool_prerelease": true
        }"#;
        let fp: EnvFingerprint = serde_json::from_str(json).expect("parses");
        assert!(fp.tool_prerelease);
    }

    fn recipe_inputs_fixture() -> RecipeInputs {
        RecipeInputs {
            features: Vec::new(),
            locked_graph_hash: None,
            command_recipe_hash: "c".repeat(64),
            inputs_hash: "d".repeat(64),
            resolution_policy: crate::policy::ResolutionPolicy::LockedOffline,
        }
    }

    /// The projection carries every recipe field — toolchain,
    /// target triple, profile, features, locked graph, command
    /// recipe, source inputs, resolution policy — in the documented
    /// canonical declaration order, with git identity retained as
    /// trailing source metadata.
    #[test]
    fn recipe_manifest_records_recipe_fields_in_canonical_order() {
        let fp = EnvFingerprint {
            profile: Profile::Cert,
            rustc: "rustc 1.95.0".to_string(),
            cargo: "cargo 1.95.0".to_string(),
            git_sha: "abc123".to_string(),
            git_branch: "main".to_string(),
            git_dirty: false,
            in_nix_shell: false,
            tools: BTreeMap::new(),
            nav_env: BTreeMap::new(),
            llvm_version: Some("20.0.0".to_string()),
            host: Host::Linux {
                arch: "x86_64".to_string(),
                libc: None,
                kernel: None,
            },
            cargo_lock_hash: Some("e".repeat(64)),
            rust_toolchain_toml: Some("[toolchain]".to_string()),
            rustflags: Some("-D warnings".to_string()),
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            tool_prerelease: false,
        };
        let manifest = fp.recipe_manifest(&recipe_inputs_fixture());
        assert_eq!(manifest.target_triple, "x86_64-unknown-linux-gnu");
        assert_eq!(manifest.profile, Profile::Cert);
        assert!(manifest.features.is_empty());
        assert_eq!(manifest.locked_graph_hash, None);
        assert_eq!(manifest.command_recipe_hash, "c".repeat(64));
        assert_eq!(manifest.inputs_hash, "d".repeat(64));
        assert_eq!(
            manifest.resolution_policy,
            crate::policy::ResolutionPolicy::LockedOffline
        );

        let text = serde_json::to_string_pretty(&manifest).expect("serialize");
        let fields = [
            "schema_version",
            "profile",
            "rustc",
            "cargo",
            "llvm_version",
            "cargo_lock_hash",
            "rust_toolchain_toml",
            "rustflags",
            "target_triple",
            "features",
            "locked_graph_hash",
            "command_recipe_hash",
            "inputs_hash",
            "resolution_policy",
            "git_sha",
            "git_branch",
            "git_dirty",
        ];
        let positions: Vec<usize> = fields
            .iter()
            .map(|name| {
                text.find(&format!("\"{name}\":"))
                    .unwrap_or_else(|| panic!("field {name} missing from manifest: {text}"))
            })
            .collect();
        assert!(
            positions.windows(2).all(|w| w[0] < w[1]),
            "canonical field order drifted — verify's byte-compare re-projection depends on it:\n{text}"
        );
    }

    /// Same fingerprint + same recipe inputs → byte-identical
    /// manifest JSON across repeated projections.
    #[test]
    fn recipe_manifest_is_byte_deterministic() {
        let fp: EnvFingerprint = serde_json::from_str(
            r#"{
                "profile": "dev",
                "rustc": "rustc 1.95.0",
                "cargo": "cargo 1.95.0",
                "git_sha": "abc",
                "git_branch": "main",
                "git_dirty": false,
                "in_nix_shell": false,
                "tools": {},
                "nav_env": {},
                "host": {"os":"linux","arch":"x86_64"},
                "target_triple": "x86_64-unknown-linux-gnu"
            }"#,
        )
        .expect("parses");
        let inputs = recipe_inputs_fixture();
        let first = serde_json::to_vec_pretty(&fp.recipe_manifest(&inputs)).expect("serialize");
        let second = serde_json::to_vec_pretty(&fp.recipe_manifest(&inputs)).expect("serialize");
        assert_eq!(first, second);
    }
}
