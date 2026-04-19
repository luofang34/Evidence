//! Resolve `TestEntry.test_selector` strings against actual
//! `#[test] fn` definitions in the workspace source.
//!
//! Without resolution, a refactor that renames a `#[test] fn` leaves
//! the selector dangling — the UUID-based `traces_to` link stays
//! valid, so nothing fires. This module is opt-in because walking
//! the source of a large workspace costs I/O; `cargo evidence trace
//! --validate --check-test-selectors` is the entry point.
//!
//! # Algorithm
//!
//! For each `TestEntry` with a non-empty `test_selector`:
//!
//! 1. Take the last `::`-separated segment as the function name.
//!    Rust's `cargo test <selector>` convention accepts a suffix
//!    match; we anchor on the function name since that's what
//!    matters for resolution.
//! 2. Walk every `.rs` file under the workspace root (skipping
//!    `target/`).
//! 3. For each file, search for `fn <name>\s*\(` such that one of
//!    the preceding five non-blank lines carries a `#[test]`
//!    attribute.
//! 4. Return `Ok(())` on first hit; `Err` with the full list of
//!    unresolvable selectors if any remain.
//!
//! # Why grep, not syn
//!
//! `syn` would handle macro-generated tests (e.g. `#[tokio::test]`,
//! `rstest`, quickcheck-style). Grep doesn't. The trade-off is
//! acceptable at this scope — the tool's self-trace uses plain
//! `#[test]` throughout. If a downstream project hits a macro
//! collision, a future PR can swap the resolver for a `syn`-based
//! one behind a `--strict` flag; the journal entry in
//! `tool/trace/README.md` documents that escape hatch.

use std::fs;
use std::path::{Path, PathBuf};

use super::entries::TestEntry;

/// One unresolvable selector surfaced by [`resolve_test_selectors`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedSelector {
    /// Test entry's human-readable ID (e.g. `"TEST-021"`).
    pub id: String,
    /// The `test_selector` string that couldn't be resolved.
    pub selector: String,
}

/// Walk `workspace_root` looking for a `#[test] fn` matching each
/// selector in `tests`. Returns the list of selectors that couldn't
/// be resolved; empty list means every selector points at a real
/// `#[test]` function.
pub fn resolve_test_selectors(
    tests: &[TestEntry],
    workspace_root: &Path,
) -> Vec<UnresolvedSelector> {
    let mut rs_files: Vec<PathBuf> = Vec::new();
    collect_rs_files(workspace_root, &mut rs_files);

    let mut unresolved = Vec::new();
    for t in tests {
        let selector = match t.test_selector.as_deref() {
            Some(s) if !s.trim().is_empty() => s,
            _ => continue, // empty / None selectors are not in scope
        };
        let fn_name = selector.rsplit("::").next().unwrap_or(selector);
        if !any_file_matches(&rs_files, fn_name) {
            unresolved.push(UnresolvedSelector {
                id: t.id.clone(),
                selector: selector.to_string(),
            });
        }
    }
    unresolved
}

fn collect_rs_files(root: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip target/ and .git/ to keep the walk fast.
            if matches!(
                path.file_name().and_then(|n| n.to_str()),
                Some("target") | Some(".git") | Some("node_modules")
            ) {
                continue;
            }
            collect_rs_files(&path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// `true` iff any `.rs` file in `files` contains a line matching
/// `fn <fn_name>\s*\(` with a `#[test]` attribute on one of the
/// preceding ≤5 non-blank lines. Grep-level parse, no syn dep.
fn any_file_matches(files: &[PathBuf], fn_name: &str) -> bool {
    for file in files {
        let text = match fs::read_to_string(file) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if file_has_test_fn(&text, fn_name) {
            return true;
        }
    }
    false
}

fn file_has_test_fn(text: &str, fn_name: &str) -> bool {
    // Build the two needle patterns: the `fn <name>(` prefix and any
    // `#[test]` attribute recognizer. Accept `#[test]` optionally
    // inside `#[cfg_attr(...)]` — not strict, best-effort.
    let fn_lines: Vec<(usize, &str)> = text
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let t = line.trim_start();
            // Match `fn <name>(` or `pub fn <name>(` or `async fn <name>(`.
            // Keep simple; the full signature can vary (`pub(crate)`,
            // `unsafe`, etc.) but `fn <name>(` always appears.
            let needle = format!("fn {}(", fn_name);
            let needle_space = format!("fn {} (", fn_name);
            t.contains(&needle) || t.contains(&needle_space)
        })
        .collect();

    if fn_lines.is_empty() {
        return false;
    }

    // For each candidate fn line, scan up to 5 preceding non-blank
    // lines looking for a #[test] attribute. Non-blank because
    // rustfmt often separates attribute clusters with blank lines.
    let all_lines: Vec<&str> = text.lines().collect();
    for (fn_idx, _) in fn_lines {
        let mut scanned = 0;
        let mut i = fn_idx;
        while i > 0 && scanned < 5 {
            i -= 1;
            let line = all_lines[i].trim();
            if line.is_empty() {
                continue;
            }
            scanned += 1;
            if line.starts_with("#[test]")
                || line.starts_with("#[cfg_attr") && line.contains("test") && line.contains(',')
            {
                return true;
            }
            // If we hit another `fn` or `}`, the attribute cluster
            // ended without a #[test].
            if line.starts_with("fn ") || line.starts_with("pub fn ") {
                break;
            }
        }
    }
    false
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;

    #[test]
    fn matches_basic_test_fn() {
        let source = r#"
            #[test]
            fn my_cool_test() {
                assert_eq!(1, 1);
            }
        "#;
        assert!(file_has_test_fn(source, "my_cool_test"));
    }

    #[test]
    fn rejects_non_test_fn() {
        let source = r#"
            fn helper_not_a_test() {}
        "#;
        assert!(!file_has_test_fn(source, "helper_not_a_test"));
    }

    #[test]
    fn rejects_unrelated_name() {
        let source = r#"
            #[test]
            fn other_test() {}
        "#;
        assert!(!file_has_test_fn(source, "my_cool_test"));
    }

    #[test]
    fn accepts_attr_cluster_separated_by_blank_line() {
        let source = r#"
            #[test]

            fn spaced_out_test() {}
        "#;
        assert!(file_has_test_fn(source, "spaced_out_test"));
    }

    #[test]
    fn accepts_pub_fn() {
        let source = r#"
            #[test]
            pub fn pub_test() {}
        "#;
        assert!(file_has_test_fn(source, "pub_test"));
    }
}
