//! Gate against rot-prone context markers in source comments +
//! policy prose (LLR-044).
//!
//! Walks:
//!
//! - `crates/**/{src,tests}/**/*.rs` — production + test source,
//!   scanned COMMENT-SCOPED: the `rust_comments` sibling module
//!   lexes each file and yields line (`//`), doc (`///`, `//!`),
//!   and block (`/* */` — Rust block comments nest) comment lines;
//!   the banned set applies to those lines only. Runtime string
//!   data (string literals, raw strings, byte strings, char
//!   literals) carrying the same words is data, not narration, and
//!   never fires the gate.
//! - `**/*.md` except `cert/trace/README.md` — top-level docs
//!   (README, per-crate docs), scanned WHOLE-FILE: Markdown is
//!   policy prose with no string-literal ambiguity, so every line
//!   is fair game. The trace journal is audit provenance and stays
//!   excluded.
//! - `cert/**/*.toml` — our own cert state, scanned WHOLE-FILE on
//!   the same policy-prose rationale. `cert/trace/**` stays
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
//! - PR-number breadcrumbs — references of the form `PR <number>`.
//! - Bare issue/PR breadcrumbs — a hash plus two-or-more digits at
//!   a token boundary. The number rots when history is rewritten;
//!   the surviving description or a stable anchor (LLR ID /
//!   function name) does not.
//! - Narrative pinned to before/after a specific PR landed, or to
//!   a review round.
//! - Absolute line counts — drift by the next edit.
//! - Recency adjectives — decay to meaningless.
//! - Forward-looking proximity hints about a future split.
//! - Temporal phrasing — the pinned patterns below carry the exact
//!   words. The comment describes a past tree, not the code it
//!   sits beside; rewrite as the present contract.
//!
//! ## Out of scope
//!
//! - `cert/trace/**` — PR refs in trace TOML are audit provenance,
//!   legitimate.
//! - `CHANGELOG.md` — the release audit journal; temporal
//!   narration is its purpose.
//! - Commit messages — immutable history.
//! - Stable identifiers (`LLR-NNN`, `TEST-NNN`, function names).
//! - Rust runtime string data — see the comment-scoped walk above.
//!
//! ## Escape hatch
//!
//! `RESERVED_TEXT_REFS` names files where a banned pattern is
//! load-bearing despite looking rot-prone. The list is empty: no
//! current exemptions. Additions require written justification in
//! a comment beside the const and are filename-scoped only — a
//! `path:line` pin rots the first time an unrelated edit moves the
//! pinned text, and `reserved_text_refs_carry_no_line_pins`
//! rejects such entries.

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

#[path = "rot_prone_markers_locked/rust_comments.rs"]
mod rust_comments;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Files where a banned pattern is load-bearing and the gate must
/// tolerate it. Each entry is a glob-free suffix match against the
/// file's workspace-relative path, filename-scoped only: a
/// `path:line` pin rots the first time an unrelated edit moves the
/// pinned text, and `reserved_text_refs_carry_no_line_pins` rejects
/// such entries outright.
///
/// Current exemptions: none — the list is empty and the scanner
/// needs none. (The audit-journal exclusions — `cert/trace/**` and
/// `CHANGELOG.md` — are walk-scope decisions documented in the
/// module doc, not entries here.) Add new entries with written
/// justification in a comment beside the const.
const RESERVED_TEXT_REFS: &[&str] = &[];

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
        // Bare issue/PR breadcrumbs — a hash plus two-or-more
        // digits at a token boundary. Requires the hash to sit at a
        // token boundary (line start, whitespace, or open-paren) so
        // upstream refs written `rust-lang/rust` + hash + number
        // (the hash follows a letter) do not match, and `\d{2,}`
        // skips ordinals like `rotation #1` / `key #2`.
        (
            "bare issue-number breadcrumb",
            Regex::new(r"(?:^|[\s(])#\d{2,}\b").expect("valid regex"),
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
        // Temporal phrasing — the CONTRIBUTING "WHY-only comments"
        // rule. Case-insensitive: the words read the same at sentence
        // start. `used to` is deliberately NOT banned as a pattern:
        // it has a legitimate instrumental reading ("used to
        // normalize"); the temporal reading is swept socially.
        (
            "temporal migration marker",
            Regex::new(r"(?i)\bmigrated from\b").expect("valid regex"),
        ),
        (
            "temporal 'previously' marker",
            Regex::new(r"(?i)\bpreviously\b").expect("valid regex"),
        ),
        (
            "temporal 'historically' marker",
            Regex::new(r"(?i)\bhistorically\b").expect("valid regex"),
        ),
        (
            "temporal 'formerly' marker",
            Regex::new(r"(?i)\bformerly\b").expect("valid regex"),
        ),
        (
            "temporal 'before this' marker",
            Regex::new(r"(?i)\bbefore this (?:module|crate|feature|change|fix)\b")
                .expect("valid regex"),
        ),
    ]
}

