//! `compare_reproduction` — reproduced-output comparison over two
//! evidence bundles (SYS-048 / HLR-112 / LLR-146).
//!
//! Where [`verify_bundle`](crate::verify::verify_bundle) answers
//! "is THIS bundle internally intact?", this module answers "did
//! THAT bundle reproduce the baseline?" — the comparison the
//! recipe identity (`index.json.recipe_hash`,
//! [`RecipeManifest`](crate::env::RecipeManifest)) exists
//! to support. The two questions are different claims:
//!
//! 1. **Bundle content integrity** — `SHA256SUMS` / `content_hash`
//!    prove a bundle's recorded bytes are untampered.
//! 2. **Recipe identity** — `recipe_hash` proves two bundles declare
//!    the same recipe (same toolchain, target, profile, features,
//!    locked graph, command recipe, source inputs, resolution
//!    policy).
//! 3. **Cross-host recipe parity** — the six-field toolchain
//!    projection the CI determinism gates compare; proves toolchain
//!    sameness across hosts and time. Because the full recipe binds
//!    host-defining fields (`target_triple`), parity across hosts is
//!    judged on the projection, not on the raw `recipe_hash`.
//! 4. **Reproduced-output equality** — THIS comparison: input
//!    digests, recipe fields, and output digests all equal. It is a
//!    same-target claim; target artifacts legitimately differ across
//!    hosts, which is exactly why (3) and (4) are separate.
//!
//! # Equality rule
//!
//! Two bundles are reproduction-equal iff [`compare_reproduction`]
//! returns an empty finding list: identical canonical input digest
//! sets, identical recipe fields, identical output digest maps.
//! Any difference yields typed findings, deterministically sorted
//! (the enum's declaration order, then the path/field name inside a
//! variant). Missing or unparseable plane files yield explicit
//! non-success findings — never a panic, never a silent pass.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// One typed difference between a baseline and a candidate bundle.
///
/// `Ord` follows declaration order — inputs first, then recipe,
/// then outputs — with the path/field name ordering inside a
/// variant, so a sorted finding list is deterministic.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReproductionFinding {
    /// A declared source input's digest differs between baseline
    /// and candidate.
    InputChanged {
        /// Bundle-relative input path (key of `inputs_hashes.json`).
        path: String,
    },
    /// A source input is declared on exactly one side. The
    /// canonical input digest set must be identical; any asymmetry
    /// is a missing declaration.
    InputMissing {
        /// Bundle-relative input path.
        path: String,
    },
    /// A source input cannot be verified: its recorded digest is
    /// not 64-character lowercase hex, or the whole
    /// `inputs_hashes.json` plane is missing/unparseable on one
    /// side (reported with `path = "inputs_hashes.json"`).
    InputUnverifiable {
        /// Bundle-relative input path, or the plane filename.
        path: String,
    },
    /// One recipe field differs between the two manifests. `field`
    /// is the canonical recipe-field name; the lockfile plane
    /// (`cargo_lock_hash`) is reported as `dependency_lock`.
    RecipeFieldChanged {
        /// Canonical recipe-field name.
        field: &'static str,
    },
    /// `deterministic-manifest.json` is missing or unparseable on
    /// at least one side, so the recipe plane cannot be compared
    /// at all.
    RecipeUnavailable,
    /// A declared output artifact's digest differs between
    /// baseline and candidate.
    OutputChanged {
        /// Bundle-relative artifact path (key of
        /// `outputs_hashes.json`).
        artifact: String,
    },
    /// An output declared by the baseline is absent from the
    /// candidate — an expected artifact was not reproduced.
    OutputMissing {
        /// Bundle-relative artifact path.
        artifact: String,
    },
    /// An output declared by the candidate has no baseline
    /// counterpart — the candidate produced something the baseline
    /// never attested.
    OutputExtra {
        /// Bundle-relative artifact path.
        artifact: String,
    },
    /// An output artifact cannot be verified: its recorded digest
    /// is not 64-character lowercase hex, or the whole
    /// `outputs_hashes.json` plane is missing/unparseable on one
    /// side (reported with `artifact = "outputs_hashes.json"`).
    OutputUnverifiable {
        /// Bundle-relative artifact path, or the plane filename.
        artifact: String,
    },
}

