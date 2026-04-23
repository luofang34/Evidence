//! Meta-bijection for `evidence_core::HAND_EMITTED_MCP_CODES`.
//!
//! Every entry in the registry must appear as a string literal
//! somewhere under `crates/evidence-mcp/src` — either as the
//! body of a `pub(crate) const MCP_*: &str = "MCP_*"` declaration
//! (the parse-terminal case), inside a `RunError::code` match
//! arm (the subprocess-wrapper case), or as the argument to
//! `workspace_fallback_diagnostic` (the workspace-fallback case).
//!
//! Mirrors `doctor_checks_locked::every_doctor_code_emitted_in_source`
//! on the CLI side. A new code landing in the registry without a
//! real emit site fires here, not at runtime.

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
