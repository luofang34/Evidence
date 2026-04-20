//! Shared traversal primitive for locked-test walkers.
//!
//! Introduces one source of truth for `follow_links(false)` so each
//! converted callsite inherits the same semantics as the pre-walkdir
//! `fs::read_dir` recursive walk. Filter logic (which dirs to skip,
//! which extensions to keep) stays at the callsite because every
//! locked test has slightly different rules — a config-knobbed
//! generic walker would cost more than the duplication it replaced.
//!
//! Usage:
//!
//! ```ignore
//! walk(root)
//!     .filter_entry(|e| !is_skipped_dir(e))
//!     .filter_map(Result::ok)
//!     .filter(|e| e.file_type().is_file())
//!     .filter(|e| has_ext(e.path(), "rs"))
//!     .map(|e| e.into_path())
//!     .collect::<Vec<_>>()
//! ```
//!
//! `filter_entry` runs before descent, so returning `false` for a
//! directory prunes its subtree — equivalent to the `continue` arm
//! each original walker had for `target/` / `.git/` / etc.
//!
//! Not a production-code concern: the `evidence` crate already ships
//! `walkdir` as a regular dependency (`verify::bundle`, `hash`), so
//! this helper adds no new transitive deps.

#![allow(
    dead_code,
    reason = "callsites consume some but not all of these helpers"
)]

use std::path::Path;

use walkdir::{DirEntry, IntoIter, WalkDir};

/// Recursive walk rooted at `root` without following symbolic links.
/// Return type is `walkdir::IntoIter` so callers can still apply
/// `.filter_entry(...)` for subtree pruning — that method isn't
/// available after the iterator is mapped or filtered.
pub fn walk(root: &Path) -> IntoIter {
    WalkDir::new(root).follow_links(false).into_iter()
}

/// Convenience: does `path`'s extension equal `want`?
pub fn has_ext(path: &Path, want: &str) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some(want)
}

/// Convenience: is this a directory whose file-name matches any of
/// the given names? Used inside `filter_entry` closures to prune
/// well-known directories (`target`, `.git`, `node_modules`, ...).
pub fn is_dir_named(e: &DirEntry, names: &[&str]) -> bool {
    e.file_type().is_dir() && e.file_name().to_str().is_some_and(|n| names.contains(&n))
}