/// Errors from [`compare_reproduction`]. Only genuine operational
/// failures are errors — missing plane files are findings, not
/// errors.
///
/// Deliberately uncoded (no [`crate::diagnostic::DiagnosticCode`]
/// impl), same as [`crate::corpus::CorpusError`]: the comparison is
/// a library API, not a diagnostic surface.
#[derive(Debug, Error)]
pub enum ReproductionError {
    /// One of the two bundle roots is not a directory.
    #[error("bundle directory not found: {0:?}")]
    BundleNotFound(PathBuf),
    /// A plane file that exists could not be read.
    #[error("reading {path:?}")]
    Io {
        /// File whose read failed.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
}

/// The recipe fields compared plane-wise, as
/// `(manifest JSON key, reported field name)` pairs. Order is the
/// canonical [`RecipeManifest`](crate::env::RecipeManifest)
/// field order; `schema_version` and
/// the git-identity fields are excluded — git identity is source
/// metadata (cross-host parity's domain), not recipe content.
pub(crate) const RECIPE_FIELDS: &[(&str, &str)] = &[
    ("profile", "profile"),
    ("rustc", "rustc"),
    ("cargo", "cargo"),
    ("llvm_version", "llvm_version"),
    ("cargo_lock_hash", "dependency_lock"),
    ("rust_toolchain_toml", "rust_toolchain_toml"),
    ("rustflags", "rustflags"),
    ("target_triple", "target_triple"),
    ("features", "features"),
    ("locked_graph_hash", "locked_graph_hash"),
    ("command_recipe_hash", "command_recipe_hash"),
    ("inputs_hash", "inputs_hash"),
    ("resolution_policy", "resolution_policy"),
];

/// Compare two bundle directories for reproduced-output equality.
///
/// Reads `inputs_hashes.json`, `deterministic-manifest.json`, and
/// `outputs_hashes.json` from both `baseline` and `candidate` and
/// returns every difference as a sorted list of typed
/// [`ReproductionFinding`]s. Equality holds iff the list is empty.
///
/// # Errors
///
/// Returns [`ReproductionError::BundleNotFound`] when either root is
/// not a directory, and [`ReproductionError::Io`] when a plane file
/// that exists cannot be read. Missing plane files produce findings
/// ([`ReproductionFinding::RecipeUnavailable`],
/// [`ReproductionFinding::InputUnverifiable`],
/// [`ReproductionFinding::OutputUnverifiable`]), not errors.
pub fn compare_reproduction(
    baseline: &Path,
    candidate: &Path,
) -> Result<Vec<ReproductionFinding>, ReproductionError> {
    if !baseline.is_dir() {
        return Err(ReproductionError::BundleNotFound(baseline.to_path_buf()));
    }
    if !candidate.is_dir() {
        return Err(ReproductionError::BundleNotFound(candidate.to_path_buf()));
    }

    let mut findings = Vec::new();
    compare_inputs(baseline, candidate, &mut findings)?;
    compare_recipe(baseline, candidate, &mut findings)?;
    compare_outputs(baseline, candidate, &mut findings)?;
    // Derived `Ord`: variant declaration order (inputs, recipe,
    // outputs), then the path/field name inside a variant.
    findings.sort();
    Ok(findings)
}

/// A digest plane either loaded cleanly or is unavailable (missing
/// or unparseable) — the two failure shapes map to the same
/// single-plane finding, so they share one marker.
pub(crate) enum Plane {
    Loaded(BTreeMap<String, String>),
    Unavailable,
}

/// Read a `path → digest` map from a bundle file. Missing or
/// unparseable yields [`Plane::Unavailable`]; a genuine read error
/// is the only `Err` path.
pub(crate) fn read_digest_map(bundle: &Path, name: &str) -> Result<Plane, ReproductionError> {
    let path = bundle.join(name);
    if !path.exists() {
        return Ok(Plane::Unavailable);
    }
    let bytes = std::fs::read(&path).map_err(|source| ReproductionError::Io {
        path: path.clone(),
        source,
    })?;
    match serde_json::from_slice::<BTreeMap<String, String>>(&bytes) {
        Ok(map) => Ok(Plane::Loaded(map)),
        Err(_) => Ok(Plane::Unavailable),
    }
}

/// Read `deterministic-manifest.json` as a JSON value — shape-
/// agnostic, so manifests from before the recipe fields existed
/// still compare field-by-field (absent fields compare as `null`).
/// Missing or unparseable yields `Ok(None)`.
pub(crate) fn read_recipe(bundle: &Path) -> Result<Option<serde_json::Value>, ReproductionError> {
    let path = bundle.join("deterministic-manifest.json");
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|source| ReproductionError::Io {
        path: path.clone(),
        source,
    })?;
    Ok(serde_json::from_slice::<serde_json::Value>(&bytes).ok())
}

