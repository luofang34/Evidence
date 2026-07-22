//! Tests for `compare_reproduction` — fixture builders + the
//! per-plane finding taxonomy (TEST-163).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use super::*;
use serde_json::{Value, json};
use std::fs;
use tempfile::TempDir;

/// A 64-char lowercase-hex digest made of one repeated hex char.
/// Only hex characters are valid inputs — anything else is the
/// malformed-digest fixture.
fn digest(ch: &str) -> String {
    ch.repeat(64)
}

fn canonical_manifest() -> Value {
    json!({
        "schema_version": crate::schema_versions::DETERMINISTIC_MANIFEST,
        "profile": "dev",
        "rustc": "rustc 1.95.0 (abc)",
        "cargo": "cargo 1.95.0 (abc)",
        "llvm_version": "20.0.0",
        "cargo_lock_hash": digest("1"),
        "rust_toolchain_toml": "[toolchain]\nchannel = \"1.95\"\n",
        "rustflags": "-D warnings",
        "target_triple": "x86_64-unknown-linux-gnu",
        "features": [],
        "locked_graph_hash": digest("2"),
        "command_recipe_hash": digest("3"),
        "inputs_hash": digest("4"),
        "resolution_policy": "locked_offline",
        "git_sha": "a".repeat(40),
        "git_branch": "main",
        "git_dirty": false,
    })
}

fn canonical_inputs() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("src/lib.rs".to_string(), digest("a")),
        ("src/main.rs".to_string(), digest("b")),
    ])
}

fn canonical_outputs() -> BTreeMap<String, String> {
    BTreeMap::from([("target/release/tool".to_string(), digest("c"))])
}

fn write_bundle(
    dir: &Path,
    manifest: &Value,
    inputs: &BTreeMap<String, String>,
    outputs: &BTreeMap<String, String>,
) {
    fs::write(
        dir.join("deterministic-manifest.json"),
        serde_json::to_vec_pretty(manifest).unwrap(),
    )
    .unwrap();
    fs::write(
        dir.join("inputs_hashes.json"),
        serde_json::to_vec_pretty(inputs).unwrap(),
    )
    .unwrap();
    fs::write(
        dir.join("outputs_hashes.json"),
        serde_json::to_vec_pretty(outputs).unwrap(),
    )
    .unwrap();
}

/// Two tempdir bundles written byte-identical.
struct Pair {
    baseline: TempDir,
    candidate: TempDir,
}

fn identical_pair() -> Pair {
    let baseline = TempDir::new().unwrap();
    let candidate = TempDir::new().unwrap();
    let manifest = canonical_manifest();
    let inputs = canonical_inputs();
    let outputs = canonical_outputs();
    write_bundle(baseline.path(), &manifest, &inputs, &outputs);
    write_bundle(candidate.path(), &manifest, &inputs, &outputs);
    Pair {
        baseline,
        candidate,
    }
}

impl Pair {
    fn compare(&self) -> Vec<ReproductionFinding> {
        compare_reproduction(self.baseline.path(), self.candidate.path()).expect("comparison runs")
    }

    fn mutate_manifest(&self, mutate: impl FnOnce(&mut Value)) {
        let mut m = canonical_manifest();
        mutate(&mut m);
        fs::write(
            self.candidate.path().join("deterministic-manifest.json"),
            serde_json::to_vec_pretty(&m).unwrap(),
        )
        .unwrap();
    }

    fn mutate_inputs(&self, mutate: impl FnOnce(&mut BTreeMap<String, String>)) {
        let mut m = canonical_inputs();
        mutate(&mut m);
        fs::write(
            self.candidate.path().join("inputs_hashes.json"),
            serde_json::to_vec_pretty(&m).unwrap(),
        )
        .unwrap();
    }

