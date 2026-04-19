//! Parsed `cargo test` stdout as a serializable summary plus optional
//! per-test outcome map.
//!
//! `parse_cargo_test_output` normalizes CRLF→LF on entry so a Windows-
//! captured stdout parses identically to Linux/macOS; every `test result:`
//! line in the stream is accumulated so workspace-wide test runs (which
//! emit one `test result:` line per crate) don't silently lose failures
//! in later crates.
//!
//! `parse_cargo_test_output_with_outcomes` additionally tracks the
//! `Running target/debug/deps/<binary>-<hash>` header so per-test result
//! lines (`test <name> ... ok|FAILED|ignored`) can be mapped to a
//! fully-qualified key `binary_name::<name>`. That map is what PR #46's
//! `cargo evidence check` uses to answer "did the test for this
//! requirement pass in this run?"

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Parsed summary of `cargo test` output.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TestSummary {
    /// Sum of `passed + failed + ignored + filtered_out` across every
    /// `test result:` line emitted by the run.
    pub total: u32,
    /// Count of tests that passed across the whole run.
    pub passed: u32,
    /// Count of tests that failed across the whole run.
    pub failed: u32,
    /// Count of tests skipped via `#[ignore]`.
    pub ignored: u32,
    /// Count of tests filtered out by a name/module filter.
    pub filtered_out: u32,
}

/// Outcome of a single named test from `cargo test` stdout.
///
/// `Ignored` covers `#[ignore]`-annotated tests; `Filtered` is not
/// represented because filtered tests don't emit `test <name> ...` lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestOutcome {
    /// Test ran and passed.
    Passed,
    /// Test ran and failed.
    Failed,
    /// Test was marked `#[ignore]` and did not run.
    Ignored,
}

/// Parse cargo test result lines into an accumulated `TestSummary`.
///
/// In a workspace, `cargo test` produces multiple `test result:` lines
/// (one per crate). This function accumulates ALL of them to avoid
/// silently discarding failures in later crates.
///
/// Normalizes `\r\n` → `\n` on entry so output captured from a Windows
/// cargo run (which terminates lines with CRLF) is parsed the same way
/// as Linux/macOS output — a stray trailing `\r` would otherwise break
/// the `trim_end_matches(';')` / split-by-space tokenization on the
/// last segment of every line.
///
/// Returns `None` if no matching line is found.
pub fn parse_cargo_test_output(output: &str) -> Option<TestSummary> {
    parse_cargo_test_output_with_outcomes(output).map(|(summary, _)| summary)
}

/// Parse cargo test stdout into both the aggregate `TestSummary` AND a
/// per-test outcome map keyed by `binary_name::<test_path>`.
///
/// The outcome map is what `cargo evidence check` uses to resolve each
/// requirement's `test_selector` to a concrete pass/fail. Keys follow
/// cargo's own selector convention — agents and humans can copy a key
/// into `cargo test <key>` and run that test directly.
///
/// Binary-name tracking: whenever the parser sees a header line like
/// `Running unittests /path/src/lib.rs (target/debug/deps/<binary>-<hash>)`
/// or `Running tests/foo.rs (target/debug/deps/<foo>-<hash>)`, it
/// records `<binary>` (with the trailing hash stripped). Subsequent
/// `test <name> ... <outcome>` lines are attributed to that binary.
/// If the parser never saw a Running header before a test line
/// (shouldn't happen in practice for a real `cargo test` run), the
/// test is keyed under `__unknown_binary__::<name>` so it remains
/// addressable for debugging.
///
/// Returns `None` if no `test result:` line is found in the input
/// (empty or non-test output).
pub fn parse_cargo_test_output_with_outcomes(
    output: &str,
) -> Option<(TestSummary, BTreeMap<String, TestOutcome>)> {
    let output = output.replace("\r\n", "\n");

    let mut total_passed = 0u64;
    let mut total_failed = 0u64;
    let mut total_ignored = 0u64;
    let mut total_filtered_out = 0u64;
    let mut found = false;

    let mut current_binary: Option<String> = None;
    let mut outcomes: BTreeMap<String, TestOutcome> = BTreeMap::new();

    for line in output.lines() {
        let line = line.trim();

        // Binary header: cargo emits lines like
        //   Running unittests src/lib.rs (target/debug/deps/evidence-3f4a2c)
        //   Running tests/verify_jsonl.rs (target/debug/deps/verify_jsonl-8e91)
        // Extract the segment inside the last parens, take the basename
        // of the path, strip `-<hash>`.
        if line.starts_with("Running ") {
            if let Some(bin) = extract_binary_name(line) {
                current_binary = Some(bin);
            }
            continue;
        }

        // Aggregate `test result:` line — check before the per-test
        // "test <name> ..." shape because both prefixes start with
        // `"test "`.
        let is_aggregate = line.starts_with("test result:");

        // Per-test result line: "test <name> ... ok" / "... FAILED" /
        // "... ignored". `<name>` may be a bare fn (integration test)
        // or a module-qualified path (unit test inside the binary).
        if !is_aggregate && let Some(rest) = line.strip_prefix("test ") {
            // Shape: "<name> ... <outcome>" — tokenize by " ... ".
            if let Some((name, outcome_tail)) = rest.split_once(" ... ") {
                let outcome = match outcome_tail.split_whitespace().next() {
                    Some("ok") => Some(TestOutcome::Passed),
                    Some("FAILED") => Some(TestOutcome::Failed),
                    Some("ignored") => Some(TestOutcome::Ignored),
                    _ => None, // e.g. "bench" from `cargo bench`, ignore
                };
                if let Some(o) = outcome {
                    let bin = current_binary.as_deref().unwrap_or("__unknown_binary__");
                    let key = format!("{}::{}", bin, name);
                    // If a key appears twice (shouldn't, but guard),
                    // Failed wins over Passed wins over Ignored.
                    match (outcomes.get(&key).copied(), o) {
                        (Some(TestOutcome::Failed), _) => {}
                        (_, TestOutcome::Failed) => {
                            outcomes.insert(key, TestOutcome::Failed);
                        }
                        (Some(TestOutcome::Passed), _) => {}
                        _ => {
                            outcomes.insert(key, o);
                        }
                    }
                }
            }
            continue;
        }

        if !is_aggregate {
            continue;
        }
        let after_prefix = if let Some(rest) = line.strip_prefix("test result: ok. ") {
            rest
        } else if let Some(rest) = line.strip_prefix("test result: FAILED. ") {
            rest
        } else {
            continue;
        };

        found = true;

        for segment in after_prefix.split(';') {
            let segment = segment.trim().trim_end_matches(';');
            let parts: Vec<&str> = segment.splitn(2, ' ').collect();
            if parts.len() != 2 {
                continue;
            }
            let n: u64 = match parts[0].parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            match parts[1].trim() {
                "passed" => total_passed += n,
                "failed" => total_failed += n,
                "ignored" => total_ignored += n,
                "filtered out" => total_filtered_out += n,
                _ => {}
            }
        }
    }

    if !found {
        return None;
    }

    let total = total_passed
        .saturating_add(total_failed)
        .saturating_add(total_ignored)
        .saturating_add(total_filtered_out);

    Some((
        TestSummary {
            total: total as u32,
            passed: total_passed as u32,
            failed: total_failed as u32,
            ignored: total_ignored as u32,
            filtered_out: total_filtered_out as u32,
        },
        outcomes,
    ))
}

