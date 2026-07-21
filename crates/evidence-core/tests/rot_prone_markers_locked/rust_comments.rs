//! Comment extractor for the rot-gate (`rot_prone_markers_locked`,
//! LLR-044).
//!
//! `.rs` files are scanned comment-scoped: the pinned banned set
//! applies to line comments (`//`), doc comments (`///` outer,
//! `//!` inner), and block comments (`/* */`, including the doc
//! forms `/** */` and `/*! */`) — and to nothing else. Runtime
//! data (string literals, raw strings, byte strings, char
//! literals) carrying the same words stays data, not narration.
//!
//! [`extract`] is a hand-rolled byte-level state machine — code,
//! line comment, block comment (depth-tracked: Rust block comments
//! nest), string, raw string, char literal — with no dependencies.
//! It lexes just enough Rust to keep comment markers inside string
//! data from opening phantom comments, and quotes inside comments
//! from opening phantom strings:
//!
//! - `//` and `/*` open comments only in code state.
//! - `"` opens a string; `\` escapes the next byte. `b"` is the
//!   byte-string form, same rules.
//! - `r"` / `r#"` / `r##"` … (and the `br` byte forms) open raw
//!   strings: the closing quote must be followed by exactly the
//!   opening hash count. A raw identifier (`r#type`) is not an
//!   opener because no quote follows the hashes.
//! - `'` is the one genuinely ambiguous byte. Heuristic: a quote
//!   followed by `\` is a char literal (escape sequence); a quote
//!   with a closing quote two bytes later is a single-character
//!   literal (covers letters, digits, whitespace, and a literal
//!   `"`); anything else is a lifetime (`'a`, `'static`) and only
//!   the quote byte itself is consumed. A multi-byte char literal
//!   (an emoji, say) misclassifies as a lifetime, which is
//!   harmless: its interior bytes are all >= 0x80 and can never
//!   spell a comment or string marker, so the scan stays exact.
//! - A newline inside any comment flushes the comment text up to
//!   (not including) that newline as one `(line_number, text)`
//!   item, so reported `file:line` positions match an editor's.
//!
//! Input is assumed to be valid Rust (the tree compiles; fixtures
//! are written valid). Known limit, documented rather than handled:
//! `c"..."` / `cr"..."` C-string literals lex here as identifier
//! byte plus ordinary string. Their payload is still skipped as
//! string data; only a C raw string whose payload ends in a
//! backslash could desynchronize the scan, and none exist in this
//! tree.

/// Byte-level lexer state. `Copy` so the active state can be matched
/// by value and written back after mutation (block-comment depth).
#[derive(Clone, Copy)]
enum State {
    Code,
    LineComment,
    BlockComment { depth: usize },
    Str,
    RawStr { hashes: usize },
    Char,
}

