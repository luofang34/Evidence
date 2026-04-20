//! Gate against narrative trace-ID references (SYS/HLR/LLR/TEST/
//! DERIVED-NNN) that don't resolve to a real entry in `tool/trace/`
//! (LLR-046).
//!
//! Walks:
//!
//! - `crates/**/{src,tests}/**/*.rs` — production + test source.
//! - `**/*.md` except `tool/trace/README.md` — top-level docs.
//! - `**/*.toml` outside `tool/trace/` — our own cert state, Cargo
//!   manifests, etc. `tool/trace/**/*.toml` is the ground-truth
//!   registry and is excluded from the walk (its own `id` fields
//!   are what we're validating against).
//!
//! For every match of `\b(SYS|HLR|LLR|TEST|DERIVED)-\d+\b`, the
//! gate looks up the ref against the `id` field set in the matching
//! kind's trace file and fails via `assert!` with a sorted
//! `file:line <kind>-<id>` listing on any unresolved ref. No
//! `Diagnostic` wire shape; no `RULES` entry — the test's failure
//! message is the diagnostic. Same pattern as
//! `schema_versions_locked`, `diagnostic_codes_locked`,
//! `rot_prone_markers_locked`.
//!
//! ## Escape hatch
//!
//! `RESERVED_TEXT_REFS` lists `(file_suffix, ref)` pairs where a
//! narrative mention is illustrative (e.g., `LLR-NNN` as a template
//! placeholder in a docstring explaining the ID format) rather than
//! a real cross-reference. Initially empty; additions require
//! written justification in a comment beside the const.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// `(file_suffix, ref)` pairs where a banned pattern is allowed
/// because it's illustrative prose, not a real ref. Each entry
/// takes a suffix match on the workspace-relative path + an exact
/// match on the reference text (e.g. `("some_file.rs",
/// "LLR-NNN")`).
///
/// Current exemptions — all illustrative / local-fixture refs in
/// test-file prose, not cross-references to `tool/trace/`:
const RESERVED_TEXT_REFS: &[(&str, &str)] = &[
    // Illustrative example of the single-digit fixture-ID shape in
    // `comment_window`'s docstring. Not a trace ref.
    ("trace_id_refs_locked.rs", "HLR-1"),
    // Intentional ghost ref used by the positive-fire test.
    // Resolving it would defeat the test.
    ("trace_id_refs_locked.rs", "LLR-999"),
    // Narrative reference to a locally-constructed synthetic
    // fixture in `check_source_tree.rs`. Not a cross-reference to
    // `tool/trace/`.
    ("check_source_tree.rs", "TEST-1"),
];

/// Collect all in-scope files.
///
/// Scope:
/// - `crates/**/*.rs` (excluding `target/`, `fixtures/`).
/// - `**/*.md` at workspace root and under `crates/`, but NOT
///   `tool/trace/README.md` (journal = audit provenance; stale
///   refs there would be historical artifacts, not drift).
/// - `**/*.toml` outside `tool/trace/` — Cargo manifests, cert
///   baselines, floors.toml. `tool/trace/**/*.toml` is the
///   ground truth we validate against and is explicitly excluded.
fn collect_scan_targets(workspace: &Path) -> Vec<PathBuf> {
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
            // Drop into cert/ too, but it's otherwise an ordinary
            // recurse.
            let _ = is_workspace_root;
            collect_toml_non_trace(&path, out, false);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            out.push(path);
        }
    }
}

/// Per-kind valid-ID set loaded from `tool/trace/`.
struct TraceIdSets {
    sys: BTreeSet<String>,
    hlr: BTreeSet<String>,
    llr: BTreeSet<String>,
    test: BTreeSet<String>,
    derived: BTreeSet<String>,
}

fn load_trace_id_sets(workspace: &Path) -> TraceIdSets {
    let trace_root = workspace
        .join("tool")
        .join("trace")
        .to_str()
        .expect("path is UTF-8")
        .to_string();
    let trace = evidence::read_all_trace_files(&trace_root).expect("tool/trace must load cleanly");
    let sys: BTreeSet<String> = trace
        .sys
        .requirements
        .iter()
        .map(|r| r.id.clone())
        .collect();
    let hlr: BTreeSet<String> = trace
        .hlr
        .requirements
        .iter()
        .map(|r| r.id.clone())
        .collect();
    let llr: BTreeSet<String> = trace
        .llr
        .requirements
        .iter()
        .map(|r| r.id.clone())
        .collect();
    let test: BTreeSet<String> = trace.tests.tests.iter().map(|t| t.id.clone()).collect();
    let derived: BTreeSet<String> = trace
        .derived
        .as_ref()
        .map(|d| d.requirements.iter().map(|e| e.id.clone()).collect())
        .unwrap_or_default();
    TraceIdSets {
        sys,
        hlr,
        llr,
        test,
        derived,
    }
}

