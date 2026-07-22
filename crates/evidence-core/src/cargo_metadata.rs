//! Deterministic projection of `cargo metadata --format-version 1`
//! used as a bundle artifact (`cargo_metadata.json`) so verify-time
//! can replay the boundary checks generate ran, and so the bundle
//! binds the resolved dependency graph the baseline produced
//! (LLR-072 / LLR-141).
//!
//! Wire shape: an object with two members:
//!
//! - `packages`: a flat array of `{ name, targets[].kind, links }`
//!   entries, sorted by `name` ascending. The minimum needed for
//!   [`crate::boundary_check::check_no_build_rs`] and
//!   [`crate::boundary_check::check_no_proc_macros`].
//! - `dependencies`: the RESOLVED dependency graph (not the declared
//!   dependency specs), projected from `resolve.nodes`: a map from
//!   each resolved package's `"name version"` identity to the sorted,
//!   deduplicated set of `"name version"` identities it depends on.
//!   Resolved-package identity is name+version rather than cargo's
//!   package-id spec because the spec embeds host-specific absolute
//!   paths for path dependencies, which would break cross-host
//!   byte-stability. A resolve node or dependency whose id is absent
//!   from `packages[]`, or two resolved packages collapsing onto the
//!   same name+version identity, fails the projection closed.
//!
//! Sorting is load-bearing for SYS-003 (cross-host
//! reproducibility): two hosts with the same git state must
//! produce byte-identical bundles, so the projection must serialize
//! deterministically. Packages sort by `name` ascending; targets
//! retain insertion order (cargo emits them in a deterministic
//! order from manifest declarations, so re-sorting is unnecessary);
//! the dependency map and its value sets are `BTree`-ordered by
//! construction.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::boundary_check::{BuildRsViolation, ProcMacroViolation};

/// Projection of cargo metadata that lands in the bundle as
/// `cargo_metadata.json`. See module docs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CargoMetadataProjection {
    /// Sorted list of package projections.
    pub packages: Vec<PackageProjection>,
    /// The resolved dependency graph: each resolved package's
    /// `"name version"` identity mapped to the sorted set of
    /// `"name version"` identities it depends on. Every resolved
    /// package appears as a key, including packages with no
    /// dependencies (empty set), so the map's key set is the full
    /// resolved package set the baseline produced.
    pub dependencies: BTreeMap<String, BTreeSet<String>>,
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
    /// A `resolve.nodes[]` entry or one of its dependencies names a
    /// package id that `packages[]` does not declare. Cargo
    /// guarantees the closure; a gap means the schema drifted and
    /// the projection would silently understate the graph — fail
    /// closed instead.
    #[error("resolved package id '{0}' is absent from packages[]")]
    UnresolvablePackageId(String),
    /// Two resolved packages collapse onto the same `"name version"`
    /// identity (the same name+version resolved from two different
    /// sources). Merging them would bind a graph the baseline did
    /// not produce — fail closed instead.
    #[error("resolved graph maps two package ids onto the identity '{0}'")]
    AmbiguousPackageIdentity(String),
}

impl CargoMetadataProjection {
    /// Build a projection from raw `cargo metadata --format-version
    /// 1` JSON output. Sorts packages by name; projects the resolved
    /// dependency graph from `resolve.nodes` (see module docs).
    pub fn from_raw_metadata(json: &str) -> Result<Self, ProjectionError> {
        let raw: RawMetadata =
            serde_json::from_str(json).map_err(ProjectionError::ParseRawMetadata)?;
        let dependencies = project_resolved_graph(&raw)?;
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
        Ok(Self {
            packages,
            dependencies,
        })
    }

    /// Read a projection serialized by an earlier
    /// `cargo_metadata.json` capture.
    pub fn from_projection_json(json: &str) -> Result<Self, ProjectionError> {
        serde_json::from_str(json).map_err(ProjectionError::ParseProjection)
    }

    /// Serialize to the canonical pretty-printed JSON written into
    /// the bundle. Determinism is via the sort applied on
    /// construction plus the `BTree` ordering of the dependency
    /// graph; serialization preserves that order.
    pub fn to_canonical_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// Project `resolve.nodes` into the `"name version"` → dependency-set
/// map. Fails closed on a dangling package id or an identity
/// collision; both are documented on [`ProjectionError`].
fn project_resolved_graph(
    raw: &RawMetadata,
) -> Result<BTreeMap<String, BTreeSet<String>>, ProjectionError> {
    let id_to_identity: BTreeMap<&str, String> = raw
        .packages
        .iter()
        .map(|p| (p.id.as_str(), format!("{} {}", p.name, p.version)))
        .collect();
    let resolve_identity = |id: &str| -> Result<String, ProjectionError> {
        id_to_identity
            .get(id)
            .cloned()
            .ok_or_else(|| ProjectionError::UnresolvablePackageId(id.to_string()))
    };
    let mut dependencies: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for node in &raw.resolve.nodes {
        let identity = resolve_identity(&node.id)?;
        let deps: BTreeSet<String> = node
            .deps
            .iter()
            .map(|d| resolve_identity(&d.pkg))
            .collect::<Result<_, _>>()?;
        if dependencies.insert(identity.clone(), deps).is_some() {
            return Err(ProjectionError::AmbiguousPackageIdentity(identity));
        }
    }
    Ok(dependencies)
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
    resolve: RawResolve,
}

#[derive(Debug, Deserialize)]
struct RawPackage {
    name: String,
    id: String,
    version: String,
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

#[derive(Debug, Deserialize)]
struct RawResolve {
    nodes: Vec<RawNode>,
}

#[derive(Debug, Deserialize)]
struct RawNode {
    id: String,
    deps: Vec<RawNodeDep>,
}

#[derive(Debug, Deserialize)]
struct RawNodeDep {
    pkg: String,
}

#[cfg(test)]
#[path = "cargo_metadata/tests.rs"]
mod tests;
