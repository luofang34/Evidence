//! Lock test: `.github/workflows/*.yml` must not pin deprecated
//! action versions.
//!
//! GitHub forces Node.js 20 actions to run on Node 24 starting
//! 2026-06-02 and removes Node 20 entirely on 2026-09-16. This
//! test fires if a PR reintroduces a pre-Node-24 pin on any
//! first-party `actions/*` action or on the two
//! `DeterminateSystems/*` actions this project uses. Add to
//! `BANNED_PINS` when a new deprecation lands; remove when the
//! corresponding pin is safe again.
//!
//! Mechanical-guardrail style (no `Diagnostic`, no `RULES`
//! entry — mirrors `schema_versions_locked` / `walker_usage_locked`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::PathBuf;

/// `(pin, reason)` pairs. The grep is substring-based: any file
/// containing the `pin` string fires the gate. Reasons are
/// surfaced in the failure message so reviewers see the upgrade
/// rationale without having to remember the deprecation calendar.
const BANNED_PINS: &[(&str, &str)] = &[
    (
        "actions/checkout@v4",
        "Node 20 — bump to @v5 or later (Node 24)",
    ),
    (
        "actions/cache@v4",
        "Node 20 — bump to @v5 or later (Node 24)",
    ),
    (
        "actions/upload-artifact@v4",
        "Node 20 — bump to @v6 or later (v5 still targets Node 20)",
    ),
    (
        "actions/upload-artifact@v5",
        "Node 20 — bump to @v6 or later",
    ),
    (
        "actions/download-artifact@v4",
        "Node 20 — bump to @v7 or later (v5 and v6 target Node 20)",
    ),
    (
        "actions/download-artifact@v5",
        "Node 20 — bump to @v7 or later",
    ),
    (
        "actions/download-artifact@v6",
        "Node 20 — bump to @v7 or later",
    ),
    (
        "DeterminateSystems/nix-installer-action@v14",
        "Node 20 — bump to @v22 or later",
    ),
    (
        "DeterminateSystems/magic-nix-cache-action@main",
        "pin to a released tag (e.g. @v13) for reproducibility",
    ),
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn no_deprecated_action_versions_in_workflows() {
    let root = workspace_root();
    let workflows = root.join(".github").join("workflows");
    let entries = fs::read_dir(&workflows).expect("read .github/workflows");
    let mut hits: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.expect("dirent");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yml") {
            continue;
        }
        let body = fs::read_to_string(&path).expect("read workflow");
        let rel = path.strip_prefix(&root).unwrap_or(&path);
        for (line_idx, line) in body.lines().enumerate() {
            let trimmed = line.trim_start();
            // Skip comment lines so the TEST file's own docstring
            // references don't fire the gate when this file's
            // source is distributed alongside a workflow clone.
            if trimmed.starts_with('#') {
                continue;
            }
            for (pin, reason) in BANNED_PINS {
                if line.contains(pin) {
                    hits.push(format!(
                        "{}:{}  {}  (reason: {})",
                        rel.display(),
                        line_idx + 1,
                        pin,
                        reason
                    ));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "deprecated action pin(s) found in .github/workflows/*.yml. \
         GitHub removes Node.js 20 on 2026-09-16; bump each pin to \
         the Node-24 replacement listed in the reason. If a pin is \
         genuinely load-bearing, remove it from BANNED_PINS with \
         written justification.\n\nHits:\n  {}",
        hits.join("\n  ")
    );
}
