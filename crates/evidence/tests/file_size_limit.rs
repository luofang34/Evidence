//! Guardrail: every `.rs` file in the workspace must stay under
//! [`LIMIT`] lines.
//!
//! CLAUDE.md sets the limit at 500 lines per `.rs` file — split into
//! sub-modules when exceeded. The limit is a pushback against files
//! that accumulate unrelated concerns; three-concern files at 300
//! lines are cheaper to navigate and review than a 900-line file
//! that happens to compile.
//!
//! This test walks `crates/` and fails if any file exceeds the
//! limit. If it goes red, the options are:
//!
//!   1. Split the offending file into sibling `.rs` files under a
//!      new directory (see the trace / bundle / verify / compliance
//!      modules for the pattern this project uses). Preferred.
//!   2. If the file is a third-party-generated artifact or otherwise
//!      genuinely can't be split, add an entry to [`ALLOWLIST`]
//!      below with a comment explaining why and a follow-up plan.
//!
//! Do not bump `LIMIT` to pass the test. The point is to feel
//! friction when a single file grows past the threshold, not to let
//! the threshold drift upward one PR at a time.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::{Path, PathBuf};

/// Maximum allowed line count per `.rs` file.
const LIMIT: usize = 500;

/// Files exempted from the size limit with explicit justification.
///
/// Paths are relative to the workspace root and use forward slashes.
/// Every entry must have a comment naming the follow-up PR or
/// reason; empty this list out as entries get resolved.
const ALLOWLIST: &[(&str, &str)] = &[
    // `tests/integration.rs` (~1240 lines) is a single Cargo test
    // binary holding 29 cross-concern integration tests against the
    // public API. Every `.rs` file directly in `tests/` compiles to
    // its own test binary, so splitting requires either tolerating
    // per-concern test binaries (slow compile) or introducing a
    // shared `mod common;` pattern that conflicts with CLAUDE.md's
    // "no mod.rs" rule. Scheduled for a dedicated refactor PR once
    // the split strategy is agreed on.
    ("crates/evidence/tests/integration.rs", "pending split PR"),
    // `env.rs` (~544 lines) is at ~9% over the limit. The module
    // decomposes cleanly into host detection / EnvFingerprint /
    // DeterministicManifest / capture, but the split is mechanical
    // and orthogonal to the compliance.rs work in the same PR that
    // introduced this guardrail. Land guardrail first, split in a
    // follow-up.
    ("crates/evidence/src/env.rs", "pending split PR"),
    // `policy.rs` (~554 lines) — same story as env.rs. Profile /
    // BoundaryConfig / Dal / EvidencePolicy / TracePolicy are clear
    // split lines but belong in a separate PR.
    ("crates/evidence/src/policy.rs", "pending split PR"),
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn is_allowlisted(rel: &str) -> Option<&'static str> {
    ALLOWLIST
        .iter()
        .find(|(path, _)| *path == rel)
        .map(|(_, reason)| *reason)
}

/// Recursively walk `dir`, returning every regular `.rs` file beneath it.
fn walk_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip `target/` so we don't descend into cargo's build
            // output (vendored crate sources, generated bindings, ...).
            if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            walk_rs_files(&path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn count_lines(path: &Path) -> usize {
    fs::read_to_string(path)
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

#[test]
fn rs_files_under_line_limit() {
    let root = workspace_root();
    let crates_dir = root.join("crates");
    assert!(
        crates_dir.is_dir(),
        "crates/ directory not found at {:?}",
        crates_dir
    );

    let mut rs_files = Vec::new();
    walk_rs_files(&crates_dir, &mut rs_files);
    rs_files.sort();
    assert!(!rs_files.is_empty(), "found no .rs files under crates/");

    let mut offenders: Vec<(String, usize)> = Vec::new();
    let mut stale_allowlist: Vec<String> = Vec::new();

    for path in &rs_files {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let lines = count_lines(path);

        if is_allowlisted(&rel).is_some() {
            if lines <= LIMIT {
                // Allowlist entry no longer needed — file is small
                // enough to stand on its own. Flag it so the list
                // doesn't silently grow stale.
                stale_allowlist.push(rel);
            }
            continue;
        }

        if lines > LIMIT {
            offenders.push((rel, lines));
        }
    }

    let mut msg = String::new();
    if !offenders.is_empty() {
        msg.push_str(&format!(
            "\n{} .rs file(s) exceed {} lines:\n",
            offenders.len(),
            LIMIT
        ));
        for (rel, lines) in &offenders {
            msg.push_str(&format!("  {}: {} lines\n", rel, lines));
        }
        msg.push_str(
            "\nFix by splitting into sibling `.rs` files under a new directory \
             (see `crates/evidence/src/{trace,bundle,verify,compliance}/` for the pattern). \
             Only add an entry to ALLOWLIST in this test if splitting is genuinely \
             impractical — and document why in the comment next to the entry.\n",
        );
    }
    if !stale_allowlist.is_empty() {
        msg.push_str(
            "\nStale ALLOWLIST entries (file is now under the limit, remove the \
             allowlist row in `tests/file_size_limit.rs`):\n",
        );
        for rel in &stale_allowlist {
            msg.push_str(&format!("  {}\n", rel));
        }
    }

    assert!(msg.is_empty(), "{}", msg);
}

#[test]
fn allowlist_paths_exist() {
    // An allowlist entry for a path that doesn't exist is a silent
    // foot-gun: it papers over nothing and implies we're exempting
    // something we're not. Require every allowlisted path to
    // actually resolve.
    let root = workspace_root();
    for (rel, _reason) in ALLOWLIST {
        let p = root.join(rel);
        assert!(
            p.exists(),
            "ALLOWLIST entry {} points at a non-existent file; remove the entry.",
            rel
        );
    }
}