/// Extract the binary name from a `Running … (target/debug/deps/<name>-<hash>)`
/// header line. Returns `None` if the line doesn't match the expected
/// cargo-emitted shape.
fn extract_binary_name(line: &str) -> Option<String> {
    // Find the parenthesized path and keep the content between the last
    // `/` and the final `-<hash>` suffix.
    let open = line.rfind('(')?;
    let close = line.rfind(')')?;
    if close <= open + 1 {
        return None;
    }
    let path = &line[open + 1..close];
    let basename = path.rsplit('/').next()?;
    // Strip the trailing `-<hex>` that cargo appends. Hex segment is
    // typically 16 chars on Linux/macOS but length varies by host;
    // match by taking everything before the LAST `-`.
    let cut = basename.rfind('-')?;
    let bin = &basename[..cut];
    if bin.is_empty() {
        return None;
    }
    Some(bin.to_string())
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
    fn test_parse_cargo_test_output_handles_crlf_line_endings() {
        // Windows `Command::output()` captures cargo test with CRLF line
        // endings; the parser must normalize before tokenizing or the
        // trailing `\r` on each `; N filtered out` segment breaks the
        // "filtered out" match.
        let crlf =
            "test result: ok. 3 passed; 1 failed; 2 ignored; 0 filtered out; finished in 0.01s\r\n";
        let summary = parse_cargo_test_output(crlf).expect("should parse CRLF output");
        assert_eq!(summary.passed, 3);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.ignored, 2);
        assert_eq!(summary.filtered_out, 0);
        assert_eq!(summary.total, 6);
    }

    #[test]
    fn test_parse_cargo_test_output_ok() {
        let output = "\
running 20 tests
test foo ... ok
test result: ok. 20 passed; 0 failed; 1 ignored; 0 measured; 3 filtered out; finished in 0.5s
";
        let summary = parse_cargo_test_output(output).expect("should parse");
        assert_eq!(summary.passed, 20);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.ignored, 1);
        assert_eq!(summary.filtered_out, 3);
        assert_eq!(summary.total, 24);
    }

    #[test]
    fn test_parse_cargo_test_output_failed() {
        let output =
            "test result: FAILED. 18 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out";
        let summary = parse_cargo_test_output(output).expect("should parse");
        assert_eq!(summary.passed, 18);
        assert_eq!(summary.failed, 2);
        assert_eq!(summary.total, 20);
    }

    #[test]
    fn test_parse_cargo_test_output_no_match() {
        let output = "compiling something\nfinished dev";
        assert!(parse_cargo_test_output(output).is_none());
    }
}
