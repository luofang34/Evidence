//! `cargo evidence diff` — a thin renderer over
//! [`compare_bundles`] (LLR-148 / HLR-113).
//!
//! The engine compares two bundles across every assurance-relevant
//! category; this module prints (or serializes) the result. It
//! reports, never judges: the exit code is [`EXIT_SUCCESS`]
//! whether or not differences exist. I/O and parse failures
//! surface through the anyhow error envelope as exit 1.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

use evidence_core::{CategoryDiff, DiffCategoryStatus, EvidenceIndex, compare_bundles};

use super::args::EXIT_SUCCESS;
use super::output::emit_json;

#[derive(Serialize)]
struct EnvFieldChange {
    field: String,
    a: String,
    b: String,
}

/// The JSON envelope for `cargo evidence diff --json`. The
/// authoritative content is `categories`; the four legacy keys are
/// kept populated for the MCP wrapper and existing consumers.
#[derive(Serialize)]
struct DiffOutput {
    bundle_a: String,
    bundle_b: String,
    categories: Vec<CategoryDiff>,
    /// Deprecated: superseded by the `inputs` / `outputs`
    /// categories; kept for back-compat with pre-category
    /// consumers.
    inputs_diff: HashDiff,
    /// Deprecated: see `inputs_diff`.
    outputs_diff: HashDiff,
    /// Deprecated: superseded by the `scope` category.
    metadata_diff: MetadataDiff,
    /// Deprecated: superseded by the `tool_identity` category.
    env_diff: Vec<EnvFieldChange>,
}

#[derive(Serialize, Default)]
struct HashDiff {
    added: Vec<String>,
    removed: Vec<String>,
    changed: Vec<ChangedFile>,
}

#[derive(Serialize)]
struct ChangedFile {
    path: String,
    hash_a: String,
    hash_b: String,
}

#[derive(Serialize, Default)]
struct MetadataDiff {
    profile: Option<StringChange>,
    git_sha: Option<StringChange>,
    git_branch: Option<StringChange>,
    git_dirty: Option<BoolChange>,
}

#[derive(Serialize)]
struct StringChange {
    a: String,
    b: String,
}

#[derive(Serialize)]
struct BoolChange {
    a: bool,
    b: bool,
}

/// `cargo evidence diff` handler: compare two bundles on-disk
/// across every assurance-relevant category and print (or emit as
/// JSON) the per-category delta. Returns [`EXIT_SUCCESS`] even
/// when differences are found — the diff command reports
/// differences, it doesn't judge them. A bundle root that doesn't
/// exist is an operational failure (anyhow error, exit 1).
pub fn cmd_diff(bundle_a: PathBuf, bundle_b: PathBuf, json_output: bool) -> Result<i32> {
    let categories = compare_bundles(&bundle_a, &bundle_b).map_err(|e| anyhow::anyhow!("{e}"))?;

    if json_output {
        // Legacy keys, computed from the same on-disk artifacts
        // the engine read. Best-effort: a missing index or hash
        // file yields an empty legacy section while the categories
        // carry the authoritative Unverifiable status.
        let diff_output = DiffOutput {
            bundle_a: bundle_a.display().to_string(),
            bundle_b: bundle_b.display().to_string(),
            categories,
            inputs_diff: legacy_hash_diff(&bundle_a, &bundle_b, "inputs_hashes.json"),
            outputs_diff: legacy_hash_diff(&bundle_a, &bundle_b, "outputs_hashes.json"),
            metadata_diff: legacy_metadata_diff(&bundle_a, &bundle_b),
            env_diff: compute_env_diff(&bundle_a, &bundle_b),
        };
        emit_json(&diff_output)?;
    } else {
        println!(
            "Comparing bundles:\n  A: {:?}\n  B: {:?}\n",
            bundle_a, bundle_b
        );
        for diff in &categories {
            println!("=== {} === ({})", diff.category, status_label(diff.status));
            for line in &diff.details {
                println!("  {line}");
            }
        }
        // The no-changes marker is only honest when EVERY category
        // compared equal — an unverifiable or changed category must
        // never let the report read as "nothing differs".
        if categories
            .iter()
            .all(|d| d.status == DiffCategoryStatus::Equal)
        {
            println!("\n(no changes)");
        }
    }

    Ok(EXIT_SUCCESS)
}

