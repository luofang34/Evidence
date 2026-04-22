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
//! fully-qualified key `binary_name::<name>`. That map is what //! `cargo evidence check` uses to answer "did the test for this
//! requirement pass in this run?"
//!
//! **Interleaving gotcha.** Cargo emits `Running …` to stderr and the
//! `test <name> … ok` lines to stdout. When callers capture the two
//! streams and concatenate (stdout-then-stderr), the binary headers
//! appear at the *end* of the merged buffer — so a single-pass tracker
//! never sees the header before the tests it describes. The parser is
//! therefore two-pass: first it harvests every `Running` binary name in
//! order of appearance, then it walks the stream and bumps through that
//! list at every `running N tests` (stdout-side start-of-binary
//! marker). That makes the parser robust to either order — merged via
//! a shell `2>&1`, or concatenated from separate captures.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod detailed;

pub use detailed::parse_cargo_test_output_detailed;

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

    // Pass 1: harvest binary names in order of appearance. Cargo emits
    // these on stderr; callers that concatenate stdout-then-stderr push
    // them to the end of the merged buffer, so a single-pass tracker
    // would see every test line before any binary name. By collecting
    // first, we can attribute tests correctly regardless of stream
    // ordering.
    let binaries: Vec<String> = output
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("Running "))
        .filter_map(extract_binary_name)
        .collect();

    let mut total_passed = 0u64;
    let mut total_failed = 0u64;
    let mut total_ignored = 0u64;
    let mut total_filtered_out = 0u64;
    let mut found = false;

    // Pass 2: advance `binary_idx` on every `running N tests` marker
    // (stdout-side start-of-binary). The idx starts at -1 so the first
    // marker bumps us to 0, matching `binaries[0]`.
    let mut binary_idx: isize = -1;
    let mut outcomes: BTreeMap<String, TestOutcome> = BTreeMap::new();

    for line in output.lines() {
        let line = line.trim();

        // Start-of-binary marker on stdout. `running 0 tests` also
        // counts — an empty test binary still occupies a slot.
        if line.starts_with("running ")
            && line
                .split_whitespace()
                .nth(2)
                .is_some_and(|w| w == "tests" || w == "test")
        {
            binary_idx += 1;
            continue;
        }

        // Skip the stderr-side binary header lines harvested in pass 1.
        if line.starts_with("Running ") {
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
                    let bin = if binary_idx >= 0 {
                        binaries
                            .get(binary_idx as usize)
                            .map(String::as_str)
                            .unwrap_or("__unknown_binary__")
                    } else {
                        "__unknown_binary__"
                    };
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
