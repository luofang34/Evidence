//! Filesystem walkers for `trace_id_refs_locked`.
//!
//! Split out of the main file to stay under the 500-line workspace
//! file-size limit (see `crates/evidence/tests/file_size_limit.rs`).
//! Each walker carries its own skip-list so callsites don't share a
//! `config` struct — cheaper than the abstraction. Once the
//! workspace-wide `walkdir`-based consolidation lands, all three
//! collapse into a single callsite here.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::{Path, PathBuf};

/// Collect all in-scope files for the trace-ID ref scan.
///
/// Scope:
/// - `crates/**/*.rs` (excluding `target/`, `fixtures/`).
/// - `**/*.md` at workspace root and under `crates/`, but NOT
///   `tool/trace/README.md` (journal = audit provenance; stale
///   refs there would be historical artifacts, not drift).
/// - `**/*.toml` outside `tool/trace/` — Cargo manifests, cert
///   baselines, floors.toml. `tool/trace/**/*.toml` is the
///   ground truth we validate against and is explicitly excluded.
pub fn collect_scan_targets(workspace: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_by_ext(&workspace.join("crates"), "rs", &mut out, &[]);
    collect_md_non_trace(workspace, &mut out, true);
    collect_toml_non_trace(workspace, &mut out, true);
    out
}

fn collect_by_ext(root: &Path, ext: &str, out: &mut Vec<PathBuf>, skip_dirs: &[&str]) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(
                name,
                "target" | ".git" | "node_modules" | "fixtures" | ".claude" | ".githooks"
            ) || skip_dirs.contains(&name)
            {
                continue;
            }
            collect_by_ext(&path, ext, out, skip_dirs);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(path);
        }
    }
}

/// Walk `.md` files, skipping `tool/trace/` (journal is audit
/// provenance).
fn collect_md_non_trace(root: &Path, out: &mut Vec<PathBuf>, is_workspace_root: bool) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(
                name,
                "target" | ".git" | "node_modules" | "fixtures" | ".claude" | ".githooks"
            ) {
                continue;
            }
            if name == "trace" && path.parent().and_then(|p| p.file_name()) == Some("tool".as_ref())
            {
                continue;
            }
            // Don't descend into cert under the workspace root — the
            // toml walker handles it.
            if is_workspace_root && name == "cert" {
                continue;
            }
            collect_md_non_trace(&path, out, false);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

/// Walk `.toml` files, skipping `tool/trace/` (the source of
/// truth) and `target/`.
fn collect_toml_non_trace(root: &Path, out: &mut Vec<PathBuf>, is_workspace_root: bool) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(
                name,
                "target" | ".git" | "node_modules" | "fixtures" | ".claude" | ".githooks"
            ) {
                continue;
            }
            if name == "trace" && path.parent().and_then(|p| p.file_name()) == Some("tool".as_ref())
            {
                continue;
            }
            let _ = is_workspace_root;
            collect_toml_non_trace(&path, out, false);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            out.push(path);
        }
    }
}
