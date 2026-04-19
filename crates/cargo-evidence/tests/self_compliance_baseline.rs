//! Self-compliance baseline gate (PR #47 / LLR-034 / TEST-034).
//!
//! Runs `cargo evidence generate --skip-tests` on the tool's own
//! workspace and diffs the produced `compliance/*.json` against the
//! committed baseline at `cert/baselines/self_compliance.json`. Fails
//! with the exact `(crate, objective_id, old_status, new_status)`
//! tuple that drifted.
//!
//! **Why a baseline, not a strict filter.** The original plan called
//! for asserting every objective is `Met` or `ManualReviewRequired`.
//! That filter would fail on PR #47's first CI run — many traceability
//! objectives sit at `Partial` today because the current compliance
//! logic conservatively requires manual review + supporting coverage
//! we don't yet ship. The baseline approach instead *locks* the
//! current state: any tool change that flips a status worse (e.g.
//! `ManualReviewRequired → NotMet`) fires here. Status upgrades land
//! as explicit PRs that edit the baseline with written justification
//! in the PR body.
//!
//! **Regenerating the baseline.** Expected in two cases:
//!
//! 1. A tool change legitimately upgrades objectives — desired.
//!    Update the baseline in the same PR with the upgrade; the PR
//!    body explains why.
//! 2. Compliance semantics changed (new objective added, status
//!    values evolved). Same workflow, just clearly document.
//!
//! Update helper: run this test, copy the printed baseline JSON
//! into `cert/baselines/self_compliance.json`, re-run.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

/// Run `cargo evidence generate --skip-tests --out-dir <tmp>` on the
/// tool's own workspace and return the produced
/// `{crate → {objective_id → status}}` map. Skips tests to avoid the
/// cargo-test-inside-cargo-test target-dir contention trap.
fn generate_and_collect_statuses(tmp: &Path) -> BTreeMap<String, BTreeMap<String, String>> {
    let out = cargo_evidence()
        .current_dir(workspace_root())
        .args([
            "evidence",
            "generate",
            "--skip-tests",
            "--profile",
            "dev",
            "--out-dir",
        ])
        .arg(tmp)
        .output()
        .expect("spawn `cargo evidence generate`");
    assert!(
        out.status.success(),
        "generate failed: stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // `generate --out-dir` produces exactly one bundle dir under `tmp`.
    let bundle = std::fs::read_dir(tmp)
        .expect("read tmp")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_dir())
        .expect("bundle dir produced");

    let compliance = bundle.join("compliance");
    assert!(
        compliance.is_dir(),
        "no compliance/ dir in bundle at {}",
        bundle.display()
    );

    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&compliance)
        .expect("read compliance/")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    entries.sort();

    for path in &entries {
        let text = std::fs::read_to_string(path).expect("read compliance file");
        let v: Value = serde_json::from_str(&text).expect("compliance json parses");
        let crate_name = v["crate_name"].as_str().expect("crate_name").to_string();
        let mut statuses: BTreeMap<String, String> = BTreeMap::new();
        for obj in v["objectives"].as_array().expect("objectives array") {
            let id = obj["objective_id"]
                .as_str()
                .expect("objective_id")
                .to_string();
            let status = obj["status"].as_str().expect("status").to_string();
            statuses.insert(id, status);
        }
        out.insert(crate_name, statuses);
    }
    out
}

#[test]
fn baseline_matches_current_generation() {
    let baseline_path = workspace_root()
        .join("cert")
        .join("baselines")
        .join("self_compliance.json");
    let baseline_text = std::fs::read_to_string(&baseline_path).unwrap_or_else(|e| {
        panic!(
            "baseline missing at {}: {}. Run `cargo test baseline_matches_current_generation \
             -- --nocapture` and commit the printed baseline to this path.",
            baseline_path.display(),
            e
        )
    });
    let baseline: BTreeMap<String, BTreeMap<String, String>> =
        serde_json::from_str(&baseline_text).expect("baseline is valid JSON");

    let tmp = TempDir::new().expect("tempdir");
    let current = generate_and_collect_statuses(tmp.path());

    if current != baseline {
        let mut drifts: Vec<String> = Vec::new();

        // Crates only in baseline (deleted).
        for crate_name in baseline.keys() {
            if !current.contains_key(crate_name) {
                drifts.push(format!("  REMOVED crate: {}", crate_name));
            }
        }
        // Crates only in current (added).
        for crate_name in current.keys() {
            if !baseline.contains_key(crate_name) {
                drifts.push(format!("  ADDED crate: {}", crate_name));
            }
        }
        // Per-crate: objective-level diffs.
        for (crate_name, current_obs) in &current {
            let Some(baseline_obs) = baseline.get(crate_name) else {
                continue;
            };
            for (obj_id, current_status) in current_obs {
                match baseline_obs.get(obj_id) {
                    None => drifts.push(format!(
                        "  ADDED: {} / {} = {}",
                        crate_name, obj_id, current_status
                    )),
                    Some(baseline_status) if baseline_status != current_status => {
                        drifts.push(format!(
                            "  CHANGED: {} / {}: {} -> {}",
                            crate_name, obj_id, baseline_status, current_status
                        ))
                    }
                    _ => {}
                }
            }
            for obj_id in baseline_obs.keys() {
                if !current_obs.contains_key(obj_id) {
                    drifts.push(format!("  REMOVED: {} / {}", crate_name, obj_id));
                }
            }
        }

        // Emit the full current map as a copy-pasteable replacement
        // for the baseline, matching the regeneration workflow
        // documented in the module header.
        let pretty = serde_json::to_string_pretty(&current).unwrap();

        panic!(
            "self-compliance baseline drift ({} change(s)):\n{}\n\n\
             If the drift is intentional (status upgrade or schema change), \
             replace cert/baselines/self_compliance.json with:\n{}\n",
            drifts.len(),
            drifts.join("\n"),
            pretty
        );
    }
}
