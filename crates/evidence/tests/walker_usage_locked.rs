//! Gate against hand-rolled recursive `fs::read_dir` walkers
//! (LLR-047).
//!
//! Walks `crates/**/*.rs` and fails via `assert!` if any file calls
//! `fs::read_dir` or `std::fs::read_dir` without being listed in
//! [`ALLOWED_READ_DIR_FILES`].
//!
//! The project convention is **walkdir-only** for recursive file-tree
//! traversal (see CLAUDE.md "File-tree traversal"). `walkdir` is
//! already a production dep of `evidence`, so there's no incremental
//! cost. Non-recursive single-directory listings are the sole
//! legitimate use for hand-rolled `fs::read_dir`; every such use must
//! be allowlisted in this file with written justification.
//!
//! This gate doesn't distinguish recursive from non-recursive at
//! scan time — the substring `fs::read_dir(` is a blunt match. The
//! allowlist is the escape hatch. Forcing a named exemption per
//! legitimate non-recursive use keeps the surface small and each
//! entry reviewer-visible.
//!
//! No `Diagnostic` wire shape; no `RULES` entry — the test's failure
//! message is the diagnostic. Same pattern as
//! `schema_versions_locked`, `rot_prone_markers_locked`,
//! `trace_id_refs_locked`.

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

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Files where `fs::read_dir` or `std::fs::read_dir` is allowed.
/// Every entry must be a non-recursive single-directory listing; if
/// recursion is needed, use `walkdir::WalkDir` instead.
///
/// Each entry is a workspace-relative suffix match.
const ALLOWED_READ_DIR_FILES: &[&str] = &[
    // This gate's own source mentions `fs::read_dir` in prose + the
    // string-literal needle used by `scan_for_hits`. Excluded by
    // filename so the needle stays literal and the failure message
    // stays readable.
    "tests/walker_usage_locked.rs",
    // Documentation-only reference in the shared helper's module
    // docstring ("replaces pre-walkdir `fs::read_dir` walk").
    "tests/walker_helpers.rs",
    // Single-dir `compliance/*.json` listing — index-vs-compliance
    // drift detection. Not a tree walk.
    "src/verify/consistency.rs",
    // Single-dir bundle-finder: `generate --out-dir` produces exactly
    // one bundle dir under tmp; the listing picks it up by name.
    // Separately, a single-dir `compliance/` listing collects the
    // per-crate JSON files.
    "tests/self_compliance_baseline.rs",
    // Single-dir bundle-finder — same pattern as the compliance
    // baseline test.
    "tests/verify_jsonl.rs",
    // Single-dir bundle-finder — same pattern.
    "tests/check_bundle_mode.rs",
];

/// Substring needles that flag a hand-rolled `fs::read_dir` call.
/// Both shapes (`fs::read_dir` and `std::fs::read_dir`) are scanned
/// so a fully-qualified call doesn't slip past.
const READ_DIR_NEEDLES: &[&str] = &["fs::read_dir(", "std::fs::read_dir("];

fn is_allowed(rel: &str) -> bool {
    let normalized = rel.replace('\\', "/");
    ALLOWED_READ_DIR_FILES
        .iter()
        .any(|suffix| normalized.ends_with(suffix))
}

/// Scan every `.rs` file under `crates/` for `fs::read_dir(`-shape
/// calls outside `ALLOWED_READ_DIR_FILES`. Returns sorted
/// `(file, line, needle)` hits.
fn scan_for_hits(workspace: &Path) -> Vec<(String, usize, &'static str)> {
    let files: Vec<PathBuf> = traversal::walk(&workspace.join("crates"))
        .filter_entry(|e| {
            !traversal::is_dir_named(e, &["target", ".git", "node_modules", "fixtures"])
        })
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && traversal::has_ext(e.path(), "rs"))
        .map(|e| e.into_path())
        .collect();

    let mut hits: Vec<(String, usize, &'static str)> = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(workspace)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        if is_allowed(&rel) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };
        for (lineno, line) in content.lines().enumerate() {
            for needle in READ_DIR_NEEDLES {
                if line.contains(needle) {
                    hits.push((rel.clone(), lineno + 1, *needle));
                }
            }
        }
    }
    hits.sort();
    hits
}

#[test]
fn no_unauthorized_read_dir() {
    let hits = scan_for_hits(&workspace_root());
    assert!(
        hits.is_empty(),
        "found {} unallowlisted `fs::read_dir` call site(s). Recursive walks \
         must use `walkdir::WalkDir` (already a workspace dep). If a call is \
         genuinely non-recursive (single-directory listing), add the file to \
         `ALLOWED_READ_DIR_FILES` in tests/walker_usage_locked.rs with \
         written justification.\n\n{}",
        hits.len(),
        hits.iter()
            .map(|(f, l, n)| format!("  {}:{}  {}", f, l, n))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

/// Positive dogfood: planting an `fs::read_dir(` call in a fixture
/// file fires the gate.
#[test]
fn fires_on_unallowlisted_call() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let crate_src = tmp.path().join("crates").join("fake").join("src");
    std::fs::create_dir_all(&crate_src).expect("mkdir -p");
    std::fs::write(
        crate_src.join("lib.rs"),
        "use std::fs;\npub fn f() { let _ = fs::read_dir(\"x\"); }\n",
    )
    .expect("write fixture");

    let hits = scan_for_hits(tmp.path());
    assert!(
        !hits.is_empty(),
        "expected fake fs::read_dir call to fire the gate"
    );
    assert!(
        hits.iter().any(|(f, _, _)| f.ends_with("fake/src/lib.rs")),
        "expected hit in fake/src/lib.rs; got {:?}",
        hits
    );
}

/// Negative dogfood: a file whose path matches
/// `ALLOWED_READ_DIR_FILES` passes even when it contains the
/// banned substring.
#[test]
fn passes_on_allowlisted_file() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    // Create the file path shape that the allowlist exempts.
    let test_dir = tmp
        .path()
        .join("crates")
        .join("cargo-evidence")
        .join("tests");
    std::fs::create_dir_all(&test_dir).expect("mkdir -p");
    std::fs::write(
        test_dir.join("verify_jsonl.rs"),
        "use std::fs;\npub fn f() { let _ = fs::read_dir(\"x\"); }\n",
    )
    .expect("write fixture");

    let hits = scan_for_hits(tmp.path());
    assert!(
        hits.iter().all(|(f, _, _)| !f.ends_with("verify_jsonl.rs")),
        "expected allowlisted file to NOT fire the gate; got {:?}",
        hits
    );
}
