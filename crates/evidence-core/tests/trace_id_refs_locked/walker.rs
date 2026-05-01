//! Filesystem walkers for `trace_id_refs_locked`.
//!
//! Split out of the main file to stay under the 500-line workspace
//! file-size limit (see `crates/evidence-core/tests/file_size_limit.rs`).
//! Filter logic stays here (per-callsite tree-skip rules); the shared
//! `follow_links(false)` primitive lives in `tests/walker_helpers.rs`
//! and is loaded by the parent binary as `mod traversal;`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::path::{Path, PathBuf};

use super::traversal;

/// Collect all in-scope files for the trace-ID ref scan.
///
/// Scope:
/// - `crates/**/*.rs` (excluding `target/`, `fixtures/`).
/// - `**/*.md` at workspace root and under `crates/`, but NOT
///   `cert/trace/README.md` (journal = audit provenance; stale
///   refs there would be historical artifacts, not drift).
/// - `**/*.toml` outside `cert/trace/` — Cargo manifests, cert
///   baselines, floors.toml. `cert/trace/**/*.toml` is the
///   ground truth we validate against and is explicitly excluded.
pub fn collect_scan_targets(workspace: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_by_ext(&workspace.join("crates"), "rs", &mut out);
    collect_md_non_trace(workspace, &mut out, true);
    collect_toml_non_trace(workspace, &mut out);
    out
}

fn collect_by_ext(root: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let files = traversal::walk(root)
        .filter_entry(|e| {
            !traversal::is_dir_named(
                e,
                &[
                    "target",
                    ".git",
                    "node_modules",
                    "fixtures",
                    ".claude",
                    ".githooks",
                ],
            )
        })
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && traversal::has_ext(e.path(), ext))
        .map(|e| e.into_path());
    out.extend(files);
}

/// Walk `.md` files, skipping `cert/trace/` (journal = audit
/// provenance) at any depth, and — when invoked from the workspace
/// root — top-level `cert/` so the toml walker owns that subtree.
fn collect_md_non_trace(root: &Path, out: &mut Vec<PathBuf>, is_workspace_root: bool) {
    let top_skip: &[&str] = if is_workspace_root { &["cert"] } else { &[] };
    let files = traversal::walk(root)
        .filter_entry(|e| {
            if traversal::is_dir_named(
                e,
                &[
                    "target",
                    ".git",
                    "node_modules",
                    "fixtures",
                    ".claude",
                    ".githooks",
                ],
            ) {
                return false;
            }
            if e.file_type().is_dir()
                && e.file_name().to_str() == Some("trace")
                && e.path()
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    == Some("cert")
            {
                return false;
            }
            if e.depth() == 1 && !top_skip.is_empty() && traversal::is_dir_named(e, top_skip) {
                return false;
            }
            true
        })
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && traversal::has_ext(e.path(), "md"))
        .map(|e| e.into_path());
    out.extend(files);
}

/// Walk `.toml` files, skipping `cert/trace/` (the source of
/// truth) and standard noise dirs.
fn collect_toml_non_trace(root: &Path, out: &mut Vec<PathBuf>) {
    let files = traversal::walk(root)
        .filter_entry(|e| {
            if traversal::is_dir_named(
                e,
                &[
                    "target",
                    ".git",
                    "node_modules",
                    "fixtures",
                    ".claude",
                    ".githooks",
                ],
            ) {
                return false;
            }
            if e.file_type().is_dir()
                && e.file_name().to_str() == Some("trace")
                && e.path()
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    == Some("cert")
            {
                return false;
            }
            true
        })
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && traversal::has_ext(e.path(), "toml"))
        .map(|e| e.into_path());
    out.extend(files);
}
