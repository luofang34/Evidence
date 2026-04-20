//! Gate against rot-prone context markers in source + docs
//! (LLR-044).
//!
//! Walks:
//!
//! - `crates/**/{src,tests}/**/*.rs` — production + test source.
//! - `**/*.md` except `tool/trace/README.md` — top-level docs (README,
//!   per-crate docs). The trace journal is audit provenance and stays
//!   excluded.
//! - `cert/**/*.toml` — our own cert state. `tool/trace/**` stays
//!   excluded (legitimate audit trail).
//!
//! Applies a pinned regex pattern set and fails via `assert!` with
//! `file:line` listing for any offending match. No `Diagnostic`
//! wire shape; no `RULES` entry — the test's failure message is
//! the diagnostic. Mirrors `schema_versions_locked`,
//! `diagnostic_codes_locked`, `floors_equal_current_no_slack`.
//!
//! ## What counts as "rot-prone"
//!
//! Markers whose truth depends on transient state outside the file:
//!
//! - PR-number breadcrumbs — `PR #49 removed ...` and relatives.
//! - "post-PR" / "pre-PR" / review-round narrative.
//! - Absolute line counts — drift by the next edit.
//! - "Newly-added" / "just-added" adjectives — decay to meaningless.
//! - Forward-looking proximity hints — `next natural split`.
//!
//! ## Out of scope
//!
//! - `tool/trace/**` — PR refs in trace TOML are audit provenance,
//!   legitimate.
//! - Commit messages — immutable history.
//! - Stable identifiers (`LLR-NNN`, `TEST-NNN`, function names).
//!
//! ## Escape hatch
//!
//! `RESERVED_TEXT_REFS` names files + lines where a banned pattern
//! is load-bearing despite looking rot-prone. Initially empty;
//! additions require written justification in a comment beside the
//! const.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
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

/// Files or file+line pairs where a banned pattern is load-bearing
/// and the gate must tolerate it. Each entry is a glob-free suffix
/// match against the file's workspace-relative path; if a match
/// warrants exemption only at a specific line, use
/// `<path>:<line_number>`.
///
/// Current exemptions: none. Add with justification.
const RESERVED_TEXT_REFS: &[&str] = &[
    // The rot-gate itself must pattern-match the banned text in its
    // source (the regex patterns appear verbatim inside string
    // literals) — otherwise it couldn't enforce the rule. Excluded
    // by filename so the patterns stay literal and auditable.
    "tests/rot_prone_markers_locked.rs",
];

/// Pinned banned-pattern set. Each entry is a label + regex. Labels
/// appear in failure output so a hit reads as "file:line matched
/// <label>", not as a raw regex.
///
/// Rules of thumb for adding a pattern:
///
/// - Must be narrow enough that a passing tree is achievable.
/// - Must have no legitimate use in `.rs` docstrings or comments.
/// - A new pattern lands with a sweep commit that cleans the tree
///   first; the gate test fires on its own tree otherwise.
fn banned_patterns() -> Vec<(&'static str, Regex)> {
    vec![
        (
            "PR-number breadcrumb",
            Regex::new(r"PR\s+#\d+").expect("valid regex"),
        ),
        (
            "pre-/post-PR narrative",
            Regex::new(r"\b(?:pre-PR|post-PR)\b").expect("valid regex"),
        ),
        (
            "review-round marker",
            Regex::new(r"\bround[\s-]?\d+\b").expect("valid regex"),
        ),
        (
            "absolute line-count narrative",
            Regex::new(r"sits at ~?\d+ lines|currently at \d+ lines").expect("valid regex"),
        ),
        (
            "newness decay marker",
            Regex::new(r"\b(?:newly-introduced|newly-added|just-added)\b").expect("valid regex"),
        ),
        (
            "forward split hint",
            Regex::new(r"\bnext natural split\b").expect("valid regex"),
        ),
    ]
}

/// Collect all in-scope files for the gate.
///
/// Scope:
/// - `crates/**/*.rs` (excluding `target/`, `fixtures/`).
/// - `**/*.md` at the workspace root and under `crates/`, but NOT
///   `tool/trace/README.md` (audit journal; legitimate PR refs).
/// - `cert/**/*.toml` (our own cert state); `tool/trace/**/*.toml`
///   stays excluded — entries legitimately cite the implementing PR.
fn collect_scan_targets(workspace_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_rs(&workspace_root.join("crates"), &mut out);
    collect_md(workspace_root, &mut out, true);
    collect_toml_under(&workspace_root.join("cert"), &mut out);
    out
}