/// Extract every comment line from Rust source as
/// `(1-based line number, comment text)` pairs. The text keeps its
/// comment markers (`//`, `/*`, `*/`) and drops the trailing
/// newline; a trailing `\r` is stripped so CRLF files report the
/// same text as their LF form. See the module doc for the state
/// machine and its documented heuristics.
pub fn extract(source: &str) -> Vec<(usize, &str)> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut line = 1usize;
    // Start (byte offset, line number) of the comment text on the
    // current line; only meaningful inside a comment state.
    let mut seg_start = 0usize;
    let mut seg_line = 1usize;
    let mut state = State::Code;

    while pos < bytes.len() {
        let b = bytes[pos];
        match state {
            State::Code => match b {
                b'/' if bytes.get(pos + 1) == Some(&b'/') => {
                    state = State::LineComment;
                    seg_start = pos;
                    seg_line = line;
                    pos += 2;
                }
                b'/' if bytes.get(pos + 1) == Some(&b'*') => {
                    // `/*` is always a comment opener in Rust — the
                    // language reserves it, so `a / *b` division-then-
                    // deref must be written with a space.
                    state = State::BlockComment { depth: 1 };
                    seg_start = pos;
                    seg_line = line;
                    pos += 2;
                }
                b'"' => {
                    state = State::Str;
                    pos += 1;
                }
                b'r' => {
                    if let Some(hashes) = raw_string_hashes(bytes, pos + 1) {
                        state = State::RawStr { hashes };
                        pos += 1 + hashes + 1;
                    } else {
                        pos += 1;
                    }
                }
                b'b' if bytes.get(pos + 1) == Some(&b'"') => {
                    state = State::Str;
                    pos += 2;
                }
                b'b' if bytes.get(pos + 1) == Some(&b'r') => {
                    if let Some(hashes) = raw_string_hashes(bytes, pos + 2) {
                        state = State::RawStr { hashes };
                        pos += 2 + hashes + 1;
                    } else {
                        pos += 1;
                    }
                }
                b'\'' => {
                    if is_char_literal_start(bytes, pos) {
                        state = State::Char;
                    }
                    pos += 1;
                }
                _ => {
                    if b == b'\n' {
                        line += 1;
                    }
                    pos += 1;
                }
            },
            State::LineComment => {
                if b == b'\n' {
                    push_segment(&mut out, source, seg_start, pos, seg_line);
                    line += 1;
                    pos += 1;
                    state = State::Code;
                } else {
                    pos += 1;
                }
            }
            State::BlockComment { mut depth } => match b {
                b'/' if bytes.get(pos + 1) == Some(&b'*') => {
                    depth += 1;
                    state = State::BlockComment { depth };
                    pos += 2;
                }
                b'*' if bytes.get(pos + 1) == Some(&b'/') => {
                    depth -= 1;
                    pos += 2;
                    if depth == 0 {
                        push_segment(&mut out, source, seg_start, pos, seg_line);
                        state = State::Code;
                    } else {
                        state = State::BlockComment { depth };
                    }
                }
                b'\n' => {
                    push_segment(&mut out, source, seg_start, pos, seg_line);
                    line += 1;
                    pos += 1;
                    seg_start = pos;
                    seg_line = line;
                }
                _ => pos += 1,
            },
            State::Str => match b {
                b'\\' => pos += 2,
                b'"' => {
                    state = State::Code;
                    pos += 1;
                }
                _ => {
                    if b == b'\n' {
                        line += 1;
                    }
                    pos += 1;
                }
            },
            State::RawStr { hashes } => {
                if b == b'"' && closing_hashes(bytes, pos + 1, hashes) {
                    state = State::Code;
                    pos += 1 + hashes;
                } else {
                    if b == b'\n' {
                        line += 1;
                    }
                    pos += 1;
                }
            }
            State::Char => match b {
                b'\\' => pos += 2,
                b'\'' => {
                    state = State::Code;
                    pos += 1;
                }
                _ => {
                    if b == b'\n' {
                        line += 1;
                    }
                    pos += 1;
                }
            },
        }
    }

    // Unterminated comment at EOF (invalid Rust, but the gate scans
    // whatever is on disk): flush what was accumulated.
    if matches!(state, State::LineComment | State::BlockComment { .. }) {
        push_segment(&mut out, source, seg_start, bytes.len(), seg_line);
    }
    out
}

/// If a raw-string opener follows (`"` or `#`×N then `"`), return
/// the hash count. `pos` points just past the `r`. A raw identifier
/// (`r#type`) yields `None` because no quote follows the hashes.
fn raw_string_hashes(bytes: &[u8], mut pos: usize) -> Option<usize> {
    let mut hashes = 0;
    while bytes.get(pos) == Some(&b'#') {
        hashes += 1;
        pos += 1;
    }
    (bytes.get(pos) == Some(&b'"')).then_some(hashes)
}

/// Are the `hashes` bytes starting at `pos` all `#`? For a zero-hash
/// raw string (`r"…"`) the empty check passes and any quote closes.
fn closing_hashes(bytes: &[u8], pos: usize, hashes: usize) -> bool {
    (0..hashes).all(|k| bytes.get(pos + k) == Some(&b'#'))
}

/// Char-literal vs lifetime heuristic at a `'` byte (full rationale
/// in the module doc): `\`-next is always a char (escape sequence);
/// a closing quote two bytes on means a single-character literal;
/// anything else is a lifetime.
fn is_char_literal_start(bytes: &[u8], pos: usize) -> bool {
    bytes.get(pos + 1) == Some(&b'\\') || bytes.get(pos + 2) == Some(&b'\'')
}

/// Push one comment line, stripping a trailing `\r` so CRLF input
/// reports the same text as LF.
fn push_segment<'a>(
    out: &mut Vec<(usize, &'a str)>,
    source: &'a str,
    start: usize,
    end: usize,
    line_no: usize,
) {
    let text = &source[start..end];
    out.push((line_no, text.strip_suffix('\r').unwrap_or(text)));
}

/// Run the gate's scanner over a tempdir tree whose only file is
/// `crates/fake/src/lib.rs` with the given content.
fn scan_fixture(source: &str) -> Vec<(String, usize, &'static str, String)> {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("crates").join("fake").join("src");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::write(src.join("lib.rs"), source).expect("write fixture");
    crate::scan_tree(tmp.path())
}

/// Positive dogfood: banned temporal narration in a `//` line
/// comment fires the gate.
#[test]
fn fires_on_line_comment_temporal() {
    let hits = scan_fixture("// previously the loader skipped checks.\npub fn f() {}\n");
    assert!(
        hits.iter()
            .any(|(_, _, label, _)| *label == "temporal 'previously' marker"),
        "expected temporal 'previously' marker hit; got {hits:?}"
    );
}

/// Positive dogfood: banned temporal narration in a `///` outer
/// doc comment fires the gate.
#[test]
fn fires_on_outer_doc_comment_temporal() {
    let hits = scan_fixture("/// Migrated from the old loader.\npub fn f() {}\n");
    assert!(
        hits.iter()
            .any(|(_, _, label, _)| *label == "temporal migration marker"),
        "expected temporal migration marker hit; got {hits:?}"
    );
}

