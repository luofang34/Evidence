//! Enriched per-test outcome parsing (failure-block capture).
//!
//! Sibling of [`super::test_summary`]. Split here to keep the
//! parent file under the 500-line limit; the detailed parser
//! grew ~250 LOC for failure-block scanning + unit tests.

use std::collections::BTreeMap;

use super::super::outcome_record::{TestOutcomeRecord, TestsError};
use super::{TestOutcome, TestSummary, parse_cargo_test_output_with_outcomes};

/// Enriched parse: per-test outcomes + captured failure messages
/// from libtest's `---- <test> stdout ----` blocks. Returns:
///
/// - `TestSummary` (aggregate counters — same as the basic
///   parser).
/// - `Vec<TestOutcomeRecord>` one-per-test, sorted
///   alphabetically by `(module_path, name)` for deterministic
///   on-disk serialization. Failed tests carry
///   `failure_message: Some(...)` when the scanner matched a
///   failure block to the test; `None` if no block was present
///   or the parser couldn't match one back.
/// - `Vec<TestsError>` graceful-degrade signals surfaced to the
///   CLI for JSONL emission (today: a failure block referenced
///   a test not present in the outcomes map).
///
/// Returns `None` if no `test result:` line is found — same
/// semantics as the basic parser.
///
/// **Failure-block shape.** After the aggregate `test result:`
/// line libtest emits, for each failing test:
///
/// ```text
/// ---- <qualified_test_name> stdout ----
/// <panic message, possibly multi-line>
/// thread '<name>' panicked at …
/// note: run with `RUST_BACKTRACE=1` …
///
/// failures:
///     <qualified_test_name>
/// ```
///
/// We capture everything from the `---- … stdout ----` header
/// up to the next `---- …` header or the `failures:` summary
/// line, trimming trailing blank lines. The captured block is
/// attached to the matching test by `<qualified_test_name>`.
pub fn parse_cargo_test_output_detailed(
    output: &str,
) -> Option<(TestSummary, Vec<TestOutcomeRecord>, Vec<TestsError>)> {
    let (summary, outcomes) = parse_cargo_test_output_with_outcomes(output)?;

    let failure_messages = scan_failure_blocks(output);

    // Convert the outcomes map to records. Failed entries get
    // their message if the block scan found a match; others stay
    // `None`.
    let mut errors: Vec<TestsError> = Vec::new();
    let mut records: Vec<TestOutcomeRecord> = Vec::with_capacity(outcomes.len());

    for (qualified_key, outcome) in &outcomes {
        let (module_path, name) = split_qualified_key(qualified_key);
        let (passed, ignored, failure_message) = match outcome {
            TestOutcome::Passed => (true, false, None),
            TestOutcome::Failed => {
                // Failure blocks key by the libtest-qualified name
                // WITHOUT the binary prefix — e.g. for outcomes
                // key `fixture::mod1::fails`, the failure block
                // header is `---- mod1::fails stdout ----`.
                // Strip the binary prefix (everything before the
                // first `::`) to reconstruct the scanner's key.
                let libtest_name = qualified_key
                    .split_once("::")
                    .map(|(_binary, rest)| rest.to_string())
                    .unwrap_or_else(|| qualified_key.clone());
                let msg = failure_messages.get(&libtest_name).cloned();
                (false, false, msg)
            }
            TestOutcome::Ignored => (false, true, None),
        };
        records.push(TestOutcomeRecord {
            name: name.to_string(),
            module_path: module_path.to_string(),
            passed,
            ignored,
            failure_message,
            duration_ms: None,
        });
    }

    // Graceful-degrade signal: a failure block referenced a test
    // name we couldn't find in the outcomes map. Shouldn't
    // happen on well-formed libtest output but pins the
    // behaviour loud if libtest's format drifts. A successful
    // match requires the failure_messages key to equal the
    // libtest-suffix of some outcome key.
    let matched_libtest_names: std::collections::BTreeSet<String> = outcomes
        .keys()
        .filter_map(|k| k.split_once("::").map(|(_bin, rest)| rest.to_string()))
        .collect();
    for key in failure_messages.keys() {
        if !matched_libtest_names.contains(key) {
            errors.push(TestsError::OutcomeParseFailed {
                test_name: key.clone(),
            });
        }
    }

    records.sort_by(|a, b| {
        a.module_path
            .cmp(&b.module_path)
            .then_with(|| a.name.cmp(&b.name))
    });

    Some((summary, records, errors))
}