/// Collect all in-scope files for the gate.
///
/// Scope:
/// - `crates/**/*.rs` (excluding `target/`, `fixtures/`) — scanned
///   comment-scoped via `rust_comments::extract`.
/// - `**/*.md` at the workspace root and under `crates/`, but NOT
///   `cert/trace/README.md` (audit journal; legitimate PR refs) —
///   scanned whole-file.
/// - `cert/**/*.toml` (our own cert state), scanned whole-file;
///   `cert/trace/**/*.toml` stays excluded — entries legitimately
///   cite the implementing PR.
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
/// `cert/trace/` (journal = audit provenance). When invoked from
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
            // Skip cert/trace at any depth: the journal there is
            // audit trail, not rot.
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
        // CHANGELOG.md is the release audit journal: temporal
        // narration is its purpose, like the cert/trace journal.
        .filter(|e| e.file_name().to_str() != Some("CHANGELOG.md"))
        .map(|e| e.into_path());
    out.extend(files);
}

fn collect_toml_under(root: &Path, out: &mut Vec<PathBuf>) {
    let files = traversal::walk(root)
        .filter_entry(|e| {
            // Skip cert/trace/ — the trace TOMLs are an audit
            // journal whose PR refs are legitimate provenance, not
            // rot.
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

fn is_reserved(rel: &str) -> bool {
    RESERVED_TEXT_REFS.iter().any(|pat| rel.ends_with(pat))
}

/// Scan the tree for banned patterns. Returns a list of
/// `(relative_path, line_number, label, matched_text)` tuples.
///
/// Per-file-kind scope (kept in sync with the module doc — the
/// documented contract IS this behavior):
///
/// - `.rs` — extracted comment lines only (line, doc, and block
///   comments; see the `rust_comments` sibling module). String
///   literals and other runtime data are not scanned: the same
///   words are legitimate there.
/// - `.md` / `.toml` — every line. Policy and prose files carry no
///   string-literal ambiguity, so the whole file is fair game.
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
        if is_reserved(&rel) {
            continue;
        }
        if file.extension().and_then(|e| e.to_str()) == Some("rs") {
            for (lineno, comment) in rust_comments::extract(&content) {
                for (label, re) in &patterns {
                    if let Some(m) = re.find(comment) {
                        hits.push((rel.clone(), lineno, *label, m.as_str().to_string()));
                    }
                }
            }
        } else {
            for (lineno, line) in content.lines().enumerate() {
                for (label, re) in &patterns {
                    if let Some(m) = re.find(line) {
                        hits.push((rel.clone(), lineno + 1, *label, m.as_str().to_string()));
                    }
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

/// Positive dogfood: a bare `#NN` issue breadcrumb fires the gate.
#[test]
fn fires_on_bare_issue_number() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("crates").join("fake").join("src");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::write(
        src.join("lib.rs"),
        "//! The bug #82 fixed. See (#73) for context.\npub fn f() {}\n",
    )
    .expect("write fixture");
    let hits = scan_tree(tmp.path());
    assert!(
        hits.iter()
            .any(|(_, _, label, _)| *label == "bare issue-number breadcrumb"),
        "expected bare issue-number breadcrumb hit; got {:?}",
        hits
    );
}

/// Positive dogfood: temporal phrasing in comments fires the gate
/// (the CONTRIBUTING WHY-only rule's pinned word set).
#[test]
fn fires_on_temporal_phrasing() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("crates").join("fake").join("src");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::write(
        src.join("lib.rs"),
        "//! Previously this slid through. Migrated from the old loader.\n\
         // Before this module existed, X happened.\npub fn f() {}\n",
    )
    .expect("write fixture");
    let hits = scan_tree(tmp.path());
    for label in [
        "temporal 'previously' marker",
        "temporal migration marker",
        "temporal 'before this' marker",
    ] {
        assert!(
            hits.iter().any(|(_, _, l, _)| *l == label),
            "expected {label} hit; got {hits:?}"
        );
    }
}

/// Negative dogfood: upstream `rust-lang/rust#NNN` refs and small
/// ordinals (`rotation #1`) are legitimate and must not fire.
#[test]
fn does_not_fire_on_upstream_refs_or_ordinals() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("crates").join("clean").join("src");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::write(
        src.join("lib.rs"),
        "//! Tracks rust-lang/rust#144999 (merged upstream).\n\
         //! Labels rotations `rotation #1` and `key #2`.\n\
         pub fn stable() {}\n",
    )
    .expect("write fixture");
    let hits = scan_tree(tmp.path());
    assert!(
        hits.is_empty(),
        "expected upstream refs + ordinals to pass; got hits {:?}",
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

/// Defense against the line-pin brittleness: exemption entries are
/// filename-scoped, so an unrelated edit that moves the exempted
/// text can never silently break the gate. An entry carrying a `:`
/// line pin fails here — reword the text instead of pinning it.
#[test]
fn reserved_text_refs_carry_no_line_pins() {
    let pinned: Vec<&&str> = RESERVED_TEXT_REFS
        .iter()
        .filter(|entry| entry.contains(':'))
        .collect();
    assert!(
        pinned.is_empty(),
        "RESERVED_TEXT_REFS entries must be filename-scoped (no `:line` pins); \
         reword the exempted text to drop the banned literal instead: {pinned:?}"
    );
}