/// `true` for 64-character lowercase hex — the SHA-256 digest shape
/// every recorded input/output digest must have to be verifiable.
fn is_sha256_hex(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Directional key-level diff of two digest planes. Shared by the
/// reproduction comparison (which maps these buckets onto typed
/// findings) and the bundle diff engine (which renders them with
/// direction). Empty on every bucket means plane-equal.
#[derive(Debug, Default)]
pub(crate) struct DigestPlaneDiff {
    /// Keys present on both sides with differing, well-formed
    /// digests.
    pub(crate) changed: Vec<String>,
    /// Keys present only on the baseline side.
    pub(crate) base_only: Vec<String>,
    /// Keys present only on the candidate side.
    pub(crate) cand_only: Vec<String>,
    /// Keys whose recorded digest is not 64-char lowercase hex —
    /// the key cannot be verified on the malformed side.
    pub(crate) unverifiable: Vec<String>,
}

/// Diff two digest planes key by key. A malformed baseline digest
/// is unverifiable before any presence/match question; a candidate-
/// only key is reported without inspecting its digest shape
/// (matching the reproduction comparison's semantics).
pub(crate) fn diff_digest_planes(
    base: &BTreeMap<String, String>,
    cand: &BTreeMap<String, String>,
) -> DigestPlaneDiff {
    let mut diff = DigestPlaneDiff::default();
    for (key, base_digest) in base {
        if !is_sha256_hex(base_digest) {
            diff.unverifiable.push(key.clone());
            continue;
        }
        match cand.get(key) {
            None => diff.base_only.push(key.clone()),
            Some(cand_digest) if cand_digest != base_digest => {
                if is_sha256_hex(cand_digest) {
                    diff.changed.push(key.clone());
                } else {
                    diff.unverifiable.push(key.clone());
                }
            }
            Some(_) => {}
        }
    }
    for key in cand.keys() {
        if !base.contains_key(key) {
            diff.cand_only.push(key.clone());
        }
    }
    diff
}

/// Input plane: canonical source-input digests must be identical.
fn compare_inputs(
    baseline: &Path,
    candidate: &Path,
    findings: &mut Vec<ReproductionFinding>,
) -> Result<(), ReproductionError> {
    let base = read_digest_map(baseline, "inputs_hashes.json")?;
    let cand = read_digest_map(candidate, "inputs_hashes.json")?;
    let (Plane::Loaded(base), Plane::Loaded(cand)) = (&base, &cand) else {
        findings.push(ReproductionFinding::InputUnverifiable {
            path: "inputs_hashes.json".to_string(),
        });
        return Ok(());
    };
    let diff = diff_digest_planes(base, cand);
    for path in diff.changed {
        findings.push(ReproductionFinding::InputChanged { path });
    }
    for path in diff.base_only.into_iter().chain(diff.cand_only) {
        findings.push(ReproductionFinding::InputMissing { path });
    }
    for path in diff.unverifiable {
        findings.push(ReproductionFinding::InputUnverifiable { path });
    }
    Ok(())
}

/// Recipe plane: every recipe field must agree. Absent fields
/// compare as `null`, so a pre-recipe-fields manifest reports
/// exactly the fields the recipe added or changed.
pub(crate) fn compare_recipe_fields(
    base: &serde_json::Value,
    cand: &serde_json::Value,
) -> Vec<&'static str> {
    let mut changed = Vec::new();
    for (key, reported) in RECIPE_FIELDS {
        let base_value = base.get(*key).cloned().unwrap_or(serde_json::Value::Null);
        let cand_value = cand.get(*key).cloned().unwrap_or(serde_json::Value::Null);
        if base_value != cand_value {
            changed.push(*reported);
        }
    }
    changed
}

/// Recipe plane driver for the reproduction comparison: load both
/// manifests and push a finding per differing field.
fn compare_recipe(
    baseline: &Path,
    candidate: &Path,
    findings: &mut Vec<ReproductionFinding>,
) -> Result<(), ReproductionError> {
    let base = read_recipe(baseline)?;
    let cand = read_recipe(candidate)?;
    let (Some(base), Some(cand)) = (&base, &cand) else {
        findings.push(ReproductionFinding::RecipeUnavailable);
        return Ok(());
    };
    for field in compare_recipe_fields(base, cand) {
        findings.push(ReproductionFinding::RecipeFieldChanged { field });
    }
    Ok(())
}

/// Output plane: recorded artifact digests must be identical.
fn compare_outputs(
    baseline: &Path,
    candidate: &Path,
    findings: &mut Vec<ReproductionFinding>,
) -> Result<(), ReproductionError> {
    let base = read_digest_map(baseline, "outputs_hashes.json")?;
    let cand = read_digest_map(candidate, "outputs_hashes.json")?;
    let (Plane::Loaded(base), Plane::Loaded(cand)) = (&base, &cand) else {
        findings.push(ReproductionFinding::OutputUnverifiable {
            artifact: "outputs_hashes.json".to_string(),
        });
        return Ok(());
    };
    let diff = diff_digest_planes(base, cand);
    for artifact in diff.changed {
        findings.push(ReproductionFinding::OutputChanged { artifact });
    }
    for artifact in diff.base_only {
        findings.push(ReproductionFinding::OutputMissing { artifact });
    }
    for artifact in diff.unverifiable {
        findings.push(ReproductionFinding::OutputUnverifiable { artifact });
    }
    for artifact in diff.cand_only {
        findings.push(ReproductionFinding::OutputExtra { artifact });
    }
    Ok(())
}

// Tests live in a sibling file pulled in via `#[path]` so this
// module stays under the workspace 500-line limit.
#[cfg(test)]
#[path = "reproduction/tests.rs"]
mod tests;