/// Split a `binary_name::module::path::test_name` key into
/// `(module_path, test_name)` where module_path may be empty
/// (bare integration-test fn with no module prefix).
///
/// The parser's outcomes map keys are
/// `"{binary}::{libtest_name}"` where `libtest_name` may itself
/// be `fn` (integration test) or `mod::path::fn` (unit test
/// inside a library). For the wire record, module_path is
/// `binary::mod::path` (everything before the final `::`) and
/// name is the final segment.
fn split_qualified_key(key: &str) -> (&str, &str) {
    match key.rsplit_once("::") {
        Some((module_path, name)) => (module_path, name),
        None => ("", key),
    }
}

/// Walk `output` for `---- <test_name> stdout ----` failure
/// blocks and capture the contained message text. Returns a map
/// keyed by the bare test name (no binary prefix — libtest uses
/// the unqualified form inside the block header).
fn scan_failure_blocks(output: &str) -> BTreeMap<String, String> {
    let output = output.replace("\r\n", "\n");
    let mut messages: BTreeMap<String, String> = BTreeMap::new();
    let mut current_test: Option<String> = None;
    let mut current_buf: Vec<&str> = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();

        // New failure block starts. `---- <name> stdout ----`
        // closes any previous block.
        if let Some(name) = parse_failure_header(trimmed) {
            if let Some(test) = current_test.take() {
                flush_block(&mut messages, test, &current_buf);
                current_buf.clear();
            }
            current_test = Some(name);
            continue;
        }

        // Block terminators: `failures:` summary header, or the
        // `test result:` aggregate line.
        if trimmed == "failures:" || trimmed.starts_with("test result:") {
            if let Some(test) = current_test.take() {
                flush_block(&mut messages, test, &current_buf);
                current_buf.clear();
            }
            continue;
        }

        if current_test.is_some() {
            current_buf.push(line);
        }
    }

    // Final block flush (in case output ends mid-block).
    if let Some(test) = current_test.take() {
        flush_block(&mut messages, test, &current_buf);
    }

    messages
}

