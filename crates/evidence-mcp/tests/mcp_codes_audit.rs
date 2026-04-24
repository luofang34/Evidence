//! Meta-bijection for `evidence_core::HAND_EMITTED_MCP_CODES`.
//!
//! Two-way check:
//!
//! - Registry → source (`every_mcp_code_emitted_in_source`):
//!   every entry in the registry must appear as a string
//!   literal somewhere under `crates/evidence-mcp/src`. Catches
//!   the pseudo-code anti-pattern — a code declared in RULES
//!   but never actually emitted.
//! - Source → registry (`every_mcp_literal_in_source_is_registered`):
//!   every `MCP_*` string literal in source must be in the
//!   registry. Catches the silent-contract-break case — a new
//!   code emitted from the server but never added to the public
//!   vocabulary.
//!
//! Mirrors `doctor_checks_locked::every_doctor_code_emitted_in_source`
//! on the CLI side, extended with the reverse direction.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::{Path, PathBuf};

/// TEST-063 selector: every `HAND_EMITTED_MCP_CODES` entry has
/// a source site under `crates/evidence-mcp/src`.
#[test]
fn every_mcp_code_emitted_in_source() {
    let src = mcp_src_root();
    let haystack = read_all_rs_files(&src);

    let mut missing: Vec<&'static str> = Vec::new();
    for code in evidence_core::HAND_EMITTED_MCP_CODES {
        let quoted = format!("\"{code}\"");
        if !haystack.contains(&quoted) {
            missing.push(code);
        }
    }
    assert!(
        missing.is_empty(),
        "the following MCP_* code(s) are in \
         evidence_core::HAND_EMITTED_MCP_CODES but never emitted as a \
         string literal under {src:?} (pseudo-code anti-pattern): \
         {missing:?}. Either emit the code or remove it from the registry.",
    );
}

/// TEST-071 selector: every `MCP_*` string literal under
/// `crates/evidence-mcp/src` must be in
/// `evidence_core::HAND_EMITTED_MCP_CODES`. Reverse direction
/// of `every_mcp_code_emitted_in_source` — catches the case
/// where a handler emits a new code that the public registry
/// doesn't know about.
#[test]
fn every_mcp_literal_in_source_is_registered() {
    let src = mcp_src_root();
    let haystack = read_all_rs_files(&src);

    let registry: std::collections::BTreeSet<&'static str> = evidence_core::HAND_EMITTED_MCP_CODES
        .iter()
        .copied()
        .collect();

    let literals = extract_mcp_literals(&haystack);
    let mut orphans: Vec<String> = literals
        .into_iter()
        .filter(|lit| !registry.contains(lit.as_str()))
        .collect();
    orphans.sort();
    orphans.dedup();

    assert!(
        orphans.is_empty(),
        "the following `MCP_*` string literal(s) are emitted \
         from {src:?} but not registered in \
         evidence_core::HAND_EMITTED_MCP_CODES (silent contract \
         break — agents pattern-matching on `.code` against the \
         rules manifest would miss these): {orphans:?}. Either \
         register the code or rename the literal.",
    );
}

/// Extract every `MCP_*` string literal from the given
/// concatenated source text. A literal is an `MCP_` followed by
/// one or more uppercase letters, digits, or underscores,
/// immediately preceded by `"` and immediately followed by `"`.
/// Anchoring on the double quotes avoids matching identifiers
/// or partial substrings inside longer codes.
///
/// Scope caveat: a quoted `MCP_*` inside a doc comment (e.g.,
/// `/// see "MCP_FUTURE_CODE"`) would be picked up as a
/// literal. No such mention exists today; if one ever does, the
/// fix is either register the code or rewrite the comment.
/// Tightening the scanner to skip `//` / `///` contexts would
/// double its length for negligible gain.
fn extract_mcp_literals(haystack: &str) -> Vec<String> {
    let bytes = haystack.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i + 5 < bytes.len() {
        if bytes[i] == b'"' && &bytes[i + 1..i + 5] == b"MCP_" {
            let start = i + 1;
            let mut end = start + 4;
            while end < bytes.len() {
                let c = bytes[end];
                if c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_' {
                    end += 1;
                } else {
                    break;
                }
            }
            if end < bytes.len() && bytes[end] == b'"' {
                // The inner loop only advances over ASCII bytes
                // (upper, digit, or `_`), so the subslice is
                // ASCII by construction — expect rather than
                // unwrap_or("") signals the intent.
                let lit = std::str::from_utf8(&bytes[start..end])
                    .expect("ascii-only subslice from the inner character class");
                if !lit.is_empty() {
                    out.push(lit.to_string());
                }
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn mcp_src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn read_all_rs_files(root: &Path) -> String {
    let mut out = String::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != "target")
        .filter_map(Result::ok)
    {
        if entry.file_type().is_file()
            && entry.path().extension().and_then(|e| e.to_str()) == Some("rs")
        {
            let body = fs::read_to_string(entry.path())
                .unwrap_or_else(|e| panic!("read {:?}: {e}", entry.path()));
            out.push_str(&body);
            out.push('\n');
        }
    }
    out
}
