//! Mechanical guard against editor-duplicate filename artifacts
//! (LLR-076).
//!
//! Walks the workspace and fires on any path whose basename matches
//! the regex:
//!
//! ```text
//! ^.+ ([0-9]{1,2})\.(rs|toml|yml|yaml|md|json|lock)$
//! ```
//!
//! These are the artifacts a misbehaving `cp old new` or save-as
//! dialog leaves behind — `helpers 2.rs`, `audit 2.yml`, `tests
//! 2.toml`, etc. They compile, hash, and ratchet floor counts
//! silently; only a manual sweep retroactively catches them. A
//! pre-1.0 cert-track project wants this enforcement mechanical.
//!
//! Test failure renders one line per offender (`<workspace-relative
//! path>:<line 1>` — line is always 1 because the offense is the
//! filename itself, not its content).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::path::{Path, PathBuf};

use regex::Regex;

#[path = "walker_helpers.rs"]
mod traversal;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Filenames that legitimately match the editor-duplicate regex.
/// Each entry is a glob-free workspace-relative path suffix; the
/// match is `ends_with(suffix)` on a forward-slash-normalized
/// rendering of the absolute path. Initially empty. Add only with
/// written justification beside the const.
const RESERVED_DUPLICATE_PATHS: &[&str] = &[];

/// The pinned regex. Anchored at the basename: stem + ASCII space +
/// 1-2 digits + extension. Every recognized cert-relevant extension
/// gets the same enforcement; an editor that produces a different
/// suffix shape (`~`, `.bak`, `.orig`) is out of scope and would
/// land as a sibling regex rather than a relaxation of this one.
fn duplicate_regex() -> Regex {
    Regex::new(r"^.+ ([0-9]{1,2})\.(rs|toml|yml|yaml|md|json|lock)$").expect("valid regex")
}

/// Collect every editor-duplicate-shaped basename in the workspace.
/// Walks `workspace_root`, prunes the conventional skip directories,
/// applies the duplicate regex to each file's basename, drops
/// reserved entries, returns the remaining list as workspace-
/// relative paths (forward-slash separators).
fn scan_for_duplicates(root: &Path) -> Vec<String> {
    let re = duplicate_regex();
    traversal::walk(root)
        .filter_entry(|e| {
            // Skip the conventional build / VCS / vendor dirs plus
            // editor-and-tool metadata trees that aren't part of
            // the project's source. The `.claude/` directory in
            // particular collects scheduled-task lock files whose
            // names match the duplicate pattern by accident
            // (`scheduled_tasks 2.lock`); they are not source
            // artifacts and not under audit.
            !traversal::is_dir_named(
                e,
                &[
                    "target",
                    ".git",
                    "node_modules",
                    "fixtures",
                    ".direnv",
                    ".claude",
                    ".idea",
                    ".vscode",
                ],
            )
        })
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| re.is_match(name))
        })
        .map(|e| {
            e.path()
                .strip_prefix(root)
                .unwrap_or(e.path())
                .to_string_lossy()
                .replace('\\', "/")
        })
        .filter(|rel| {
            !RESERVED_DUPLICATE_PATHS
                .iter()
                .any(|exempt| rel.ends_with(exempt))
        })
        .collect()
}

/// Load-bearing regression: the current tree is clean.
#[test]
fn current_tree_is_clean() {
    let hits = scan_for_duplicates(&workspace_root());
    assert!(
        hits.is_empty(),
        "found {} editor-duplicate filename(s) in the workspace. \
         Each one is a `cp old new` / save-as artifact that compiles \
         silently, inflates floor counts, and pollutes audit \
         hashes. Delete with `\\rm` (the leading backslash bypasses \
         shell aliases) or rename to a real filename.\n\n{}",
        hits.len(),
        hits.iter()
            .map(|p| format!("  {}", p))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

/// Positive dogfood: a fixture with one duplicate-shaped filename
/// fires the gate.
#[test]
fn fires_on_synthetic_duplicate() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("crates").join("fake").join("src");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::write(src.join("helpers 2.rs"), "// editor duplicate\n").expect("write fixture");
    let hits = scan_for_duplicates(tmp.path());
    assert!(
        !hits.is_empty(),
        "expected gate to fire on `helpers 2.rs`; hits were empty"
    );
    assert!(
        hits.iter().any(|p| p.ends_with("helpers 2.rs")),
        "expected `helpers 2.rs` in the hit list; got {:?}",
        hits
    );
}

/// Negative dogfood: a filename whose digits do NOT follow a leading
/// space is legitimate and must not fire the gate.
#[test]
fn passes_on_legitimate_digits_filename() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("crates").join("fake").join("src");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::write(src.join("mcdc_2024.rs"), "// legitimate filename\n").expect("write fixture");
    std::fs::write(src.join("v1_data.toml"), "[meta]\n").expect("write fixture");
    let hits = scan_for_duplicates(tmp.path());
    assert!(
        hits.is_empty(),
        "expected legitimate digit-bearing filenames to pass; got hits {:?}",
        hits
    );
}