/// Parse `---- <test_name> stdout ----` into `<test_name>`.
/// Libtest prefixes the header with four dashes, the qualified
/// test name, a space, the literal "stdout", and a closing four
/// dashes. Returns None for any line that doesn't match exactly.
fn parse_failure_header(line: &str) -> Option<String> {
    let inner = line.strip_prefix("---- ")?;
    let inner = inner.strip_suffix(" ----")?;
    let name = inner.strip_suffix(" stdout")?;
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

/// Trim leading/trailing blank lines in `buf` and insert the
/// joined text into `messages` under `test`. A fully-empty
/// block (block header followed by terminator with no content)
/// still inserts an empty string so the caller can distinguish
/// "failure block present but empty" from "no block at all."
fn flush_block(messages: &mut BTreeMap<String, String>, test: String, buf: &[&str]) {
    let start = buf
        .iter()
        .position(|l| !l.trim().is_empty())
        .unwrap_or(buf.len());
    let end = buf
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map(|i| i + 1)
        .unwrap_or(0);
    // `end.max(start)` guards the all-blank case where start =
    // len and end = 0 — without it `buf[len..0]` panics.
    let joined = buf[start..end.max(start)].join("\n");
    messages.insert(test, joined);
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

    /// Canonical libtest failure-block shape: aggregate summary
    /// precedes one or more `---- <name> stdout ----` blocks,
    /// each capturing the panic text, terminated by the
    /// `failures:` summary.
    #[test]
    fn parse_failure_block_captures_message() {
        let output = "\
Running target/debug/deps/fixture-aaaaaaaaaaaaaaaa

running 2 tests
test mod1::passes ... ok
test mod1::fails ... FAILED

failures:

---- mod1::fails stdout ----
thread 'mod1::fails' panicked at src/lib.rs:7:5:
assertion `left != right` failed
  left: 42
 right: 42
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

failures:
    mod1::fails

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
";
        let (summary, records, errors) = parse_cargo_test_output_detailed(output).expect("parses");
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.failed, 1);
        assert!(
            errors.is_empty(),
            "well-formed output → no TestsError: {errors:?}"
        );

        let failing = records
            .iter()
            .find(|r| r.name == "fails")
            .expect("fails record present");
        assert!(!failing.passed);
        let msg = failing
            .failure_message
            .as_deref()
            .expect("failure_message populated");
        assert!(msg.contains("assertion"), "message: {msg}");
        assert!(msg.contains("left: 42"), "message: {msg}");

        let passing = records
            .iter()
            .find(|r| r.name == "passes")
            .expect("passes record present");
        assert!(passing.passed);
        assert!(passing.failure_message.is_none());
    }

    /// No failures → parser still produces records, all with
    /// `passed = true` and `failure_message = None`. No
    /// TestsError.
    #[test]
    fn parse_tolerates_missing_failure_block() {
        let output = "\
Running target/debug/deps/fixture-aaaaaaaaaaaaaaaa

running 2 tests
test mod1::a ... ok
test mod1::b ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
";
        let (_summary, records, errors) = parse_cargo_test_output_detailed(output).expect("parses");
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|r| r.passed));
        assert!(records.iter().all(|r| r.failure_message.is_none()));
        assert!(errors.is_empty());
    }

    /// Multiple failing tests each get their own message —
    /// failure-block boundaries don't bleed.
    #[test]
    fn parse_captures_multiple_failure_blocks_independently() {
        let output = "\
Running target/debug/deps/fixture-aaaaaaaaaaaaaaaa

running 2 tests
test mod1::a_fails ... FAILED
test mod1::b_fails ... FAILED

failures:

---- mod1::a_fails stdout ----
thread 'mod1::a_fails' panicked at src/lib.rs:1:1:
alpha failure text

---- mod1::b_fails stdout ----
thread 'mod1::b_fails' panicked at src/lib.rs:2:2:
beta failure text

failures:
    mod1::a_fails
    mod1::b_fails

test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
";
        let (_summary, records, _errors) =
            parse_cargo_test_output_detailed(output).expect("parses");
        let a = records
            .iter()
            .find(|r| r.name == "a_fails")
            .expect("a present");
        let b = records
            .iter()
            .find(|r| r.name == "b_fails")
            .expect("b present");
        assert!(
            a.failure_message.as_deref().unwrap_or("").contains("alpha"),
            "a.msg: {:?}",
            a.failure_message
        );
        assert!(
            b.failure_message.as_deref().unwrap_or("").contains("beta"),
            "b.msg: {:?}",
            b.failure_message
        );
        assert!(
            !a.failure_message.as_deref().unwrap_or("").contains("beta"),
            "a's block bled into b"
        );
        assert!(
            !b.failure_message.as_deref().unwrap_or("").contains("alpha"),
            "b's block bled into a"
        );
    }

    /// Empty stdout → parser returns `None`, same as the basic
    /// function. Edge case the CLI layer handles with its
    /// `CLI_INVALID_ARGUMENT` mislabel check.
    #[test]
    fn parse_detailed_returns_none_on_empty() {
        assert!(parse_cargo_test_output_detailed("").is_none());
    }
}
