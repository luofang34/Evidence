//! Gate against library-layer `tracing::error!` calls in
//! `evidence-core`.
//!
//! **The anti-pattern this prevents.** The library layer
//! (`evidence-core`) must not own user-facing presentation of typed
//! error results. `tracing::error!` in a function that also
//! *returns* a typed error (`Result<_, VerifyError>`,
//! `Result<_, TraceValidationError>`, …) is a presentation policy
//! the CLI already owns:
//!
//! 1. The CLI has two rendering paths — human (prose on
//!    stdout/stderr) and JSONL (structured `Diagnostic` on stdout,
//!    silent stderr per Schema Rule 2). Both emit every error the
//!    library returns.
//! 2. The CLI can downgrade individual errors' severity before
//!    rendering (see `cmd_verify` / `cmd_verify_jsonl`, which
//!    downgrade `VERIFY_PRERELEASE_TOOL` on `Profile::Dev` to
//!    Warning + `VERIFY_OK` + exit 0).
//! 3. A library-layer `tracing::error!` fires *before* the CLI
//!    partition runs. Default `tracing_subscriber` level is WARN
//!    (see `cargo-evidence/src/main.rs:64-74`), so `error!` always
//!    prints — even when the outcome was downgraded to Warning +
//!    exit 0. That is exactly what caused
//!    `ERROR VERIFY ERROR: …` to appear on stderr despite exit 0
//!    on the dev-profile prerelease verify path; removing the loop
//!    in `verify::bundle` and here in `trace::validation` is the
//!    full fix.
//!
//! **Scope.** Walks `crates/evidence-core/src/**/*.rs` and fails if
//! any non-comment line contains `tracing::error!(`. Only
//! `evidence-core` is scanned; the CLI crate (`cargo-evidence`) can
//! legitimately `tracing::error!` because it owns presentation.
//!
//! **Allowlist.** [`ALLOWED_FILES`] names files where
//! `tracing::error!` is acceptable. Today that's exactly
//! `bundle/builder.rs` for the cargo-test-subprocess-failure path
//! (different category — dumps a spawned subprocess's stderr to the
//! operator, not a Result-bearing validation path). Adding a file
//! requires written justification.
//!
//! **Mirrors** `walker_usage_locked` / `rot_prone_markers_locked` /
//! `schema_versions_locked` — no `Diagnostic` wire shape, no
//! `RULES` entry, the test's failure message is the diagnostic.

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

/// Files where `tracing::error!` is allowed in `evidence-core/src`.
/// Suffix-match against the workspace-relative path.
///
/// Today: the cargo-test subprocess wrapper in `bundle/builder.rs`
/// dumps the spawned process's stderr + non-zero exit code when a
/// gate command fails. That's diagnostic output for an opaque
/// external process, not a Result-bearing library validation — and
/// the caller is always the CLI's `generate` pipeline, which
/// immediately surfaces the failure to the user. Different category,
/// different trade-off.
///
/// Grow this list only with written justification per-file; the
/// point of the gate is to catch the "oh I'll just log it here"
/// instinct before it reintroduces the bug.
const ALLOWED_FILES: &[&str] = &["crates/evidence-core/src/bundle/builder.rs"];

/// Substring needle for library-layer `tracing::error!` calls.
/// Includes the opening paren so prose mentions like "calls
/// `tracing::error!`" in docstrings don't match.
const NEEDLE: &str = "tracing::error!(";

fn is_allowed(rel: &str) -> bool {
    let normalized = rel.replace('\\', "/");
    ALLOWED_FILES
        .iter()
        .any(|p| normalized == *p || normalized.ends_with(p))
}

/// True iff the needle at `needle_idx` is inside a `//` line comment
/// on the same line. Block comments (`/* */`) are not recognized;
/// the library doesn't use them today and the bluntness is acceptable
/// — a future block-comment mention would false-fire and require a
/// targeted exemption, which is the right escalation.
fn needle_in_comment(line: &str, needle_idx: usize) -> bool {
    match line.find("//") {
        Some(comment_start) => comment_start < needle_idx,
        None => false,
    }
}

/// Scan every `.rs` file under `crates/evidence-core/src/` for
/// `tracing::error!(` calls outside `ALLOWED_FILES` and outside
/// `//` comments.
fn scan_for_hits(workspace: &Path) -> Vec<(String, usize, String)> {
    let src_root = workspace.join("crates").join("evidence-core").join("src");
    let files: Vec<PathBuf> = traversal::walk(&src_root)
        .filter_entry(|e| !traversal::is_dir_named(e, &["target", ".git"]))
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && traversal::has_ext(e.path(), "rs"))
        .map(|e| e.into_path())
        .collect();

    let mut hits: Vec<(String, usize, String)> = Vec::new();
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
            if let Some(idx) = line.find(NEEDLE)
                && !needle_in_comment(line, idx)
            {
                hits.push((rel.clone(), lineno + 1, line.trim().to_string()));
            }
        }
    }
    hits.sort();
    hits
}

#[test]
fn no_unauthorized_tracing_error_in_library() {
    let hits = scan_for_hits(&workspace_root());
    assert!(
        hits.is_empty(),
        "found {} `tracing::error!(` call site(s) in `crates/evidence-core/src/` \
         outside the allowlist. Library code must not log at error level — the \
         CLI owns severity presentation (and can downgrade individual errors to \
         Warning + exit 0). A library log fires before the CLI partition runs \
         and leaks `ERROR` to stderr even when the outcome was a warning.\n\n\
         Either (a) delete the log and let the caller render the returned \
         `Result<_, E>`, or (b) if the log is truly presentation-owning \
         (subprocess-stderr dump, panic-adjacent invariant) add the file to \
         `ALLOWED_FILES` in `tests/library_no_tracing_error_locked.rs` with \
         written justification.\n\n{}",
        hits.len(),
        hits.iter()
            .map(|(f, l, line)| format!("  {}:{}  {}", f, l, line))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

#[test]
fn fires_on_unallowlisted_call() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("crates").join("evidence-core").join("src");
    std::fs::create_dir_all(&src).expect("mkdir -p");
    std::fs::write(
        src.join("lib.rs"),
        "pub fn f() { tracing::error!(\"nope\"); }\n",
    )
    .expect("write fixture");

    let hits = scan_for_hits(tmp.path());
    assert!(
        !hits.is_empty(),
        "positive dogfood: planted `tracing::error!(` call wasn't detected",
    );
}

#[test]
fn ignores_call_inside_line_comment() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("crates").join("evidence-core").join("src");
    std::fs::create_dir_all(&src).expect("mkdir -p");
    std::fs::write(
        src.join("lib.rs"),
        "// tracing::error!(\"in a comment, ignored\");\npub fn g() {}\n",
    )
    .expect("write fixture");

    let hits = scan_for_hits(tmp.path());
    assert!(
        hits.is_empty(),
        "false-fire on comment-only mention: {:?}",
        hits
    );
}
