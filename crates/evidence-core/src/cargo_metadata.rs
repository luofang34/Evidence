//! Deterministic projection of `cargo metadata --format-version 1`
//! used as a bundle artifact (`cargo_metadata.json`) so verify-time
//! can replay the boundary checks generate ran.
//!
//! Wire shape: a flat array of `{ name, targets[].kind, links }`
//! entries, sorted by `name` ascending. The minimum needed for
//! [`crate::boundary_check::check_no_build_rs`] and
//! [`crate::boundary_check::check_no_proc_macros`] —
//! everything else cargo emits (id strings, paths, manifest
//! locations, dep graph) is dropped because it isn't load-bearing
//! for the recheck and would inflate the artifact.
//!
//! Sorting is load-bearing for SYS-003 (cross-host
//! reproducibility): two hosts with the same git state must
//! produce byte-identical bundles, so the projection must serialize
//! deterministically. Sort by package `name` ascending; targets
//! retain insertion order (cargo emits them in a deterministic
//! order from manifest declarations, so re-sorting is unnecessary).

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::boundary_check::{BuildRsViolation, ProcMacroViolation};

/// Projection of cargo metadata that lands in the bundle as
/// `cargo_metadata.json`. See module docs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct CargoMetadataProjection {
    /// Sorted list of package projections.
    pub packages: Vec<PackageProjection>,
}

/// One package's worth of cargo metadata that the boundary checks
/// care about.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageProjection {
    /// Package `name` field from `Cargo.toml`.
    pub name: String,
    /// Targets the package declares. Each target's `kind` array
    /// is the discriminator the checks key on (`"custom-build"`,
    /// `"proc-macro"`, `"lib"`, `"bin"`, …).
    pub targets: Vec<TargetProjection>,
    /// `links` field from `Cargo.toml`, if declared. Surfaces
    /// native-FFI bindings into the build_rs violation message
    /// (Layer 2). `None` for packages that don't declare it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<String>,
}

/// One target's `kind` array. Wrapped in a struct (rather than
/// being a bare `Vec<String>`) so the wire shape stays mirror-
/// symmetric with `cargo metadata`'s `packages[].targets[]`
/// objects — easier for an auditor reading the artifact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TargetProjection {
    /// `kind` values cargo emits, e.g. `["lib"]`,
    /// `["custom-build"]`, `["proc-macro"]`, `["bin"]`.
    pub kind: Vec<String>,
}

/// Errors building or loading a [`CargoMetadataProjection`].
#[derive(Debug, Error)]
pub enum ProjectionError {
    /// The raw `cargo metadata` output was not valid JSON in the
    /// shape this module expects.
    #[error("parsing cargo metadata JSON for projection")]
    ParseRawMetadata(#[source] serde_json::Error),
    /// The cached projection (read from a bundle's
    /// `cargo_metadata.json`) was not valid JSON in the
    /// projection shape.
    #[error("parsing cargo_metadata.json projection")]
    ParseProjection(#[source] serde_json::Error),
}

impl CargoMetadataProjection {
    /// Build a projection from raw `cargo metadata --format-version
    /// 1` JSON output. Sorts packages by name.
    pub fn from_raw_metadata(json: &str) -> Result<Self, ProjectionError> {
        let raw: RawMetadata =
            serde_json::from_str(json).map_err(ProjectionError::ParseRawMetadata)?;
        let mut packages: Vec<PackageProjection> = raw
            .packages
            .into_iter()
            .map(|p| PackageProjection {
                name: p.name,
                targets: p
                    .targets
                    .into_iter()
                    .map(|t| TargetProjection { kind: t.kind })
                    .collect(),
                links: p.links,
            })
            .collect();
        packages.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Self { packages })
    }

    /// Read a previously-serialized projection from
    /// `cargo_metadata.json`.
    pub fn from_projection_json(json: &str) -> Result<Self, ProjectionError> {
        serde_json::from_str(json).map_err(ProjectionError::ParseProjection)
    }

    /// Serialize to the canonical pretty-printed JSON written into
    /// the bundle. Determinism is via the sort applied on
    /// construction; serialization preserves that order.
    pub fn to_canonical_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// Build_rs violations against a cached projection. Same per-
/// crate scoping invariant as the live-cargo-metadata check.
pub fn check_build_rs_in_projection(
    in_scope: &[String],
    projection: &CargoMetadataProjection,
) -> Vec<BuildRsViolation> {
    let in_scope_set: std::collections::BTreeSet<&str> =
        in_scope.iter().map(String::as_str).collect();
    let mut out: Vec<BuildRsViolation> = projection
        .packages
        .iter()
        .filter(|p| in_scope_set.contains(p.name.as_str()))
        .filter(|p| p.targets.iter().any(target_is_build_rs))
        .map(|p| BuildRsViolation {
            crate_name: p.name.clone(),
            links: p.links.clone(),
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Proc-macro violations against a cached projection.
pub fn check_proc_macros_in_projection(
    in_scope: &[String],
    projection: &CargoMetadataProjection,
) -> Vec<ProcMacroViolation> {
    let in_scope_set: std::collections::BTreeSet<&str> =
        in_scope.iter().map(String::as_str).collect();
    let mut out: Vec<ProcMacroViolation> = projection
        .packages
        .iter()
        .filter(|p| in_scope_set.contains(p.name.as_str()))
        .filter(|p| p.targets.iter().any(target_is_proc_macro))
        .map(|p| ProcMacroViolation {
            crate_name: p.name.clone(),
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

fn target_is_build_rs(t: &TargetProjection) -> bool {
    t.kind.iter().any(|k| k == "custom-build")
}

fn target_is_proc_macro(t: &TargetProjection) -> bool {
    t.kind.iter().any(|k| k == "proc-macro")
}

impl PartialOrd for PackageProjection {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PackageProjection {
    fn cmp(&self, other: &Self) -> Ordering {
        self.name.cmp(&other.name)
    }
}

// ============================================================================
// Raw cargo metadata subset we deserialize at projection time.
// Private — only `CargoMetadataProjection::from_raw_metadata` constructs
// these and immediately maps them into the public types above.
// ============================================================================

#[derive(Debug, Deserialize)]
struct RawMetadata {
    packages: Vec<RawPackage>,
}

#[derive(Debug, Deserialize)]
struct RawPackage {
    name: String,
    #[serde(default)]
    targets: Vec<RawTarget>,
    #[serde(default)]
    links: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawTarget {
    #[serde(default)]
    kind: Vec<String>,
}

#[cfg(test)]
#[path = "cargo_metadata/tests.rs"]
mod tests;
