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

/// Strip `//` single-line comments from `text`, preserving newlines
/// so line numbers stay accurate. Sufficient for Rust source because
/// doc-comment code literals (`//! impl DiagnosticCode for …`) would
/// otherwise pollute the walked set with examples like
/// `MY_BAD_CHECKSUM` that aren't real impls. `//` inside a string
/// literal would be a corner case; the walked targets are all
/// UPPER_SNAKE_CASE codes so the conservative stripper is safe.
fn strip_line_comments(text: &str) -> String {
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
        let stripped = strip_line_comments(&content);
        for (code, line) in extract_codes(&stripped) {
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

// ============================================================================
// PR #47 bijection invariants — source ↔ RULES ↔ LLR.emits closed loop.
// ============================================================================
//
// Four new invariants tighten the "what can the tool say" contract to
// match what the RULES manifest advertises and what the trace
// requirements claim. Together with the existing walk-level regex +
// uniqueness checks above and PR #43's terminal-namespace set, these
// close the code↔test↔requirement loop: adding a code without an
// RULES entry, an LLR without a test, or a test without a selector
// all fail CI with targeted messages.
//
// The walking logic is shared with `diagnostic_codes_locked` above —
// `walked_codes()` re-runs the same extraction on the library source
// tree so a parser change in `extract_codes` is reflected in both.

fn walked_codes() -> std::collections::BTreeSet<String> {
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

/// PR #47 invariant (1): every code returned from a library
/// `DiagnosticCode::code()` impl is declared in [`RULES`]. Adding a
/// variant without updating `RULES` fires here with the orphan name.
#[test]
fn rules_contains_every_code() {
    let walked = walked_codes();
    let rules: std::collections::BTreeSet<&str> =
        evidence::RULES.iter().map(|r| r.code).collect();
    let missing: Vec<&String> = walked.iter().filter(|c| !rules.contains(c.as_str())).collect();
    assert!(
        missing.is_empty(),
        "codes returned from DiagnosticCode::code() but missing from RULES \
         (add a `RuleEntry` in crates/evidence/src/rules.rs): {:?}",
        missing
    );
}

/// PR #47 invariant (2): every non-terminal, non-hand-emitted
/// `RULES` entry is backed by a real `DiagnosticCode::code()` impl in
/// the library. Terminal codes live in `TERMINAL_CODES` (CLI-emitted,
/// not walked); hand-emitted non-terminal CLI codes live in
/// `HAND_EMITTED_CLI_CODES`. Everything else must be a library impl.
/// A stale `RULES` entry naming a code no impl returns fires here.
#[test]
fn every_rules_entry_is_implemented() {
    let walked = walked_codes();
    let terminals: std::collections::BTreeSet<&str> =
        evidence::TERMINAL_CODES.iter().copied().collect();
    let hand_emitted: std::collections::BTreeSet<&str> =
        evidence::HAND_EMITTED_CLI_CODES.iter().copied().collect();

    let orphans: Vec<&str> = evidence::RULES
        .iter()
        .filter(|r| !terminals.contains(r.code) && !hand_emitted.contains(r.code))
        .map(|r| r.code)
        .filter(|c| !walked.contains(*c))
        .collect();

    assert!(
        orphans.is_empty(),
        "RULES entries name codes with no DiagnosticCode impl, TERMINAL_CODES \
         entry, or HAND_EMITTED_CLI_CODES entry (delete the stale RULES row \
         or restore its backing): {:?}",
        orphans
    );
}

/// PR #47 invariant (3): the set of `RULES` entries with `terminal =
/// true` equals `TERMINAL_CODES` exactly. Promoting a code to a
/// terminal without updating both lists, or vice versa, fires here.
#[test]
fn rules_terminal_set_matches_terminal_codes() {
    let rules_terminals: std::collections::BTreeSet<&str> = evidence::RULES
        .iter()
        .filter(|r| r.terminal)
        .map(|r| r.code)
        .collect();
    let global_terminals: std::collections::BTreeSet<&str> =
        evidence::TERMINAL_CODES.iter().copied().collect();

    let only_in_rules: Vec<&&str> = rules_terminals.difference(&global_terminals).collect();
    let only_in_terminals: Vec<&&str> = global_terminals.difference(&rules_terminals).collect();

    assert!(
        only_in_rules.is_empty() && only_in_terminals.is_empty(),
        "RULES.terminal set != TERMINAL_CODES\n\
         only in RULES.terminal: {:?}\n\
         only in TERMINAL_CODES: {:?}",
        only_in_rules,
        only_in_terminals
    );
}

/// PR #47 invariant (4) — THE sync check. Every code in `RULES`
/// (minus `RESERVED_UNCLAIMED_CODES`) is claimed by at least one
/// `LLR.emits` list in `tool/trace/llr.toml`, and every
/// `LLR.emits` string names a real `RULES` code. Adding a code
/// without writing an owning LLR fails here; an LLR claiming a dead
/// code also fails here.
///
/// Loads the trace via `read_all_trace_files` so the test exercises
/// the production parsing path — a future change in the loader can't
/// silently make this check pass on a malformed input.
#[test]
fn every_code_is_claimed_by_an_llr() {
    let trace = evidence::read_all_trace_files(
        workspace_root()
            .join("tool")
            .join("trace")
            .to_str()
            .expect("workspace path is UTF-8"),
    )
    .expect("tool/trace must load");

    let rules: std::collections::BTreeSet<&str> =
        evidence::RULES.iter().map(|r| r.code).collect();
    let reserved: std::collections::BTreeSet<&str> =
        evidence::RESERVED_UNCLAIMED_CODES.iter().copied().collect();
    let claimed: std::collections::BTreeSet<String> = trace
        .llr
        .requirements
        .iter()
        .flat_map(|l| l.emits.iter().cloned())
        .collect();

    // 4a: every LLR.emits entry is a real RULES code.
    let dead: Vec<&String> = claimed.iter().filter(|c| !rules.contains(c.as_str())).collect();
    assert!(
        dead.is_empty(),
        "LLR.emits refers to code(s) not in RULES (typo, stale reference, or \
         the code was deleted and the LLR wasn't updated): {:?}",
        dead
    );

    // 4b: every RULES code (minus explicit reserved set) is claimed.
    let unclaimed: Vec<&str> = evidence::RULES
        .iter()
        .map(|r| r.code)
        .filter(|c| !reserved.contains(*c) && !claimed.contains(*c))
        .collect();
    assert!(
        unclaimed.is_empty(),
        "RULES code(s) not claimed by any LLR.emits in tool/trace/llr.toml \
         (add the code to an LLR that owns its emit path, or add an \
         RESERVED_UNCLAIMED_CODES entry with written justification): {:?}",
        unclaimed
    );
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
