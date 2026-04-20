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

use walkdir::{DirEntry, WalkDir};

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
    let rs_files = collect_rs_files(workspace_root);

    let mut unresolved = Vec::new();
    for t in tests {
        let selector = match t.test_selector.as_deref() {
            Some(s) if !s.trim().is_empty() => s,
            _ => continue, // empty / None selectors are not in scope
        };
        let fn_name = selector.rsplit("::").next().unwrap_or(selector);
        // Unqualified selectors (no `::`) don't constrain the search
        // scope — we resolve against any file. Qualified selectors
        // pin the search to files whose path reflects the leading
        // segment (crate / binary / module name).
        let prefix = if selector.contains("::") {
            Some(selector.split("::").next().unwrap_or(selector))
        } else {
            None
        };
        if !any_file_matches(&rs_files, fn_name, prefix) {
            unresolved.push(UnresolvedSelector {
                id: t.id.clone(),
                selector: selector.to_string(),
            });
        }
    }
    unresolved
}

fn collect_rs_files(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_noise_dir(e))
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("rs"))
        .map(|e| e.into_path())
        .collect()
}

fn is_noise_dir(e: &DirEntry) -> bool {
    e.file_type().is_dir()
        && matches!(
            e.file_name().to_str(),
            Some("target") | Some(".git") | Some("node_modules")
        )
}

/// `true` iff any `.rs` file in `files` whose path contains the
/// selector's leading segment contains a line matching `fn
/// <fn_name>\s*\(` with a `#[test]` attribute on one of the
/// preceding ≤5 non-blank lines.
///
/// The prefix anchor prevents `crate_a::...::fn_name` resolving via
/// a homonymous test function in `crate_b`. The anchor is
/// substring-style — matches `crate_a` as a directory name OR as a
/// filename stem (integration-test binaries) OR as a module path
/// segment. That's the widest rule that still eliminates cross-
/// crate false positives; tighter parsing would require mapping
/// selector prefixes to cargo target conventions, which is not
/// worth the complexity here.
fn any_file_matches(files: &[PathBuf], fn_name: &str, prefix: Option<&str>) -> bool {
    for file in files {
        if let Some(p) = prefix
            && !path_matches_prefix(file, p)
        {
            continue;
        }
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

/// Path-based prefix match: accepts `<prefix>.rs` as a filename,
/// `<prefix>/` as a directory segment, or matches against the
/// integration-test convention `tests/<prefix>.rs`. Windows-safe —
/// normalizes separators first.
fn path_matches_prefix(path: &Path, prefix: &str) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    // Filename stem equal to prefix (integration test binaries).
    if path.file_stem().and_then(|s| s.to_str()) == Some(prefix) {
        return true;
    }
    // Directory name equal to prefix (unit tests under `crates/<name>/`
    // or a module path component).
    let marker = format!("/{}/", prefix);
    normalized.contains(&marker)
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

    /// Regression for the crate-prefix bug: a selector
    /// `crate_a::...::fn_name` must not resolve via a homonymous test
    /// function in `crate_b`. The `path_matches_prefix` anchor is the
    /// guard.
    #[test]
    fn prefix_anchors_the_search_to_the_right_crate() {
        use std::fs;
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let crate_a = tmp.path().join("crates").join("crate_a").join("src");
        let crate_b = tmp.path().join("crates").join("crate_b").join("src");
        fs::create_dir_all(&crate_a).expect("mkdir a");
        fs::create_dir_all(&crate_b).expect("mkdir b");
        // Only crate_b has the test — a homonym in the wrong crate.
        fs::write(crate_a.join("lib.rs"), "pub fn unrelated() {}\n").expect("write a/lib");
        fs::write(
            crate_b.join("lib.rs"),
            "#[test]\nfn my_fn() { assert!(true); }\n",
        )
        .expect("write b/lib");

        let files = collect_rs_files(tmp.path());

        // Unqualified: resolves (legacy bare-name behavior preserved).
        assert!(any_file_matches(&files, "my_fn", None));
        // Qualified against crate_b: resolves.
        assert!(any_file_matches(&files, "my_fn", Some("crate_b")));
        // Qualified against crate_a: must NOT resolve via crate_b.
        assert!(!any_file_matches(&files, "my_fn", Some("crate_a")));
    }
}
