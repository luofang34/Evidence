//! Grep-regression for `DiagnosticCode` codes across the library.
//!
//! This file holds the test cases. The source-walking machinery lives
//! in the sibling `diagnostic_codes_locked/walker.rs` — split out to
//! stay under the 500-line workspace file-size limit (see
//! `crates/evidence/tests/file_size_limit.rs`).
//!
//! Invariants enforced (Schema Rule 3 + reserved-suffix rule from
//! Rule 1 + PR #47 bijections):
//!
//! - Every walked code matches `^[A-Z][A-Z0-9]*(_[A-Z0-9]+)*$`.
//! - No two variants across the whole library return the same code.
//! - [`TERMINAL_CODES`] is internally unique.
//! - [`TERMINAL_CODES`] is disjoint from the walked registry.
//! - Every walked code ending in `_OK` / `_FAIL` / `_ERROR` is in
//!   [`TERMINAL_CODES`].
//! - Every [`TERMINAL_CODES`] entry ends in a reserved terminal
//!   suffix.
//! - (PR #47) `RULES ⇔ source walked set` bijection.
//! - (PR #47) `RULES.terminal=true ⇔ TERMINAL_CODES` bijection.
//! - (PR #47) Every `RULES` code is claimed by at least one
//!   `LLR.emits` list, and every `LLR.emits` entry is a real `RULES`
//!   code.
//!
//! [`TERMINAL_CODES`]: evidence::TERMINAL_CODES

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

// `#[path]` avoids the `mod diagnostic_codes_locked;` naming collision
// with the outer integration-test file of the same name.
#[path = "diagnostic_codes_locked/walker.rs"]
mod walker;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use walker::{
    code_is_valid, ends_in_terminal_suffix, extract_codes, rs_files, strip_line_comments,
    walked_codes, workspace_root,
};

#[test]
fn diagnostic_codes_locked() {
    let crate_root = workspace_root().join("crates").join("evidence").join("src");
    assert!(crate_root.is_dir(), "src/ not found at {:?}", crate_root);

    let mut files = Vec::new();
    rs_files(&crate_root, &mut files);
    files.sort();

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

    assert!(
        !seen.is_empty(),
        "parser found no DiagnosticCode impls under {:?} — parser regression?",
        crate_root
    );

    assert!(
        invalid.is_empty(),
        "codes violating UPPER_SNAKE_CASE pattern:\n{}",
        invalid
            .iter()
            .map(|(code, file, line)| format!("  {:?}:{} — '{}'", file, line, code))
            .collect::<Vec<_>>()
            .join("\n")
    );

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

    let terminal_codes: std::collections::BTreeSet<&str> =
        evidence::TERMINAL_CODES.iter().copied().collect();

    assert_eq!(
        terminal_codes.len(),
        evidence::TERMINAL_CODES.len(),
        "TERMINAL_CODES contains duplicates: {:?}",
        evidence::TERMINAL_CODES
    );

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

    let walked_as_terminal: Vec<&String> = seen
        .keys()
        .filter(|k| terminal_codes.contains(k.as_str()))
        .collect();
    assert!(
        walked_as_terminal.is_empty(),
        "codes in TERMINAL_CODES must not also be returned from DiagnosticCode impls; overlap: {:?}",
        walked_as_terminal
    );

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

// ============================================================================
// PR #47 bijection invariants — source ↔ RULES ↔ LLR.emits closed loop.
// ============================================================================

/// PR #47 invariant (1): every code returned from a library
/// `DiagnosticCode::code()` impl is declared in [`RULES`].
#[test]
fn rules_contains_every_code() {
    let walked = walked_codes();
    let rules: std::collections::BTreeSet<&str> = evidence::RULES.iter().map(|r| r.code).collect();
    let missing: Vec<&String> = walked
        .iter()
        .filter(|c| !rules.contains(c.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "codes returned from DiagnosticCode::code() but missing from RULES \
         (add a `RuleEntry` in crates/evidence/src/rules.rs): {:?}",
        missing
    );
}

/// PR #47 invariant (2): every non-terminal, non-hand-emitted
/// `RULES` entry is backed by a real `DiagnosticCode::code()` impl.
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

/// PR #47 invariant (3): `RULES.terminal=true` equals `TERMINAL_CODES`.
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

/// PR #47 invariant (4): every `RULES` code (minus
/// `RESERVED_UNCLAIMED_CODES`) is claimed by at least one
/// `LLR.emits` list, and every `LLR.emits` string names a real
/// `RULES` code.
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

    let rules: std::collections::BTreeSet<&str> = evidence::RULES.iter().map(|r| r.code).collect();
    let reserved: std::collections::BTreeSet<&str> =
        evidence::RESERVED_UNCLAIMED_CODES.iter().copied().collect();
    let claimed: std::collections::BTreeSet<String> = trace
        .llr
        .requirements
        .iter()
        .flat_map(|l| l.emits.iter().cloned())
        .collect();

    let dead: Vec<&String> = claimed
        .iter()
        .filter(|c| !rules.contains(c.as_str()))
        .collect();
    assert!(
        dead.is_empty(),
        "LLR.emits refers to code(s) not in RULES (typo, stale reference, or \
         the code was deleted and the LLR wasn't updated): {:?}",
        dead
    );

    let unclaimed: Vec<&str> = evidence::RULES
        .iter()
        .map(|r| r.code)
        .filter(|c| !reserved.contains(*c) && !claimed.contains(*c))
        .collect();
    assert!(
        unclaimed.is_empty(),
        "RULES code(s) not claimed by any LLR.emits in tool/trace/llr.toml \
         (add the code to an LLR that owns its emit path, or add a \
         RESERVED_UNCLAIMED_CODES entry with written justification): {:?}",
        unclaimed
    );
}

#[test]
fn code_regex_validator_catches_known_shapes() {
    assert!(code_is_valid("VERIFY_OK"));
    assert!(code_is_valid("HASH_OPEN_FAILED"));
    assert!(code_is_valid("BUNDLE_TOCTOU"));
    assert!(code_is_valid("POLICY_UNKNOWN_DAL"));
    assert!(code_is_valid("VERIFY"));
    assert!(code_is_valid("SCHEMA_V1_COMPILE"));

    assert!(!code_is_valid(""));
    assert!(!code_is_valid("verify_ok"));
    assert!(!code_is_valid("Verify_Ok"));
    assert!(!code_is_valid("VERIFY__OK"));
    assert!(!code_is_valid("VERIFY_"));
    assert!(!code_is_valid("_VERIFY_OK"));
    assert!(!code_is_valid("VERIFY-OK"));
    assert!(!code_is_valid("0VERIFY"));
}