/// Ghost refs: one row per unresolved `(file, line, kind, id)`.
type GhostRef = (String, usize, &'static str, String);

/// Scan targets for trace-ID refs; return list of unresolved
/// references sorted by file + line for deterministic output.
fn scan_tree(workspace: &Path) -> Vec<GhostRef> {
    let files = collect_scan_targets(workspace);
    let ids = load_trace_id_sets(workspace);
    let re = Regex::new(r"\b(SYS|HLR|LLR|TEST|DERIVED)-\d+\b").expect("valid regex");
    let mut hits: Vec<GhostRef> = Vec::new();

    // Per-kind-in-closure lookup; `ids` is moved into a map keyed
    // by the captured kind string.
    let mut sets: BTreeMap<&'static str, &BTreeSet<String>> = BTreeMap::new();
    sets.insert("SYS", &ids.sys);
    sets.insert("HLR", &ids.hlr);
    sets.insert("LLR", &ids.llr);
    sets.insert("TEST", &ids.test);
    sets.insert("DERIVED", &ids.derived);

    for file in &files {
        let rel = file
            .strip_prefix(workspace)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(content) = fs::read_to_string(file) else {
            continue;
        };
        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
        for (lineno, line) in content.lines().enumerate() {
            let lineno = lineno + 1;
            // Narrow the scan window to "comment-like prose" so
            // synthetic test-fixture IDs in string literals (e.g.
            // `let h = hlr("HLR-1", ...)` in integration tests)
            // don't fire the gate.
            let scan_text = comment_window(line, ext);
            if scan_text.is_empty() {
                continue;
            }
            for m in re.captures_iter(scan_text) {
                let full = m.get(0).expect("full match").as_str();
                let kind_raw = m.get(1).expect("kind group").as_str();
                let kind: &'static str = match kind_raw {
                    "SYS" => "SYS",
                    "HLR" => "HLR",
                    "LLR" => "LLR",
                    "TEST" => "TEST",
                    "DERIVED" => "DERIVED",
                    _ => continue,
                };
                if is_reserved(&rel, full) {
                    continue;
                }
                let set = sets.get(kind).expect("kind mapped");
                if !set.contains(full) {
                    hits.push((rel.clone(), lineno, kind, full.to_string()));
                }
            }
        }
    }

    hits.sort();
    hits
}

/// Return the comment-like portion of `line` — the slice the gate
/// should scan for narrative refs. Non-comment code (string
/// literals, identifiers, module paths) is excluded.
///
/// - `.rs`: everything from the first unquoted `//` onward. A
///   `//` inside a string literal isn't a comment start; this
///   helper does a quote-parity scan to find the real one.
///   Returns `""` when the line has no comment.
/// - `.md`: the whole line (Markdown is free prose).
/// - `.toml`: everything from the first unquoted `#` onward. TOML
///   strings can contain `#` so the quote-parity scan applies
///   there too. Returns `""` when the line has no comment.
/// - Other: empty (unsupported extension).
fn comment_window<'a>(line: &'a str, ext: &str) -> &'a str {
    match ext {
        "md" => line,
        "rs" => find_comment_start(line, "//").map_or("", |i| &line[i..]),
        "toml" => find_comment_start(line, "#").map_or("", |i| &line[i..]),
        _ => "",
    }
}

/// Quote-aware search for the first occurrence of `marker` outside
/// `"..."` / `'...'` string literals. Returns the byte offset of
/// the marker (guaranteed to be a char boundary), or `None` when
/// it doesn't appear in comment position. Iterates
/// `char_indices()` so multi-byte UTF-8 sequences (em-dashes,
/// non-ASCII prose) don't break the scan.
fn find_comment_start(line: &str, marker: &str) -> Option<usize> {
    let mut in_dq = false;
    let mut in_sq = false;
    let mut chars = line.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        // Backslash escapes the next char inside any string.
        if (in_dq || in_sq) && c == '\\' {
            // Skip the escaped char.
            chars.next();
            continue;
        }
        if !in_sq && c == '"' {
            in_dq = !in_dq;
            continue;
        }
        if !in_dq && c == '\'' {
            in_sq = !in_sq;
            continue;
        }
        if !in_dq && !in_sq && line[i..].starts_with(marker) {
            return Some(i);
        }
    }
    None
}

