//! Grep-regression for `evidence::schema_versions`.
//!
//! Walks the workspace source tree under `crates/` and fails if any
//! file contains a double-quoted `"0.0.[0-9]+"`-shaped string
//! outside the single source of truth. If this test goes red,
//! either:
//!
//! - a new schema-version literal crept in — replace it with the
//!   matching constant in `evidence::schema_versions`;
//! - or the version is itself in `schema_versions.rs` because you
//!   intentionally bumped the schema version (the file is excluded
//!   from the walk).
//!
//! The committed golden fixture under `tests/fixtures/` contains
//! captured schema-version strings by design (it's a frozen bundle
//! byte-identical to what the tool produced at generation time), so
//! that directory is excluded. Cargo's `target/` is also skipped.

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

fn is_version_literal(line: &str) -> bool {
    // Pure byte-level scan for a double-quoted `0.0.<digit+>` string.
    // Deliberately avoids the `regex` crate — this is a tiny predicate
    // and adding a dep just for one test is unjustified.
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 6 < bytes.len() {
        if bytes[i] == b'"'
            && bytes[i + 1] == b'0'
            && bytes[i + 2] == b'.'
            && bytes[i + 3] == b'0'
            && bytes[i + 4] == b'.'
            && bytes[i + 5].is_ascii_digit()
        {
            // Walk the remaining digits.
            let mut j = i + 6;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'"' {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn workspace_root() -> PathBuf {
    // This test lives in `crates/evidence/tests/`.
    // The workspace root is two levels up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn is_excluded(rel: &Path) -> bool {
    // Normalize the path to forward slashes before substring-matching
    // so the same rules apply on Windows (native `\`) and Unix (`/`).
    let s = rel.to_string_lossy().replace('\\', "/");

    // The source of truth itself is allowed to declare the constants.
    if s.ends_with("evidence/src/schema_versions.rs") {
        return true;
    }
    // This regression test's own source contains the search needle.
    if s.ends_with("evidence/tests/schema_versions_locked.rs") {
        return true;
    }
    // Committed frozen evidence bundle — captured bytes are the point.
    if s.contains("tests/fixtures/") {
        return true;
    }
    if s.contains("/target/") {
        return true;
    }
    false
}

fn walk(root: &Path, hits: &mut Vec<(PathBuf, usize, String)>) {
    for entry in traversal::walk(root)
        .filter_entry(|e| !traversal::is_dir_named(e, &["target"]))
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        // Restrict to file types that could legitimately carry a
        // schema version string.
        if !matches!(ext, "rs" | "toml" | "json" | "md") {
            continue;
        }
        // Skip the project's own Cargo.lock: it's under the workspace
        // root, not a crate; our walker starts from `crates/` so we
        // never see it, but the filter is a belt-and-suspenders.
        if path.file_name().and_then(|n| n.to_str()) == Some("Cargo.lock") {
            continue;
        }
        let rel = path
            .strip_prefix(workspace_root())
            .unwrap_or(path)
            .to_path_buf();
        if is_excluded(&rel) {
            continue;
        }
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for (lineno, line) in content.lines().enumerate() {
            if is_version_literal(line) {
                hits.push((rel.clone(), lineno + 1, line.trim().to_string()));
            }
        }
    }
}

#[test]
fn no_version_literals_outside_schema_versions() {
    let crates_root = workspace_root().join("crates");
    assert!(
        crates_root.is_dir(),
        "crates/ not found at {:?}",
        crates_root
    );

    let mut hits = Vec::new();
    walk(&crates_root, &mut hits);

    if !hits.is_empty() {
        let mut msg = String::from(
            "Found version-string literals outside evidence::schema_versions. \
             Replace each with the matching constant (INDEX, BOUNDARY, TRACE, \
             COMPLIANCE), or update schema_versions.rs if you're intentionally \
             bumping a schema version:\n",
        );
        for (path, line, text) in &hits {
            msg.push_str(&format!("  {}:{}  {}\n", path.display(), line, text));
        }
        panic!("{}", msg);
    }
}

#[test]
fn test_detector_recognizes_versionish_strings() {
    assert!(is_version_literal(r#"version = "0.0.1""#));
    assert!(is_version_literal(r#"let s = "0.0.42";"#));
    // Multi-digit patch number is still a version.
    assert!(is_version_literal(r#""0.0.100""#));

    // Shapes that must NOT trigger.
    assert!(!is_version_literal(r#"version = "0.1.0""#));
    assert!(!is_version_literal(r#""1.0.0""#));
    assert!(!is_version_literal(r#"Schema version: 0.0.1"#)); // unquoted
    assert!(!is_version_literal(r#""0.0.""#)); // no digits
    assert!(!is_version_literal(r#""0.0.1"#)); // missing closing quote
}

/// pre-ship pin: every `pub const X: &str = "..."` in
/// `schema_versions.rs` must equal `"0.0.1"`. Codifies the project
/// convention that schema versions don't churn before the 1.0 ship;
/// breaking shape changes rewrite rule text in place. If a future
/// 1.0 release tier introduces genuine versioning, drop this test
/// and rewire `cargo semver-checks` or equivalent.
#[test]
fn schema_constants_pinned_at_001() {
    let path = workspace_root().join("crates/evidence/src/schema_versions.rs");
    let text =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {}", path.display(), e));

    // Simple byte-level scan: find every `pub const <NAME>: &str = "<value>";`
    // line; assert the value equals "0.0.1".
    let mut drifted: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("pub const ") || !trimmed.contains(": &str") {
            continue;
        }
        // Extract NAME (between `pub const ` and `:`).
        let Some(after_const) = trimmed.strip_prefix("pub const ") else {
            continue;
        };
        let Some(colon_pos) = after_const.find(':') else {
            continue;
        };
        let name = after_const[..colon_pos].trim().to_string();
        // Extract the quoted value.
        let Some(eq_pos) = trimmed.find('=') else {
            continue;
        };
        let after_eq = &trimmed[eq_pos + 1..];
        let Some(open_q) = after_eq.find('"') else {
            continue;
        };
        let rest = &after_eq[open_q + 1..];
        let Some(close_q) = rest.find('"') else {
            continue;
        };
        let value = rest[..close_q].to_string();
        if value != "0.0.1" {
            drifted.push((name, value));
        }
    }

    assert!(
        drifted.is_empty(),
        "schema constants drifted from the pre-ship pin of \"0.0.1\":\n{}\n\
         Reset these to \"0.0.1\" — schema versions don't bump pre-ship; \
         breaking shape changes rewrite rule text in place. If this is a \
         genuine 1.0 ship, drop this test.",
        drifted
            .iter()
            .map(|(n, v)| format!("  {} = \"{}\"", n, v))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
