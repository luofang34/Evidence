//! `cargo evidence diff`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use evidence::EvidenceIndex;

use super::args::EXIT_SUCCESS;
use super::output::emit_json;

#[derive(Serialize)]
struct EnvFieldChange {
    field: String,
    a: String,
    b: String,
}

#[derive(Serialize)]
struct DiffOutput {
    bundle_a: String,
    bundle_b: String,
    inputs_diff: HashDiff,
    outputs_diff: HashDiff,
    metadata_diff: MetadataDiff,
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

pub fn cmd_diff(bundle_a: PathBuf, bundle_b: PathBuf, json_output: bool) -> Result<i32> {
    // Load both indexes
    let index_a = load_index(&bundle_a)?;
    let index_b = load_index(&bundle_b)?;

    // Load hash files
    let inputs_a = load_hashes(&bundle_a.join("inputs_hashes.json"))?;
    let inputs_b = load_hashes(&bundle_b.join("inputs_hashes.json"))?;
    let outputs_a = load_hashes(&bundle_a.join("outputs_hashes.json"))?;
    let outputs_b = load_hashes(&bundle_b.join("outputs_hashes.json"))?;

    // Compute diffs
    let inputs_diff = compute_hash_diff(&inputs_a, &inputs_b);
    let outputs_diff = compute_hash_diff(&outputs_a, &outputs_b);

    // Compute metadata diff
    let mut metadata_diff = MetadataDiff::default();

    if index_a.profile != index_b.profile {
        metadata_diff.profile = Some(StringChange {
            a: index_a.profile.clone(),
            b: index_b.profile.clone(),
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

    // Compare env.json (toolchain, platform, flags — skip git fields already in metadata)
    let env_diff = compute_env_diff(&bundle_a, &bundle_b);

    let diff_output = DiffOutput {
        bundle_a: bundle_a.display().to_string(),
        bundle_b: bundle_b.display().to_string(),
        inputs_diff,
        outputs_diff,
        metadata_diff,
        env_diff,
    };

    if json_output {
        emit_json(&diff_output)?;
    } else {
        println!(
            "Comparing bundles:\n  A: {:?}\n  B: {:?}\n",
            bundle_a, bundle_b
        );

        // Metadata changes
        println!("=== Metadata ===");
        if let Some(ref c) = diff_output.metadata_diff.profile {
            println!("  profile: {} -> {}", c.a, c.b);
        }
        if let Some(ref c) = diff_output.metadata_diff.git_sha {
            println!(
                "  git_sha: {}... -> {}...",
                &c.a[..8.min(c.a.len())],
                &c.b[..8.min(c.b.len())]
            );
        }
        if let Some(ref c) = diff_output.metadata_diff.git_branch {
            println!("  git_branch: {} -> {}", c.a, c.b);
        }
        if let Some(ref c) = diff_output.metadata_diff.git_dirty {
            println!("  git_dirty: {} -> {}", c.a, c.b);
        }

        // Environment diff
        if !diff_output.env_diff.is_empty() {
            println!("\n=== Environment ===");
            for c in &diff_output.env_diff {
                println!("  {}: {} -> {}", c.field, c.a, c.b);
            }
        }

        // Inputs diff
        println!("\n=== Inputs ===");
        print_hash_diff(&diff_output.inputs_diff);

        // Outputs diff
        println!("\n=== Outputs ===");
        print_hash_diff(&diff_output.outputs_diff);
    }

    Ok(EXIT_SUCCESS)
}

fn load_index(bundle: &Path) -> Result<EvidenceIndex> {
    let path = bundle.join("index.json");
    let content = fs::read_to_string(&path).with_context(|| format!("reading {:?}", path))?;
    serde_json::from_str(&content).with_context(|| "parsing index.json")
}

fn load_hashes(path: &Path) -> Result<BTreeMap<String, String>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let content = fs::read_to_string(path).with_context(|| format!("reading {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("parsing {:?}", path))
}

fn compute_hash_diff(a: &BTreeMap<String, String>, b: &BTreeMap<String, String>) -> HashDiff {
    let mut diff = HashDiff::default();

    // Files in A but not in B (removed)
    for key in a.keys() {
        if !b.contains_key(key) {
            diff.removed.push(key.clone());
        }
    }

    // Files in B but not in A (added)
    for key in b.keys() {
        if !a.contains_key(key) {
            diff.added.push(key.clone());
        }
    }

    // Files in both but with different hashes
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

fn print_hash_diff(diff: &HashDiff) {
    if diff.added.is_empty() && diff.removed.is_empty() && diff.changed.is_empty() {
        println!("  (no changes)");
        return;
    }

    for f in &diff.added {
        println!("  + {}", f);
    }
    for f in &diff.removed {
        println!("  - {}", f);
    }
    for f in &diff.changed {
        println!("  ~ {} (hash changed)", f.path);
    }
}

/// Compare env.json from two bundles, returning field-level differences.
/// Skips git fields (profile, git_sha, git_branch, git_dirty) which are
/// already covered by metadata_diff.
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
