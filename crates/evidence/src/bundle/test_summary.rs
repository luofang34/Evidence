//! Parsed `cargo test` stdout as a serializable summary.
//!
//! `parse_cargo_test_output` normalizes CRLF→LF on entry so a Windows-
//! captured stdout parses identically to Linux/macOS; every `test result:`
//! line in the stream is accumulated so workspace-wide test runs (which
//! emit one `test result:` line per crate) don't silently lose failures
//! in later crates.

use serde::{Deserialize, Serialize};

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
    let output = output.replace("\r\n", "\n");

    let mut total_passed = 0u64;
    let mut total_failed = 0u64;
    let mut total_ignored = 0u64;
    let mut total_filtered_out = 0u64;
    let mut found = false;

    for line in output.lines() {
        let line = line.trim();
        if !line.starts_with("test result:") {
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

    Some(TestSummary {
        total: total as u32,
        passed: total_passed as u32,
        failed: total_failed as u32,
        ignored: total_ignored as u32,
        filtered_out: total_filtered_out as u32,
    })
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
