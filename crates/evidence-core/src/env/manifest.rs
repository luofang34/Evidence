//! `RecipeManifest` — the canonical recipe identity committed as
//! `deterministic-manifest.json`.
//!
//! The manifest is the SHA-256-hashed projection that
//! `index.json.recipe_hash` attests. It extends the toolchain
//! projection with the full recipe identity: target triple, profile,
//! feature selection, locked dependency graph, command recipe,
//! source-input digests, and resolution policy.
//!
//! # What the hash proves — and what it does not
//!
//! `recipe_hash` proves two bundles declare the SAME recipe. It does
//! not prove their outputs reproduce: output equality is a separate,
//! stronger claim checked by
//! [`crate::verify::compare_reproduction`], which
//! compares input digests, recipe fields, and output digests plane by
//! plane. The four integrity/identity claims a bundle makes are:
//!
//! 1. **Bundle content integrity** — `SHA256SUMS` / `content_hash`
//!    prove the recorded bytes are untampered.
//! 2. **Recipe identity** — `recipe_hash` proves same declared
//!    recipe (this manifest).
//! 3. **Cross-host recipe parity** — the six-field toolchain
//!    projection (`rustc`, `cargo`, `llvm_version`,
//!    `cargo_lock_hash`, `rust_toolchain_toml`, `rustflags`) the CI
//!    determinism gates compare; proves toolchain sameness across
//!    hosts and time. The full recipe binds host-defining fields
//!    (`target_triple`), so `recipe_hash` itself is a same-target
//!    identity.
//! 4. **Reproduced-output equality** — the reproduction comparison;
//!    proves outputs actually digest-equal on a shared recipe.
//!
//! # Canonical field order
//!
//! Serde serializes in declaration order, and that order IS the
//! canonical byte contract (verify re-projects and byte-compares):
//!
//! `schema_version`, `profile`, `rustc`, `cargo`, `llvm_version`,
//! `cargo_lock_hash`, `rust_toolchain_toml`, `rustflags`,
//! `target_triple`, `features`, `locked_graph_hash`,
//! `command_recipe_hash`, `inputs_hash`, `resolution_policy`,
//! `git_sha`, `git_branch`, `git_dirty`.
//!
//! `git_sha` / `git_branch` / `git_dirty` ride along as source
//! metadata inside the hashed projection. They vary commit to
//! commit, so the reproduction comparison excludes them from the
//! recipe plane and the cross-time CI gate projects the six
//! toolchain fields out of the manifest rather than comparing
//! `recipe_hash` raw. They remain hashed here for parity with the
//! pre-rename manifest, which also hashed them: dropping them would
//! change what the hash binds beyond the rename and the added
//! recipe fields.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::bundle::CommandRecord;
use crate::cargo_metadata::CargoMetadataProjection;
use crate::policy::{Profile, ResolutionPolicy};

/// Canonical recipe identity.
///
/// A committed, SHA-256-hashed projection of `env.json` plus the
/// recorded build inputs. See the module docs for the claim
/// taxonomy and the canonical field order.
///
/// Serialized as `deterministic-manifest.json` inside the bundle;
/// the filename is load-bearing (CI artifacts fetch it by name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeManifest {
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
    /// Build target triple from `rustc -vV`. Host-defining: a CI
    /// matrix of native builds produces one triple per host, so the
    /// full recipe identity is a same-target comparison while the
    /// six-field toolchain projection stays the cross-host one.
    pub target_triple: String,
    /// Cargo feature selection the build ran with. The tool does
    /// not set cargo features, so this is always `[]`; the field
    /// exists so feature selection enters the recipe identity the
    /// day the tool gains it, without a wire-shape break.
    pub features: Vec<String>,
    /// SHA-256 of the canonical resolved-dependency projection (the
    /// `dependencies` map of `cargo_metadata.json`). `null` when the
    /// bundle carries no `cargo_metadata.json` — development bundles
    /// only carry the artifact when the boundary policy enables
    /// `forbid_build_rs` / `forbid_proc_macros`.
    pub locked_graph_hash: Option<String>,
    /// SHA-256 of the canonical `commands.json` content — the exact
    /// command recipe (argv, exit codes, output paths). An empty
    /// command list hashes fine.
    pub command_recipe_hash: String,
    /// SHA-256 of the canonical `inputs_hashes.json` content — the
    /// aggregate over every recorded source-input digest.
    pub inputs_hash: String,
    /// Dependency-resolution policy the bundle was generated under.
    pub resolution_policy: ResolutionPolicy,
    /// Source commit SHA. Source metadata, not recipe content: kept
    /// hashed for parity with the pre-rename manifest, excluded from
    /// the reproduction comparison's recipe plane.
    pub git_sha: String,
    /// Source branch name. See [`Self::git_sha`].
    pub git_branch: String,
    /// Source tree dirty status. See [`Self::git_sha`].
    pub git_dirty: bool,
}