    fn mutate_outputs(&self, mutate: impl FnOnce(&mut BTreeMap<String, String>)) {
        let mut m = canonical_outputs();
        mutate(&mut m);
        fs::write(
            self.candidate.path().join("outputs_hashes.json"),
            serde_json::to_vec_pretty(&m).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn identical_bundles_compare_equal() {
    let pair = identical_pair();
    assert_eq!(pair.compare(), Vec::new());
}

#[test]
fn changed_target_triple_yields_recipe_field_finding() {
    let pair = identical_pair();
    pair.mutate_manifest(|m| m["target_triple"] = json!("aarch64-apple-darwin"));
    assert_eq!(
        pair.compare(),
        vec![ReproductionFinding::RecipeFieldChanged {
            field: "target_triple"
        }]
    );
}

#[test]
fn changed_features_yields_recipe_field_finding() {
    let pair = identical_pair();
    pair.mutate_manifest(|m| m["features"] = json!(["serde/derive"]));
    assert_eq!(
        pair.compare(),
        vec![ReproductionFinding::RecipeFieldChanged { field: "features" }]
    );
}

#[test]
fn changed_rustflags_yields_recipe_field_finding() {
    let pair = identical_pair();
    pair.mutate_manifest(|m| m["rustflags"] = json!("-C opt-level=2"));
    assert_eq!(
        pair.compare(),
        vec![ReproductionFinding::RecipeFieldChanged { field: "rustflags" }]
    );
}

#[test]
fn changed_cargo_lock_yields_dependency_lock_finding() {
    let pair = identical_pair();
    pair.mutate_manifest(|m| m["cargo_lock_hash"] = json!(digest("9")));
    assert_eq!(
        pair.compare(),
        vec![ReproductionFinding::RecipeFieldChanged {
            field: "dependency_lock"
        }]
    );
}

#[test]
fn changed_inputs_hash_yields_recipe_field_finding() {
    let pair = identical_pair();
    pair.mutate_manifest(|m| m["inputs_hash"] = json!(digest("9")));
    assert_eq!(
        pair.compare(),
        vec![ReproductionFinding::RecipeFieldChanged {
            field: "inputs_hash"
        }]
    );
}

#[test]
fn changed_command_recipe_hash_yields_recipe_field_finding() {
    let pair = identical_pair();
    pair.mutate_manifest(|m| m["command_recipe_hash"] = json!(digest("9")));
    assert_eq!(
        pair.compare(),
        vec![ReproductionFinding::RecipeFieldChanged {
            field: "command_recipe_hash"
        }]
    );
}

#[test]
fn changed_input_digest_yields_input_changed() {
    let pair = identical_pair();
    pair.mutate_inputs(|m| {
        m.insert("src/lib.rs".to_string(), digest("9"));
    });
    assert_eq!(
        pair.compare(),
        vec![ReproductionFinding::InputChanged {
            path: "src/lib.rs".to_string()
        }]
    );
}

#[test]
fn changed_output_digest_yields_output_changed() {
    let pair = identical_pair();
    pair.mutate_outputs(|m| {
        m.insert("target/release/tool".to_string(), digest("9"));
    });
    assert_eq!(
        pair.compare(),
        vec![ReproductionFinding::OutputChanged {
            artifact: "target/release/tool".to_string()
        }]
    );
}

#[test]
fn removed_output_yields_output_missing() {
    let pair = identical_pair();
    pair.mutate_outputs(|m| {
        m.remove("target/release/tool");
    });
    assert_eq!(
        pair.compare(),
        vec![ReproductionFinding::OutputMissing {
            artifact: "target/release/tool".to_string()
        }]
    );
}

#[test]
fn added_output_yields_output_extra() {
    let pair = identical_pair();
    pair.mutate_outputs(|m| {
        m.insert("target/release/extra".to_string(), digest("d"));
    });
    assert_eq!(
        pair.compare(),
        vec![ReproductionFinding::OutputExtra {
            artifact: "target/release/extra".to_string()
        }]
    );
}

#[test]
fn malformed_input_digest_yields_input_unverifiable() {
    let pair = identical_pair();
    pair.mutate_inputs(|m| {
        m.insert("src/lib.rs".to_string(), "not-a-sha256".to_string());
    });
    assert_eq!(
        pair.compare(),
        vec![ReproductionFinding::InputUnverifiable {
            path: "src/lib.rs".to_string()
        }]
    );
}

#[test]
fn missing_inputs_file_yields_input_unverifiable() {
    let pair = identical_pair();
    fs::remove_file(pair.candidate.path().join("inputs_hashes.json")).unwrap();
    assert_eq!(
        pair.compare(),
        vec![ReproductionFinding::InputUnverifiable {
            path: "inputs_hashes.json".to_string()
        }]
    );
}

#[test]
fn missing_outputs_file_yields_output_unverifiable() {
    let pair = identical_pair();
    fs::remove_file(pair.candidate.path().join("outputs_hashes.json")).unwrap();
    assert_eq!(
        pair.compare(),
        vec![ReproductionFinding::OutputUnverifiable {
            artifact: "outputs_hashes.json".to_string()
        }]
    );
}

#[test]
fn missing_manifest_yields_recipe_unavailable() {
    // Missing manifest on the candidate side.
    let pair = identical_pair();
    fs::remove_file(pair.candidate.path().join("deterministic-manifest.json")).unwrap();
    assert_eq!(pair.compare(), vec![ReproductionFinding::RecipeUnavailable]);

    // Unparseable manifest on the baseline side is the same
    // non-success finding — the recipe plane cannot be compared.
    let pair = identical_pair();
    fs::write(
        pair.baseline.path().join("deterministic-manifest.json"),
        b"not json {{{",
    )
    .unwrap();
    assert_eq!(pair.compare(), vec![ReproductionFinding::RecipeUnavailable]);
}

#[test]
fn findings_sort_deterministically() {
    let pair = identical_pair();
    pair.mutate_manifest(|m| {
        m["target_triple"] = json!("aarch64-apple-darwin");
        m["profile"] = json!("cert");
    });
    pair.mutate_inputs(|m| {
        m.insert("src/lib.rs".to_string(), digest("9"));
    });
    pair.mutate_outputs(|m| {
        m.remove("target/release/tool");
        m.insert("target/release/extra".to_string(), digest("d"));
    });
    assert_eq!(
        pair.compare(),
        vec![
            ReproductionFinding::InputChanged {
                path: "src/lib.rs".to_string()
            },
            ReproductionFinding::RecipeFieldChanged { field: "profile" },
            ReproductionFinding::RecipeFieldChanged {
                field: "target_triple"
            },
            ReproductionFinding::OutputMissing {
                artifact: "target/release/tool".to_string()
            },
            ReproductionFinding::OutputExtra {
                artifact: "target/release/extra".to_string()
            },
        ]
    );
}

#[test]
fn git_identity_is_excluded_from_the_recipe_plane() {
    let pair = identical_pair();
    pair.mutate_manifest(|m| {
        m["git_sha"] = json!("b".repeat(40));
        m["git_branch"] = json!("topic");
        m["git_dirty"] = json!(true);
    });
    assert_eq!(
        pair.compare(),
        Vec::new(),
        "git metadata must not move the reproduction comparison"
    );
}

#[test]
fn missing_bundle_directory_is_an_error_not_a_finding() {
    let pair = identical_pair();
    let gone = pair.baseline.path().join("does-not-exist");
    let outcome = compare_reproduction(&gone, pair.candidate.path());
    assert!(
        matches!(outcome, Err(ReproductionError::BundleNotFound(_))),
        "expected BundleNotFound, got {outcome:?}"
    );
}