/// Lowercase status label shared by the human renderer and the
/// JSON wire form (serde snake_case).
fn status_label(status: DiffCategoryStatus) -> &'static str {
    match status {
        DiffCategoryStatus::Equal => "equal",
        DiffCategoryStatus::Added => "added",
        DiffCategoryStatus::Removed => "removed",
        DiffCategoryStatus::Changed => "changed",
        DiffCategoryStatus::Unverifiable => "unverifiable",
    }
}

fn load_index(bundle: &Path) -> Option<EvidenceIndex> {
    let content = fs::read_to_string(bundle.join("index.json")).ok()?;
    serde_json::from_str(&content).ok()
}

fn load_hashes(path: &Path) -> BTreeMap<String, String> {
    if !path.exists() {
        return BTreeMap::new();
    }
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return BTreeMap::new(),
    };
    serde_json::from_str(&content).unwrap_or_default()
}

/// Legacy `inputs_diff` / `outputs_diff` shape for the JSON
/// envelope — the same key-level diff the pre-category command
/// reported, so old consumers keep their fields.
fn legacy_hash_diff(bundle_a: &Path, bundle_b: &Path, plane: &str) -> HashDiff {
    let a = load_hashes(&bundle_a.join(plane));
    let b = load_hashes(&bundle_b.join(plane));
    compute_hash_diff(&a, &b)
}

/// Legacy `metadata_diff` shape. Best-effort: a side whose index
/// doesn't load contributes no change rows (the `scope` category
/// reports the underlying Unverifiable status).
fn legacy_metadata_diff(bundle_a: &Path, bundle_b: &Path) -> MetadataDiff {
    let mut metadata_diff = MetadataDiff::default();
    let (Some(index_a), Some(index_b)) = (load_index(bundle_a), load_index(bundle_b)) else {
        return metadata_diff;
    };

    if index_a.profile != index_b.profile {
        metadata_diff.profile = Some(StringChange {
            a: index_a.profile.to_string(),
            b: index_b.profile.to_string(),
        });
    }
    if index_a.git_sha != index_b.git_sha {
        metadata_diff.git_sha = Some(StringChange {
            a: index_a.git_sha.clone(),
            b: index_b.git_sha.clone(),
        });
    }
    if index_a.git_branch != index_b.git_branch {
        metadata_diff.git_branch = Some(StringChange {
            a: index_a.git_branch.clone(),
            b: index_b.git_branch.clone(),
        });
    }
    if index_a.git_dirty != index_b.git_dirty {
        metadata_diff.git_dirty = Some(BoolChange {
            a: index_a.git_dirty,
            b: index_b.git_dirty,
        });
    }
    metadata_diff
}

fn compute_hash_diff(a: &BTreeMap<String, String>, b: &BTreeMap<String, String>) -> HashDiff {
    let mut diff = HashDiff::default();

    for key in a.keys() {
        if !b.contains_key(key) {
            diff.removed.push(key.clone());
        }
    }
    for key in b.keys() {
        if !a.contains_key(key) {
            diff.added.push(key.clone());
        }
    }
    for (key, hash_a) in a {
        if let Some(hash_b) = b.get(key) {
            if hash_a != hash_b {
                diff.changed.push(ChangedFile {
                    path: key.clone(),
                    hash_a: hash_a.clone(),
                    hash_b: hash_b.clone(),
                });
            }
        }
    }

    diff
}

/// Compare env.json from two bundles, returning field-level
/// differences. Skips git fields (profile, git_sha, git_branch,
/// git_dirty) which the `scope` category already covers.
fn compute_env_diff(bundle_a: &Path, bundle_b: &Path) -> Vec<EnvFieldChange> {
    let skip = ["profile", "git_sha", "git_branch", "git_dirty"];
    let load = |p: &Path| -> Option<serde_json::Map<String, serde_json::Value>> {
        let content = fs::read_to_string(p.join("env.json")).ok()?;
        let v: serde_json::Value = serde_json::from_str(&content).ok()?;
        v.as_object().cloned()
    };
    let (Some(obj_a), Some(obj_b)) = (load(bundle_a), load(bundle_b)) else {
        return Vec::new();
    };
    let all_keys: std::collections::BTreeSet<_> = obj_a.keys().chain(obj_b.keys()).collect();
    let mut changes = Vec::new();
    for key in all_keys {
        if skip.contains(&key.as_str()) {
            continue;
        }
        let val_a = obj_a.get(key).map(|v| v.to_string()).unwrap_or_default();
        let val_b = obj_b.get(key).map(|v| v.to_string()).unwrap_or_default();
        if val_a != val_b {
            changes.push(EnvFieldChange {
                field: key.clone(),
                a: val_a,
                b: val_b,
            });
        }
    }
    changes
}