/// The non-fingerprint inputs [`RecipeManifest`] needs: everything
/// the recipe binds that does not come from `env.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeInputs {
    /// Cargo feature selection. Empty until the tool gains cargo
    /// feature selection.
    pub features: Vec<String>,
    /// SHA-256 of the canonical resolved-dependency projection;
    /// `None` when the bundle carries no `cargo_metadata.json`.
    pub locked_graph_hash: Option<String>,
    /// SHA-256 of the canonical command-recipe serialization.
    pub command_recipe_hash: String,
    /// SHA-256 of the canonical source-input digest serialization.
    pub inputs_hash: String,
    /// Resolution policy the bundle was generated under.
    pub resolution_policy: ResolutionPolicy,
}

/// Errors computing or gathering the recipe projection inputs.
///
/// Deliberately uncoded (no [`crate::diagnostic::DiagnosticCode`]
/// impl), same as [`crate::corpus::CorpusError`]: the projection runs
/// inside builder/verify paths that already own the diagnostic
/// surface, so a second code family would double-report.
#[derive(Debug, Error)]
pub enum RecipeProjectionError {
    /// Reading a bundle artifact failed.
    #[error("{op} {path:?}")]
    Io {
        /// Operation attempted (`"reading"`).
        op: &'static str,
        /// File whose read failed.
        path: std::path::PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// Canonical serialization of a projection input failed.
    #[error("serializing {kind}")]
    Serialize {
        /// Which input failed (`"inputs_hashes.json"`, …).
        kind: &'static str,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },
    /// A bundle artifact present on disk did not parse into its
    /// expected shape.
    #[error("parsing {path:?} as {kind}")]
    Parse {
        /// Shape expected (`"inputs_hashes.json"`, …).
        kind: &'static str,
        /// File that failed to parse.
        path: std::path::PathBuf,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },
}

/// SHA-256 of the canonical serialization of the source-input digest
/// map — the exact bytes `write_inputs` persists to
/// `inputs_hashes.json`, so the generate-time and re-projected
/// digests agree.
pub fn inputs_digest(inputs: &BTreeMap<String, String>) -> Result<String, RecipeProjectionError> {
    let bytes =
        serde_json::to_vec_pretty(inputs).map_err(|source| RecipeProjectionError::Serialize {
            kind: "inputs_hashes.json",
            source,
        })?;
    Ok(crate::hash::sha256(&bytes))
}

/// SHA-256 of the canonical serialization of the command-recipe
/// rows — the exact bytes `write_commands` persists to
/// `commands.json`. An empty slice hashes fine.
pub fn commands_digest(commands: &[CommandRecord]) -> Result<String, RecipeProjectionError> {
    let bytes =
        serde_json::to_vec_pretty(commands).map_err(|source| RecipeProjectionError::Serialize {
            kind: "commands.json",
            source,
        })?;
    Ok(crate::hash::sha256(&bytes))
}

/// SHA-256 of the canonical resolved-dependency projection: the
/// compact serialization of the projection's `dependencies` map
/// (BTree-ordered by construction, so the bytes are deterministic).
pub fn locked_graph_digest(
    projection: &CargoMetadataProjection,
) -> Result<String, RecipeProjectionError> {
    let bytes = serde_json::to_vec(&projection.dependencies).map_err(|source| {
        RecipeProjectionError::Serialize {
            kind: "cargo_metadata.json dependencies",
            source,
        }
    })?;
    Ok(crate::hash::sha256(&bytes))
}

/// What failed when gathering [`RecipeInputs`] from a bundle
/// directory. Split from [`RecipeProjectionError`] so verify can
/// distinguish "projection input absent" (skip the re-projection —
/// the absence already fired `MissingHashedFile` upstream) from
/// "present but unreadable" (surface as projection drift).
#[derive(Debug)]
pub enum GatherFailure {
    /// A required projection input (`inputs_hashes.json` or
    /// `commands.json`) is not on disk.
    Missing,
    /// A projection input exists but cannot be read or parsed.
    Unreadable(RecipeProjectionError),
}

impl RecipeInputs {
    /// Gather the recipe inputs from a bundle directory: hashes of
    /// the canonical `inputs_hashes.json` and `commands.json`
    /// contents, plus the locked-graph hash when the bundle carries
    /// `cargo_metadata.json`. `features` is always empty — the tool
    /// does not set cargo features.
    pub fn from_bundle_dir(
        bundle: &Path,
        resolution_policy: ResolutionPolicy,
    ) -> Result<Self, GatherFailure> {
        let inputs_path = bundle.join("inputs_hashes.json");
        if !inputs_path.exists() {
            return Err(GatherFailure::Missing);
        }
        let commands_path = bundle.join("commands.json");
        if !commands_path.exists() {
            return Err(GatherFailure::Missing);
        }
        let inputs: BTreeMap<String, String> = read_json(&inputs_path, "inputs_hashes.json")?;
        let commands: Vec<CommandRecord> = read_json(&commands_path, "commands.json")?;
        let inputs_hash = inputs_digest(&inputs).map_err(GatherFailure::Unreadable)?;
        let command_recipe_hash = commands_digest(&commands).map_err(GatherFailure::Unreadable)?;

        let metadata_path = bundle.join("cargo_metadata.json");
        let locked_graph_hash = if metadata_path.exists() {
            let projection: CargoMetadataProjection =
                read_json(&metadata_path, "cargo_metadata.json")?;
            Some(locked_graph_digest(&projection).map_err(GatherFailure::Unreadable)?)
        } else {
            None
        };

        Ok(Self {
            features: Vec::new(),
            locked_graph_hash,
            command_recipe_hash,
            inputs_hash,
            resolution_policy,
        })
    }
}

/// Read + parse one bundle JSON artifact, mapping failures into
/// [`GatherFailure::Unreadable`].
fn read_json<T: serde::de::DeserializeOwned>(
    path: &Path,
    kind: &'static str,
) -> Result<T, GatherFailure> {
    let bytes = std::fs::read(path).map_err(|source| {
        GatherFailure::Unreadable(RecipeProjectionError::Io {
            op: "reading",
            path: path.to_path_buf(),
            source,
        })
    })?;
    serde_json::from_slice(&bytes).map_err(|source| {
        GatherFailure::Unreadable(RecipeProjectionError::Parse {
            kind,
            path: path.to_path_buf(),
            source,
        })
    })
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
    use std::collections::BTreeSet;

    fn sample_projection() -> CargoMetadataProjection {
        let mut dependencies: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        dependencies.insert(
            "app 0.1.0".to_string(),
            BTreeSet::from(["lib 0.2.0".to_string()]),
        );
        dependencies.insert("lib 0.2.0".to_string(), BTreeSet::new());
        CargoMetadataProjection {
            packages: Vec::new(),
            dependencies,
        }
    }

    #[test]
    fn digests_match_canonical_serializations() {
        let mut inputs = BTreeMap::new();
        inputs.insert("src/lib.rs".to_string(), "a".repeat(64));
        let expected_inputs =
            crate::hash::sha256(&serde_json::to_vec_pretty(&inputs).expect("serialize inputs"));
        assert_eq!(inputs_digest(&inputs).expect("digest"), expected_inputs);

        let commands: Vec<CommandRecord> = Vec::new();
        let expected_commands =
            crate::hash::sha256(&serde_json::to_vec_pretty(&commands).expect("serialize commands"));
        assert_eq!(
            commands_digest(&commands).expect("digest"),
            expected_commands
        );

        let projection = sample_projection();
        let expected_graph = crate::hash::sha256(
            &serde_json::to_vec(&projection.dependencies).expect("serialize graph"),
        );
        assert_eq!(
            locked_graph_digest(&projection).expect("digest"),
            expected_graph
        );
    }

    #[test]
    fn recipe_inputs_from_bundle_round_trip() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let bundle = tmp.path();

        let mut inputs = BTreeMap::new();
        inputs.insert("src/lib.rs".to_string(), "b".repeat(64));
        std::fs::write(
            bundle.join("inputs_hashes.json"),
            serde_json::to_vec_pretty(&inputs).expect("serialize inputs"),
        )
        .expect("write inputs");
        let commands: Vec<CommandRecord> = Vec::new();
        std::fs::write(
            bundle.join("commands.json"),
            serde_json::to_vec_pretty(&commands).expect("serialize commands"),
        )
        .expect("write commands");
        std::fs::write(
            bundle.join("cargo_metadata.json"),
            serde_json::to_vec_pretty(&sample_projection()).expect("serialize metadata"),
        )
        .expect("write metadata");

        let gathered = RecipeInputs::from_bundle_dir(bundle, ResolutionPolicy::LockedOffline)
            .expect("gather succeeds");
        assert!(gathered.features.is_empty());
        assert_eq!(
            gathered.inputs_hash,
            inputs_digest(&inputs).expect("digest")
        );
        assert_eq!(
            gathered.command_recipe_hash,
            commands_digest(&commands).expect("digest")
        );
        assert_eq!(
            gathered.locked_graph_hash,
            Some(locked_graph_digest(&sample_projection()).expect("digest"))
        );
        assert_eq!(gathered.resolution_policy, ResolutionPolicy::LockedOffline);

        // Without cargo_metadata.json the locked-graph hash is None.
        std::fs::remove_file(bundle.join("cargo_metadata.json")).expect("remove metadata");
        let gathered = RecipeInputs::from_bundle_dir(bundle, ResolutionPolicy::LockedOffline)
            .expect("gather succeeds without metadata");
        assert_eq!(gathered.locked_graph_hash, None);

        // Missing inputs_hashes.json is a Missing, not an Unreadable.
        std::fs::remove_file(bundle.join("inputs_hashes.json")).expect("remove inputs");
        let outcome = RecipeInputs::from_bundle_dir(bundle, ResolutionPolicy::LockedOffline);
        assert!(
            matches!(outcome, Err(GatherFailure::Missing)),
            "missing inputs_hashes.json must yield GatherFailure::Missing, got {outcome:?}"
        );
    }
}