/// Positive dogfood: banned temporal narration in a `//!` inner
/// doc comment fires the gate.
#[test]
fn fires_on_inner_doc_comment_temporal() {
    let hits = scan_fixture("//! Historically this slid through.\npub fn f() {}\n");
    assert!(
        hits.iter()
            .any(|(_, _, label, _)| *label == "temporal 'historically' marker"),
        "expected temporal 'historically' marker hit; got {hits:?}"
    );
}

/// Positive dogfood: banned temporal narration in a block comment
/// fires the gate.
#[test]
fn fires_on_block_comment_temporal() {
    let hits = scan_fixture("/* formerly unchecked. */\npub fn f() {}\n");
    assert!(
        hits.iter()
            .any(|(_, _, label, _)| *label == "temporal 'formerly' marker"),
        "expected temporal 'formerly' marker hit; got {hits:?}"
    );
}

/// Positive dogfood + line-number fidelity: a banned word on the
/// middle line of a multi-line block comment fires, reported at
/// that line.
#[test]
fn fires_on_multiline_block_comment_middle_line() {
    let hits = scan_fixture("/* header\n * formerly two parsers.\n * footer */\npub fn f() {}\n");
    let hit = hits
        .iter()
        .find(|(_, _, label, _)| *label == "temporal 'formerly' marker")
        .expect("expected formerly hit");
    assert_eq!(hit.1, 2, "hit must point at the middle comment line");
}

/// Positive dogfood: Rust block comments nest; a banned word inside
/// the nested layer still fires (depth-tracked).
#[test]
fn fires_on_nested_block_comment() {
    let hits =
        scan_fixture("/* outer\n   /* inner: previously nested */\n   tail */\npub fn f() {}\n");
    let hit = hits
        .iter()
        .find(|(_, _, label, _)| *label == "temporal 'previously' marker")
        .expect("expected previously hit inside nested block comment");
    assert_eq!(hit.1, 2, "hit must point at the nested comment line");
}

/// Positive dogfood: quote-ish runtime data (a char literal holding
/// a quote, a slash char, a lifetime, a URL string, a raw string)
/// must not blind the scanner — a banned comment right after such
/// data still fires, exactly once, at its own line.
#[test]
fn still_fires_on_comment_after_tricky_literals() {
    let hits = scan_fixture(
        "let c = '\"';\n\
         let s = '/';\n\
         let u = \"https://example.com\";\n\
         let r = r#\"raw\"#;\n\
         fn g<'a>(x: &'a str) -> &'a str { x }\n\
         // previously this fired late.\n",
    );
    let temporal: Vec<_> = hits
        .iter()
        .filter(|(_, _, label, _)| *label == "temporal 'previously' marker")
        .collect();
    assert_eq!(temporal.len(), 1, "exactly one temporal hit; got {hits:?}");
    assert_eq!(temporal[0].1, 6, "hit must point at the comment line");
}

/// Negative dogfood: banned words inside ordinary and raw string
/// literals are runtime data, not narration — no hit.
#[test]
fn does_not_fire_on_string_literal_data() {
    let hits = scan_fixture(
        "let s = \"previously recorded\";\n\
         let t = r#\"// previously in a raw string\"#;\n\
         pub fn f() { let _ = (s, t); }\n",
    );
    assert!(hits.is_empty(), "string data must not fire; got {hits:?}");
}

/// Negative dogfood: an `r##"…"##` raw string closes only at
/// quote-plus-two-hashes; the `"#` in its payload is data, and the
/// banned word after it must not become a comment.
#[test]
fn does_not_fire_on_double_hash_raw_string() {
    let hits = scan_fixture("let t = r##\"ok \"# // formerly data\"##;\npub fn f() {}\n");
    assert!(
        hits.is_empty(),
        "raw-string payload must not fire; got {hits:?}"
    );
}

/// Negative dogfood: `//` inside a string (a URL) does not open a
/// comment — the banned word in the URL tail stays data.
#[test]
fn does_not_fire_on_url_in_string() {
    let hits = scan_fixture("let u = \"https://example.com/previously\";\npub fn f() {}\n");
    assert!(hits.is_empty(), "URL data must not fire; got {hits:?}");
}

/// Negative dogfood: char literals (including one holding a quote)
/// and lifetimes are not comment openers; a file with only those
/// forms stays clean.
#[test]
fn does_not_fire_on_char_literal_and_lifetime() {
    let hits = scan_fixture(
        "let c = '\"';\n\
         let q = '\\'';\n\
         fn g<'a>(x: &'a str) -> &'a str { x }\n\
         fn h(x: &'static str) -> &'static str { x }\n",
    );
    assert!(
        hits.is_empty(),
        "char/lifetime forms must not fire; got {hits:?}"
    );
}