fn is_reserved(rel: &str, full_ref: &str) -> bool {
    let normalized = rel.replace('\\', "/");
    RESERVED_TEXT_REFS
        .iter()
        .any(|(path_suffix, r)| normalized.ends_with(path_suffix) && *r == full_ref)
}

/// Load-bearing regression: the workspace tree has no ghost refs.
#[test]
fn current_tree_is_clean() {
    let hits = scan_tree(&workspace_root());
    assert!(
        hits.is_empty(),
        "found {} trace-ID reference(s) in source/docs that don't resolve to any \
         entry in tool/trace/. A deleted or renumbered trace entry has left stale \
         narrative pointers behind. Either restore the referenced entry, update \
         the reference to a still-valid identifier, or add the (file, ref) pair \
         to RESERVED_TEXT_REFS with written justification.\n\n{}",
        hits.len(),
        hits.iter()
            .map(|(f, l, kind, id)| format!("  {}:{} {}", f, l, id_with_kind(kind, id)))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

fn id_with_kind(kind: &str, id: &str) -> String {
    // `id` already carries the full token (e.g. "LLR-999"); kind
    // is redundant in the output line but useful for grouping.
    let _ = kind;
    id.to_string()
}

/// Positive dogfood: a fixture with a ghost `LLR-999` fires the
/// gate, naming the offending file:line.
#[test]
fn fires_on_ghost_reference() {
    // The gate always loads the real `tool/trace/` (via
    // `read_all_trace_files`), so the fixture only needs to
    // contain a grep'd ref that isn't in the real set. LLR-999
    // is guaranteed not to exist.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let crate_src = tmp.path().join("crates").join("fake").join("src");
    std::fs::create_dir_all(&crate_src).expect("mkdir -p");
    std::fs::write(
        crate_src.join("lib.rs"),
        "//! See LLR-999 for details.\npub fn f() {}\n",
    )
    .expect("write fixture");

    // We need the real trace to load. The tempdir won't have
    // tool/trace/, so the scan can't run against it as a
    // self-contained workspace. Instead we synthesize a minimal
    // "fake workspace" that symlinks the real tool/trace:
    let real = workspace_root();
    let fake_trace = tmp.path().join("tool").join("trace");
    std::fs::create_dir_all(tmp.path().join("tool")).expect("mkdir tool");
    #[cfg(unix)]
    std::os::unix::fs::symlink(real.join("tool").join("trace"), &fake_trace)
        .expect("symlink trace");
    #[cfg(not(unix))]
    {
        // Windows fallback: copy the four toml files rather than
        // symlink. Cheap; they're small.
        std::fs::create_dir_all(&fake_trace).expect("mkdir trace");
        for name in ["sys.toml", "hlr.toml", "llr.toml", "tests.toml"] {
            std::fs::copy(
                real.join("tool").join("trace").join(name),
                fake_trace.join(name),
            )
            .expect("copy trace file");
        }
    }

    let hits = scan_tree(tmp.path());
    assert!(
        !hits.is_empty(),
        "expected gate to fire on LLR-999 ghost ref; hits were empty"
    );
    assert!(
        hits.iter().any(|(_, _, _, id)| id == "LLR-999"),
        "expected LLR-999 in hits; got {:?}",
        hits
    );
}

/// Negative dogfood: a fixture with only resolvable refs passes.
#[test]
fn passes_on_clean_fixture() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let crate_src = tmp.path().join("crates").join("clean").join("src");
    std::fs::create_dir_all(&crate_src).expect("mkdir -p");
    // LLR-001 is guaranteed to exist in the real trace (it's the
    // first landed LLR, `cmd_verify_jsonl emits a terminal`).
    std::fs::write(
        crate_src.join("lib.rs"),
        "//! See LLR-001 and SYS-001 for context.\npub fn stable() {}\n",
    )
    .expect("write fixture");

    let real = workspace_root();
    let fake_trace = tmp.path().join("tool").join("trace");
    std::fs::create_dir_all(tmp.path().join("tool")).expect("mkdir tool");
    #[cfg(unix)]
    std::os::unix::fs::symlink(real.join("tool").join("trace"), &fake_trace)
        .expect("symlink trace");
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(&fake_trace).expect("mkdir trace");
        for name in ["sys.toml", "hlr.toml", "llr.toml", "tests.toml"] {
            std::fs::copy(
                real.join("tool").join("trace").join(name),
                fake_trace.join(name),
            )
            .expect("copy trace file");
        }
    }

    let hits = scan_tree(tmp.path());
    assert!(
        hits.is_empty(),
        "expected clean fixture to pass; got hits {:?}",
        hits
    );
}