fn collect_rs(root: &Path, out: &mut Vec<PathBuf>) {
    let files = traversal::walk(root)
        .filter_entry(|e| {
            !traversal::is_dir_named(e, &["target", ".git", "node_modules", "fixtures"])
        })
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && traversal::has_ext(e.path(), "rs"))
        .map(|e| e.into_path());
    out.extend(files);
}

/// Walk `.md` files. Skips `target/`, `.git/`, `node_modules/`,
/// `tool/trace/` (journal = audit provenance). When invoked from
/// the workspace root, also skips `cert/` (the toml walker handles
/// it).
fn collect_md(root: &Path, out: &mut Vec<PathBuf>, is_workspace_root: bool) {
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
            // Skip tool/trace at any depth: the journal there is
            // audit trail, not rot.
            if e.file_type().is_dir()
                && e.file_name().to_str() == Some("trace")
                && e.path()
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    == Some("tool")
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

fn collect_toml_under(root: &Path, out: &mut Vec<PathBuf>) {
    let files = traversal::walk(root)
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && traversal::has_ext(e.path(), "toml"))
        .map(|e| e.into_path());
    out.extend(files);
}

fn is_reserved(rel: &str, line_num: usize) -> bool {
    let normalized = rel.replace('\\', "/");
    RESERVED_TEXT_REFS.iter().any(|pat| {
        if let Some((path_part, line_part)) = pat.split_once(':') {
            normalized.ends_with(path_part)
                && line_part.parse::<usize>().is_ok_and(|n| n == line_num)
        } else {
            normalized.ends_with(pat)
        }
    })
}

/// Scan the tree for banned patterns. Returns a list of
/// `(relative_path, line_number, label, matched_text)` tuples.
fn scan_tree(root: &Path) -> Vec<(String, usize, &'static str, String)> {
    let files = collect_scan_targets(root);
    let patterns = banned_patterns();
    let mut hits = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(content) = fs::read_to_string(file) else {
            continue;
        };
        for (lineno, line) in content.lines().enumerate() {
            let lineno = lineno + 1;
            if is_reserved(&rel, lineno) {
                continue;
            }
            for (label, re) in &patterns {
                if let Some(m) = re.find(line) {
                    hits.push((rel.clone(), lineno, *label, m.as_str().to_string()));
                }
            }
        }
    }
    hits
}

/// Load-bearing regression: the current tree is clean of rot-prone
/// markers.
#[test]
fn current_tree_is_clean() {
    let hits = scan_tree(&workspace_root());
    assert!(
        hits.is_empty(),
        "found {} rot-prone marker(s) in `.rs` sources. Each one decays \
         faster than the code around it; strip or rewrite with a stable \
         anchor (function name / LLR ID / module path).\n\n{}\n\n\
         If a specific occurrence is genuinely load-bearing, add it to \
         `RESERVED_TEXT_REFS` with written justification.",
        hits.len(),
        hits.iter()
            .map(|(f, l, label, text)| format!("  {}:{} [{}] `{}`", f, l, label, text))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

/// Positive dogfood: a fixture with one banned pattern fires the
/// gate.
#[test]
fn fires_on_banned_pattern() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("crates").join("fake").join("src");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::write(
        src.join("lib.rs"),
        "//! Module docstring.\n// PR #42 added this behavior.\npub fn f() {}\n",
    )
    .expect("write fixture");
    let hits = scan_tree(tmp.path());
    assert!(
        !hits.is_empty(),
        "expected gate to fire on `// PR #42 added this behavior.`; hits were empty"
    );
    assert!(
        hits.iter()
            .any(|(_, _, label, _)| *label == "PR-number breadcrumb"),
        "expected PR-number breadcrumb hit; got {:?}",
        hits
    );
}

/// Negative dogfood: a fixture with no banned patterns passes.
#[test]
fn passes_on_clean_fixture() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("crates").join("clean").join("src");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::write(
        src.join("lib.rs"),
        "//! A module that describes its purpose without time pins.\n\
         pub fn stable() {}\n",
    )
    .expect("write fixture");
    let hits = scan_tree(tmp.path());
    assert!(
        hits.is_empty(),
        "expected clean fixture to pass; got hits {:?}",
        hits
    );
}
