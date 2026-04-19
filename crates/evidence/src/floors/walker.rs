//! Source-walking helpers for the floors gate.
//!
//! Split out of the parent `floors.rs` to keep the facade under the
//! 500-line workspace file-size limit. Everything here is a pure
//! function of the file tree; the caller in the parent module
//! aggregates measurements from these helpers.

use std::fs;
use std::path::{Path, PathBuf};

/// Walk `root` recursively; push every `.rs` path into `out`. Skips
/// `target/` trees so a stale `cargo doc` output can't taint the
/// measurement.
pub fn walk_rs_files(root: &Path, out: &mut Vec<PathBuf>) {
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
            walk_rs_files(&path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Strip top-level `#[cfg(test)]\nmod … { … }` blocks from a Rust
/// source. Conservative: matches blocks at indent 0 by brace-depth
/// tracking; nested `#[cfg(test)]` sub-modules aren't detected and
/// their content survives the strip. Miss-rate is acceptable for a
/// floor that starts at 0 and catches any *new* library panic, even
/// one that would sit inside a nested test module.
pub fn strip_cfg_test_modules(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // Look for `#[cfg(test)]` at the start of a line.
        let at_line_start = i == 0 || bytes[i - 1] == b'\n';
        if at_line_start && bytes[i..].starts_with(b"#[cfg(test)]") {
            // Skip to the next `{` (module body open).
            let mut j = i;
            while j < bytes.len() && bytes[j] != b'{' {
                j += 1;
            }
            if j >= bytes.len() {
                break;
            }
            // Find the matching `}`.
            let mut depth: i32 = 0;
            let mut k = j;
            while k < bytes.len() {
                match bytes[k] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            k += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            i = k;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Line-level check: does the given `line` contain `needle` outside
/// any string literal? Implements two guards:
///
/// 1. **Quoted substring.** Counts unescaped `"` chars before the
///    needle; odd → mid-string → skip.
/// 2. **Raw string literal.** Detects `r"` or `r#*"` tokens
///    preceding the needle; if the needle falls between a raw-string
///    open and its matching close, skip. The walker's own source
///    has `"panic!("` inside a string array — this is the guard
///    that keeps the floor from self-tripping.
///
/// Not a full Rust lexer — escaped quotes inside non-raw strings and
/// char literals (`'"'`) can still produce false results. Those
/// cases are vanishingly rare in the library code this walker
/// targets; the regression tests in the parent module pin the
/// expected behavior for the cases that actually occur.
pub fn needle_is_outside_string_literal(line: &str, needle: &str) -> bool {
    let Some(pos) = line.find(needle) else {
        return false;
    };
    let before = &line[..pos];
    // Guard 2 first: if a raw-string opener (`r"` or `r#…"`) appears
    // before the needle and the matching closer comes AFTER, the
    // needle is inside a raw string.
    if raw_string_covers(before, &line[pos..]) {
        return false;
    }
    // Guard 1: odd number of `"` before the needle means mid-string.
    let quotes_before = before.chars().filter(|&c| c == '"').count();
    if quotes_before % 2 == 1 {
        return false;
    }
    true
}

/// Does a raw-string literal opened somewhere in `before` still cover
/// the position at the start of `after`?
///
/// Pattern scanned: `r"…"`, `r#"…"#`, `r##"…"##`, … up to a sensible
/// hash count. We walk `before` looking for `r` followed by any
/// number of `#`s followed by `"`, and if we find one, check whether
/// its matching close (`"` + same hash count) appears in `before`
/// after the opener. If the opener isn't closed inside `before`, the
/// needle at `after[0]` is inside the raw string.
fn raw_string_covers(before: &str, _after: &str) -> bool {
    // Simplified: find the last `r#*"` token in `before`. If no
    // matching close (`"#*`) appears between its position and the
    // end of `before`, the needle is inside a live raw string.
    let bytes = before.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'r' {
            // r must not be preceded by an identifier char (otherwise
            // it's part of another identifier like `for`).
            let preceded_by_ident =
                i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
            if !preceded_by_ident {
                // Count following `#` chars.
                let mut j = i + 1;
                let mut hashes = 0;
                while j < bytes.len() && bytes[j] == b'#' {
                    hashes += 1;
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'"' {
                    // Opener at i..=j. Look for closer: `"` + same
                    // number of `#`.
                    let mut k = j + 1;
                    let mut closed = false;
                    while k < bytes.len() {
                        if bytes[k] == b'"' {
                            // Check following hashes.
                            let mut h = 0;
                            let mut m = k + 1;
                            while m < bytes.len() && bytes[m] == b'#' && h < hashes {
                                h += 1;
                                m += 1;
                            }
                            if h == hashes {
                                closed = true;
                                k = m;
                                break;
                            }
                            k += 1;
                        } else {
                            k += 1;
                        }
                    }
                    if !closed {
                        // Opener has no closer in `before` — needle
                        // is inside a live raw string.
                        return true;
                    }
                    i = k;
                    continue;
                }
            }
        }
        i += 1;
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
    fn strip_cfg_test_modules_removes_nested_braces() {
        let input = "fn live() { panic!(); }\n#[cfg(test)]\nmod tests { panic!(); fn t() { todo!(); } }\nfn also_live() {}\n";
        let stripped = strip_cfg_test_modules(input);
        assert!(stripped.contains("fn live()"));
        assert!(stripped.contains("fn also_live()"));
        assert!(!stripped.contains("mod tests"));
        assert!(!stripped.contains("todo!()"));
        // The live `panic!()` on line 1 survives.
        assert!(stripped.contains("panic!()"));
    }

    #[test]
    fn needle_outside_string_literal_rejects_plain_quoted_occurrence() {
        // `"panic!("` in an array — quote-parity guard.
        let line = r#"    let xs = ["panic!(", "todo!("];"#;
        assert!(!needle_is_outside_string_literal(line, "panic!("));
        assert!(!needle_is_outside_string_literal(line, "todo!("));
    }

    #[test]
    fn needle_outside_string_literal_accepts_bare_macro_call() {
        let line = r#"    if broken { panic!("real"); }"#;
        assert!(needle_is_outside_string_literal(line, "panic!("));
    }

    #[test]
    fn needle_outside_string_literal_rejects_raw_string_occurrence() {
        // `r#"panic!(..."#` — raw-string guard. Without the guard
        // `panic!(` would be counted because no `"` precedes it
        // inside the raw literal.
        let line = r##"    let s = r#"panic!("inside raw")"#;"##;
        assert!(!needle_is_outside_string_literal(line, "panic!("));
    }

    #[test]
    fn needle_outside_string_literal_accepts_for_loop_over_r_prefix() {
        // `for` starts with `for` not `r`; our `r` check must not
        // confuse `for` with a raw-string opener.
        let line = "for item in things { panic!(\"real\"); }";
        assert!(needle_is_outside_string_literal(line, "panic!("));
    }
}
