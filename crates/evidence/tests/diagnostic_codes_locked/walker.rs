//! Source-walking helpers for `diagnostic_codes_locked`.
//!
//! Split out of the parent integration-test file to stay under the
//! 500-line workspace file-size limit. Everything here is the grep-
//! regression machinery; the actual `#[test]` cases live in
//! `diagnostic_codes_locked.rs`.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Walk `root` recursively; return every `.rs` file below it. Skips
/// `target/` so a `cargo doc` output tree can't taint the search.
pub fn rs_files(root: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            rs_files(&path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Strip `//` single-line comments from `text`, preserving newlines
/// so line numbers stay accurate. Sufficient for Rust source because
/// doc-comment code literals (`//! impl DiagnosticCode for …`) would
/// otherwise pollute the walked set with examples like
/// `MY_BAD_CHECKSUM` that aren't real impls. `//` inside a string
/// literal would be a corner case; the walked targets are all
/// UPPER_SNAKE_CASE codes so the conservative stripper is safe.
pub fn strip_line_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        if let Some(cut) = line.find("//") {
            out.push_str(&line[..cut]);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// Extract (code, source-location) for every `"X" =>` arm inside every
/// `impl DiagnosticCode for … { fn code(&self) … }` block in `text`.
///
/// The parser is deliberately simple: find `impl DiagnosticCode for`,
/// track brace depth inside `fn code(`, collect `"…"` literals on the
/// right-hand side of `=>`. Everything else (error messages inside
/// `#[error(…)]`, unrelated string literals) is not matched because
/// the parser only activates inside `fn code` body.
pub fn extract_codes(text: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let needle = b"impl DiagnosticCode for";
    let mut search_start = 0;
    while let Some(rel) = find_subslice(&bytes[search_start..], needle) {
        let abs = search_start + rel;
        search_start = abs + needle.len();

        let Some(impl_open) = bytes[abs..].iter().position(|&b| b == b'{') else {
            continue;
        };
        let impl_open = abs + impl_open;
        let impl_close = match_braces(bytes, impl_open);
        let impl_body = &bytes[impl_open + 1..impl_close];

        let fn_needle = b"fn code(";
        let Some(rel_fn) = find_subslice(impl_body, fn_needle) else {
            continue;
        };
        let fn_abs = impl_open + 1 + rel_fn;
        let Some(fn_open_rel) = bytes[fn_abs..].iter().position(|&b| b == b'{') else {
            continue;
        };
        let fn_open = fn_abs + fn_open_rel;
        let fn_close = match_braces(bytes, fn_open);
        let fn_body = &bytes[fn_open + 1..fn_close];

        let mut i = 0;
        while i + 2 < fn_body.len() {
            if fn_body[i] == b'=' && fn_body[i + 1] == b'>' {
                let mut j = i + 2;
                while j < fn_body.len() && fn_body[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < fn_body.len() && fn_body[j] == b'"' {
                    let start = j + 1;
                    let mut k = start;
                    while k < fn_body.len() && fn_body[k] != b'"' {
                        k += 1;
                    }
                    if k < fn_body.len() {
                        let code = std::str::from_utf8(&fn_body[start..k]).unwrap_or("");
                        let byte_offset = fn_open + 1 + i;
                        let line = 1 + bytes[..byte_offset].iter().filter(|&&b| b == b'\n').count();
                        out.push((code.to_string(), line));
                        i = k + 1;
                        continue;
                    }
                }
            }
            i += 1;
        }
    }
    out
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Given `open_idx` pointing at `{`, return the index of its matching
/// `}`. Aborts (panic) if unbalanced — a compile failure would fire
/// first in that case, so this is safety belt only.
fn match_braces(bytes: &[u8], open_idx: usize) -> usize {
    let mut depth: i32 = 0;
    let mut i = open_idx;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
        i += 1;
    }
    panic!("unbalanced braces at {}", open_idx);
}

pub fn code_is_valid(code: &str) -> bool {
    // `^[A-Z][A-Z0-9]*(_[A-Z0-9]+)*$` — same pattern locked by
    // `schemas/diagnostic.schema.json`.
    let bytes = code.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_uppercase() {
        return false;
    }
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'_' {
            if i + 1 >= bytes.len() {
                return false;
            }
            let next = bytes[i + 1];
            if !next.is_ascii_uppercase() && !next.is_ascii_digit() {
                return false;
            }
            i += 1;
            continue;
        }
        if !c.is_ascii_uppercase() && !c.is_ascii_digit() {
            return false;
        }
        i += 1;
    }
    true
}

pub fn ends_in_terminal_suffix(code: &str) -> bool {
    code.ends_with("_OK") || code.ends_with("_FAIL") || code.ends_with("_ERROR")
}

/// Walk the library source and return the set of codes returned from
/// every `impl DiagnosticCode::code()` arm. Used by both the existing
/// regex/uniqueness check and the bijection invariants so a
/// parser change is reflected in all four.
pub fn walked_codes() -> std::collections::BTreeSet<String> {
    let crate_root = workspace_root().join("crates").join("evidence").join("src");
    let mut files = Vec::new();
    rs_files(&crate_root, &mut files);
    files.sort();
    let mut out: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for file in &files {
        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let stripped = strip_line_comments(&content);
        for (code, _line) in extract_codes(&stripped) {
            out.insert(code);
        }
    }
    out
}
