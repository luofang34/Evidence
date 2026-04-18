//! Grep-regression for `DiagnosticCode` codes across the library.
//!
//! Walks every `crates/evidence/src/**/*.rs` file, finds `impl
//! DiagnosticCode for …` blocks, and extracts the string literals
//! returned from their `match` arms. Then enforces Schema Rule 3
//! plus the terminal-suffix namespace rule from Schema Rule 1:
//!
//! - Every walked code matches `^[A-Z][A-Z0-9]*(_[A-Z0-9]+)*$`.
//! - No two variants across the whole library return the same code.
//! - [`TERMINAL_CODES`] is internally unique.
//! - [`TERMINAL_CODES`] is disjoint from the walked registry (no code
//!   is both hand-emitted and returned from a `DiagnosticCode` impl).
//! - Every walked code ending in `_OK` / `_FAIL` / `_ERROR` is also in
//!   [`TERMINAL_CODES`] — catches the foot-gun of a future
//!   `TRACE_PARSE_ERROR` variant silently stealing a reserved suffix.
//! - Every [`TERMINAL_CODES`] entry ends in one of the three reserved
//!   terminal suffixes.
//!
//! The exhaustive `match self` in each `DiagnosticCode::code` impl is
//! the compile-time half of Schema Rule 3 — a new variant without a
//! stable code fails to compile. This test is the run-time half:
//! a renamed or copy-pasted literal that slipped past review still
//! fails here.
//!
//! # Out of scope: dead-code detection
//!
//! Schema Rule 3 does NOT promise detection of dead code strings: if a
//! variant is later removed but its code literal lingers in a `_ =>`
//! fallback arm or a stale comment, this test will not fire. The
//! compile-time exhaustiveness check prevents the common case (adding
//! a new variant without a code), but removed variants leave only a
//! dead return-path that the compiler can't flag. A contributor who
//! needs that guarantee must add a separate dead-code lint pass — this
//! test stays focused on the two positive invariants (regex +
//! uniqueness) plus the four terminal-namespace invariants above.
//!
//! This is deliberately a source-walking test rather than a reflection-
//! based one: Rust has no runtime reflection over trait impls, and
//! manually instantiating every error variant would require constructor
//! arguments that aren't universally available. Walking the source is
//! the pragmatic alternative, same shape as `schema_versions_locked`.
//!
//! [`TERMINAL_CODES`]: evidence::TERMINAL_CODES

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Walk `root` recursively; return every `.rs` file below it. Skips
/// `target/` so a `cargo doc` output tree can't taint the search.
fn rs_files(root: &Path, out: &mut Vec<PathBuf>) {
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

/// Extract (code, source-location) for every `"X" =>` arm inside every
/// `impl DiagnosticCode for … { fn code(&self) … }` block in `text`.
///
/// The parser is deliberately simple: we find `impl DiagnosticCode
/// for`, then track brace depth inside a `fn code(` definition,
/// collecting `"…"` literals that appear as the right-hand side of a
/// `=>` arm. Anything else (error messages inside `#[error(...)]`,
/// comments, unrelated string literals) is not matched because the
/// parser only activates inside the `fn code` body.
fn extract_codes(text: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();

    // Find every `impl DiagnosticCode for` start. The module's own
    // doc-comment might mention the phrase; restrict to occurrences
    // followed by an identifier + `{`, as a real impl header has.
    let needle = b"impl DiagnosticCode for";
    let mut search_start = 0;
    while let Some(rel) = find_subslice(&bytes[search_start..], needle) {
        let abs = search_start + rel;
        search_start = abs + needle.len();

        // Walk to the opening `{` of the impl body.
        let Some(impl_open) = bytes[abs..].iter().position(|&b| b == b'{') else {
            continue;
        };
        let impl_open = abs + impl_open;
        let impl_close = match_braces(bytes, impl_open);
        let impl_body = &bytes[impl_open + 1..impl_close];

        // Within the impl body, find `fn code(` and match its body.
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

        // Walk the fn body looking for `=> "CODE"` arms.
        let mut i = 0;
        while i + 2 < fn_body.len() {
            if fn_body[i] == b'=' && fn_body[i + 1] == b'>' {
                // Skip whitespace after `=>`.
                let mut j = i + 2;
                while j < fn_body.len() && fn_body[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < fn_body.len() && fn_body[j] == b'"' {
                    // Collect the string literal (no escapes expected
                    // in an UPPER_SNAKE_CASE code).
                    let start = j + 1;
                    let mut k = start;
                    while k < fn_body.len() && fn_body[k] != b'"' {
                        k += 1;
                    }
                    if k < fn_body.len() {
                        let code = std::str::from_utf8(&fn_body[start..k]).unwrap_or("");
                        // Character-count the bytes-before-the-arm to
                        // report a roughly accurate line. `fn_open +
                        // 1 + i` is the byte offset in the whole file
                        // of the `=>`.
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

fn code_is_valid(code: &str) -> bool {
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
            // Underscore must be followed by at least one [A-Z0-9].
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

#[test]
fn diagnostic_codes_locked() {
    let crate_root = workspace_root().join("crates").join("evidence").join("src");
    assert!(crate_root.is_dir(), "src/ not found at {:?}", crate_root);

    let mut files = Vec::new();
    rs_files(&crate_root, &mut files);
    files.sort();

    // code → list of (file, line) it was returned from; used to
    // pinpoint duplicates when they occur.
    let mut seen: BTreeMap<String, Vec<(PathBuf, usize)>> = BTreeMap::new();
    let mut invalid: Vec<(String, PathBuf, usize)> = Vec::new();

    for file in &files {
        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for (code, line) in extract_codes(&content) {
            if !code_is_valid(&code) {
                invalid.push((code.clone(), file.clone(), line));
            }
            seen.entry(code).or_default().push((file.clone(), line));
        }
    }

    // Must have collected something; if zero codes surfaced the parser
    // broke silently.
    assert!(
        !seen.is_empty(),
        "parser found no DiagnosticCode impls under {:?} — parser regression?",
        crate_root
    );

    // Regex gate.
    assert!(
        invalid.is_empty(),
        "codes violating UPPER_SNAKE_CASE pattern:\n{}",
        invalid
            .iter()
            .map(|(code, file, line)| format!("  {:?}:{} — '{}'", file, line, code))
            .collect::<Vec<_>>()
            .join("\n")
    );

    // Uniqueness gate.
    let duplicates: Vec<(&String, &Vec<(PathBuf, usize)>)> = seen
        .iter()
        .filter(|(_, locations)| locations.len() > 1)
        .collect();
    assert!(
        duplicates.is_empty(),
        "codes declared more than once (Schema Rule 3):\n{}",
        duplicates
            .iter()
            .map(|(code, locations)| {
                format!(
                    "  {} returned from:\n{}",
                    code,
                    locations
                        .iter()
                        .map(|(f, l)| format!("    {:?}:{}", f, l))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    );

    // =================================================================
    // Terminal-namespace invariants (Schema Rule 1 / reserved suffixes)
    // =================================================================
    //
    // The four assertions below enforce the terminal-codes contract.
    // `TERMINAL_CODES` is the single source of truth for hand-emitted
    // terminals; every invariant below reads from it. If a future
    // contributor adds a new hand-emitted terminal in the CLI without
    // appending it to `TERMINAL_CODES`, the disjointness test won't
    // fire (the CLI isn't walked) — but the test inside
    // `crates/evidence/src/diagnostic.rs` (`terminal_codes_all_end_in
    // _reserved_suffix`) anchors the slice's shape, and the CLI has
    // integration tests that exercise the terminal by matching against
    // the slice. That pair is sufficient.

    let terminal_codes: std::collections::BTreeSet<&str> =
        evidence::TERMINAL_CODES.iter().copied().collect();

    // Invariant (1): `TERMINAL_CODES` internal uniqueness.
    assert_eq!(
        terminal_codes.len(),
        evidence::TERMINAL_CODES.len(),
        "TERMINAL_CODES contains duplicates: {:?}",
        evidence::TERMINAL_CODES
    );

    // Invariant (4): every `TERMINAL_CODES` entry ends in a reserved
    // terminal suffix. (Placed before (2)/(3) because it anchors the
    // meaning of "reserved suffix" used in (3).)
    let bad_suffix: Vec<&str> = evidence::TERMINAL_CODES
        .iter()
        .copied()
        .filter(|c| !ends_in_terminal_suffix(c))
        .collect();
    assert!(
        bad_suffix.is_empty(),
        "TERMINAL_CODES entries must end in _OK / _FAIL / _ERROR; offenders: {:?}",
        bad_suffix
    );

    // Invariant (2): `TERMINAL_CODES` is disjoint from the walked
    // registry. A code can be walked-from-an-impl OR hand-emitted,
    // never both.
    let walked_as_terminal: Vec<&String> = seen
        .keys()
        .filter(|k| terminal_codes.contains(k.as_str()))
        .collect();
    assert!(
        walked_as_terminal.is_empty(),
        "codes in TERMINAL_CODES must not also be returned from DiagnosticCode impls; overlap: {:?}",
        walked_as_terminal
    );

    // Invariant (3): every walked code ending in a reserved terminal
    // suffix must be in `TERMINAL_CODES`. Catches the foot-gun of a
    // future `TRACE_PARSE_ERROR` variant silently promoting itself
    // into the terminal namespace without being registered.
    let reserved_walked: Vec<&String> = seen
        .keys()
        .filter(|k| ends_in_terminal_suffix(k) && !terminal_codes.contains(k.as_str()))
        .collect();
    assert!(
        reserved_walked.is_empty(),
        "walked-registry codes end in a reserved terminal suffix but are not in TERMINAL_CODES \
         (either the code should be moved to TERMINAL_CODES, or it should be renamed to drop the \
         reserved suffix): {:?}",
        reserved_walked
    );
}

fn ends_in_terminal_suffix(code: &str) -> bool {
    code.ends_with("_OK") || code.ends_with("_FAIL") || code.ends_with("_ERROR")
}

#[test]
fn code_regex_validator_catches_known_shapes() {
    // Happy path.
    assert!(code_is_valid("VERIFY_OK"));
    assert!(code_is_valid("HASH_OPEN_FAILED"));
    assert!(code_is_valid("BUNDLE_TOCTOU"));
    assert!(code_is_valid("POLICY_UNKNOWN_DAL"));
    // Single-segment codes still pass.
    assert!(code_is_valid("VERIFY"));
    // Digits allowed inside a segment.
    assert!(code_is_valid("SCHEMA_V1_COMPILE"));

    // Rejected shapes.
    assert!(!code_is_valid(""));
    assert!(!code_is_valid("verify_ok")); // lowercase start
    assert!(!code_is_valid("Verify_Ok")); // mixed case
    assert!(!code_is_valid("VERIFY__OK")); // empty segment
    assert!(!code_is_valid("VERIFY_")); // trailing underscore
    assert!(!code_is_valid("_VERIFY_OK")); // leading underscore
    assert!(!code_is_valid("VERIFY-OK")); // hyphen forbidden
    assert!(!code_is_valid("0VERIFY")); // digit start
}
