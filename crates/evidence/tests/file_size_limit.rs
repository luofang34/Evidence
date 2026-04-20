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
//!
//! Each allowlist entry carries a **ceiling** (its line count at the
//! time of exemption). The file is allowed to stay above `LIMIT`
//! but must not creep above its own ceiling — otherwise the
//! exemption would silently expand over time. When the file drops
//! back under `LIMIT` the entry becomes stale and the existing
//! staleness check fires; when it grows past its ceiling the new
//! over-ceiling check fires. Both force a PR conversation rather
//! than letting allowlist rot accumulate.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::{Path, PathBuf};

#[path = "walker_helpers.rs"]
mod traversal;

/// Maximum allowed line count per `.rs` file.
const LIMIT: usize = 500;

/// Files exempted from the size limit with explicit justification.
///
/// Tuple is `(path, ceiling, reason)`:
///
/// - **path**: relative to the workspace root, forward slashes.
/// - **ceiling**: the file's line count when the entry was added.
///   The file is allowed to stay above [`LIMIT`] but must not grow
///   past this value — a PR that pushes the file over its ceiling
///   fails this test and has to either split the file or raise the
///   ceiling explicitly (forcing the reviewer to see it).
/// - **reason**: brief note on why the file can't be split right now
///   and what the follow-up plan is. Empty this list out as entries
///   get resolved.
const ALLOWLIST: &[(&str, usize, &str)] = &[];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn allowlist_entry(rel: &str) -> Option<(usize, &'static str)> {
    ALLOWLIST
        .iter()
        .find(|(path, _, _)| *path == rel)
        .map(|(_, ceiling, reason)| (*ceiling, *reason))
}

/// Walk `dir` and return every regular `.rs` file beneath it. Skips
/// `target/` (cargo's build output: vendored sources, generated
/// bindings).
fn walk_rs_files(dir: &Path) -> Vec<PathBuf> {
    traversal::walk(dir)
        .filter_entry(|e| !traversal::is_dir_named(e, &["target"]))
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && traversal::has_ext(e.path(), "rs"))
        .map(|e| e.into_path())
        .collect()
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

    let mut rs_files = walk_rs_files(&crates_dir);
    rs_files.sort();
    assert!(!rs_files.is_empty(), "found no .rs files under crates/");

    let mut offenders: Vec<(String, usize)> = Vec::new();
    let mut stale_allowlist: Vec<String> = Vec::new();
    let mut over_ceiling: Vec<(String, usize, usize)> = Vec::new();

    for path in &rs_files {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let lines = count_lines(path);

        if let Some((ceiling, _reason)) = allowlist_entry(&rel) {
            if lines <= LIMIT {
                // Allowlist entry no longer needed — file is small
                // enough to stand on its own. Flag it so the list
                // doesn't silently grow stale.
                stale_allowlist.push(rel);
            } else if lines > ceiling {
                // The file is allowlisted but has grown past the
                // ceiling recorded at exemption time. The original
                // weakness of this test was that an allowlisted
                // file could creep upward indefinitely; this branch
                // closes that gap.
                over_ceiling.push((rel, lines, ceiling));
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
    if !over_ceiling.is_empty() {
        msg.push_str(
            "\nALLOWLIST entries whose files grew past their recorded ceiling \
             (split the file, or raise the ceiling explicitly in \
             `tests/file_size_limit.rs` so the increase is reviewed):\n",
        );
        for (rel, lines, ceiling) in &over_ceiling {
            msg.push_str(&format!(
                "  {}: {} lines (ceiling {})\n",
                rel, lines, ceiling
            ));
        }
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
    for (rel, _ceiling, _reason) in ALLOWLIST {
        let p = root.join(rel);
        assert!(
            p.exists(),
            "ALLOWLIST entry {} points at a non-existent file; remove the entry.",
            rel
        );
    }
}
