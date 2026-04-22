//! Per-test outcome wire shape (`TestOutcomeRecord`) for
//! `tests/test_outcomes.jsonl` inside a generated bundle.
//!
//! One record per test from a `cargo test --workspace` run.
//! Name + module path + pass/fail status are required; an
//! `Option<String> failure_message` captures the panic /
//! assertion text libtest emits in its `---- <test> stdout ----`
//! block after the run summary. `duration_ms: Option<u64>` is
//! a reserved slot — libtest stable doesn't emit timings
//! (nightly `--report-time` does; nextest adds them on stable
//! via a separate tool install). Populating it is an additive
//! follow-up when a downstream consumer asks for timing
//! evidence; until then the field serializes absent via
//! `#[serde(skip_serializing_if = "Option::is_none")]`.
//!
//! Distinct from the parser-internal [`crate::bundle::TestOutcome`]
//! enum so the wire shape is decoupled from parser internals.
//! A refactor to the parser (nextest swap, syn-walker for
//! `file:line` capture, etc.) leaves consumers that read the
//! jsonl untouched.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diagnostic::{DiagnosticCode, Location, Severity};

use super::error::BuilderError;

/// Serialize `records` as JSONL to `<bundle_dir>/tests/test_outcomes.jsonl`.
/// `Ok(None)` when `records` is empty (silent skip — dev-profile
/// runs often skip tests); `Ok(Some(path))` on success so the
/// caller (typically [`crate::bundle::EvidenceBuilder::write_test_outcomes`])
/// can decide whether to hash the file into SHA256SUMS.
pub(crate) fn write_outcomes_jsonl(
    bundle_dir: &Path,
    records: &[TestOutcomeRecord],
) -> Result<Option<PathBuf>, BuilderError> {
    if records.is_empty() {
        return Ok(None);
    }
    let dir = bundle_dir.join("tests");
    fs::create_dir_all(&dir).map_err(|source| BuilderError::Io {
        op: "creating",
        path: dir.clone(),
        source,
    })?;
    let path = dir.join("test_outcomes.jsonl");
    let mut buf = String::new();
    for rec in records {
        let line = serde_json::to_string(rec).map_err(|source| BuilderError::Serialize {
            kind: "tests/test_outcomes.jsonl",
            source,
        })?;
        buf.push_str(&line);
        buf.push('\n');
    }
    fs::write(&path, buf).map_err(|source| BuilderError::Io {
        op: "writing",
        path: path.clone(),
        source,
    })?;
    Ok(Some(path))
}

/// Failure modes the outcome-capture pipeline can surface into
/// the diagnostic stream. Today a single variant — a
/// failure-block parser hit that references a test name not in
/// the aggregate outcomes map. Kept as an enum (not a single
/// struct) so future additions (timings mismatch, malformed
/// JSONL write, etc.) slot in without a wire break on the
/// existing variant.
#[derive(Debug, thiserror::Error)]
pub enum TestsError {
    /// A `---- <test> stdout ----` failure block referenced a
    /// test name that wasn't in the aggregate outcomes map —
    /// the parser saw panic text without a matching summary
    /// row. Graceful-degrade: the bundle still ships with the
    /// aggregate view; the specific failure message just
    /// isn't attached.
    #[error(
        "failure block references test '{test_name}' not present in the aggregate outcomes map"
    )]
    OutcomeParseFailed {
        /// Test name as libtest printed it.
        test_name: String,
    },
}

impl DiagnosticCode for TestsError {
    fn code(&self) -> &'static str {
        match self {
            TestsError::OutcomeParseFailed { .. } => "TESTS_OUTCOME_PARSE_FAILED",
        }
    }

    fn severity(&self) -> Severity {
        // Warning, not Error: on cert/record profile any Error
        // diagnostic blocks bundle finalization, which would
        // contradict the variant's docstring — "the bundle still
        // ships with the aggregate view; the specific failure
        // message just isn't attached." A libtest-format drift
        // shouldn't stop the bundle from shipping.
        Severity::Warning
    }

    fn location(&self) -> Option<Location> {
        None
    }
}

/// One atomic test outcome, serialized as one line of
/// `tests/test_outcomes.jsonl`.
///
/// Field ordering here matches the intended JSON key order;
/// serde_json preserves struct field order on serialize so the
/// on-disk shape is `{name, module_path, passed, ignored,
/// failure_message?, duration_ms?}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestOutcomeRecord {
    /// Bare test function name as libtest prints it. Example:
    /// `dev_profile_warning_passes`.
    pub name: String,

    /// Fully-qualified module path leading to the test fn, minus
    /// the test fn itself. Examples:
    /// `"verify_prerelease"` (integration test binary —
    /// no module prefix);
    /// `"evidence_core::env::capture::prerelease_tests"` (unit
    /// test — crate + module chain).
    ///
    /// The concatenation `{module_path}::{name}` is the
    /// libtest-qualified key the rest of the tool uses
    /// (`test_selector` resolution, requirement-to-test
    /// matching).
    pub module_path: String,

    /// `true` iff the test ran and succeeded.
    /// `false` iff the test ran and failed — `failure_message`
    /// should carry the captured libtest stdout block.
    /// Undefined when `ignored == true`; by convention `passed`
    /// is `false` for ignored tests (they did not pass).
    pub passed: bool,

    /// `true` iff the test was marked `#[ignore]` and did not
    /// run. Mutually exclusive with a meaningful `passed`
    /// value; see above.
    pub ignored: bool,

    /// On failure, the panic / assertion text libtest printed
    /// in its `---- <test> stdout ----` block. `None` for
    /// passing + ignored tests, and for failing tests whose
    /// output could not be matched back to this entry (see
    /// `TESTS_OUTCOME_PARSE_FAILED`). Kept as a single string
    /// (multi-line preserved) so readers can present it
    /// verbatim.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub failure_message: Option<String>,

    /// Per-test wall-clock duration in milliseconds. **Not
    /// populated today** — libtest stable doesn't emit timings.
    /// The field is reserved so a future nightly-libtest or
    /// nextest follow-up can fill it without a wire break.
    /// Serializes absent when `None`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub duration_ms: Option<u64>,
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

    /// A minimal passing record round-trips through JSON without
    /// losing precision or rewriting fields. Absent
    /// `failure_message` + `duration_ms` stay absent in the
    /// serialized output (not `"null"`).
    #[test]
    fn jsonl_roundtrip() {
        let rec = TestOutcomeRecord {
            name: "verify_ok_terminates_with_verify_ok_and_exit_zero".to_string(),
            module_path: "verify_jsonl".to_string(),
            passed: true,
            ignored: false,
            failure_message: None,
            duration_ms: None,
        };
        let wire = serde_json::to_string(&rec).expect("serialize");
        assert!(
            !wire.contains("failure_message"),
            "absent Option must not serialize; got: {wire}",
        );
        assert!(
            !wire.contains("duration_ms"),
            "absent Option must not serialize; got: {wire}",
        );
        let back: TestOutcomeRecord = serde_json::from_str(&wire).expect("deserialize");
        assert_eq!(back, rec);
    }

    /// A failing record carries the panic text verbatim (multi-
    /// line preserved).
    #[test]
    fn failing_record_preserves_multiline_failure_message() {
        let rec = TestOutcomeRecord {
            name: "divides_by_zero".to_string(),
            module_path: "arith::tests".to_string(),
            passed: false,
            ignored: false,
            failure_message: Some(
                "thread 'arith::tests::divides_by_zero' panicked at src/arith.rs:12:5:\n\
                 assertion `left != right` failed\n  left: 0\n right: 0"
                    .to_string(),
            ),
            duration_ms: None,
        };
        let wire = serde_json::to_string(&rec).expect("serialize");
        let back: TestOutcomeRecord = serde_json::from_str(&wire).expect("deserialize");
        assert_eq!(back, rec);
        assert!(
            back.failure_message
                .as_deref()
                .unwrap_or("")
                .contains("assertion")
        );
    }

    /// `write_outcomes_jsonl` end-to-end: produces a jsonl file
    /// under `<bundle_dir>/tests/`, one line per record,
    /// trailing newline on each, required keys present, every
    /// line parses back to the original struct.
    #[test]
    fn write_outcomes_jsonl_end_to_end() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let records = vec![
            TestOutcomeRecord {
                name: "a".to_string(),
                module_path: "mod1".to_string(),
                passed: true,
                ignored: false,
                failure_message: None,
                duration_ms: None,
            },
            TestOutcomeRecord {
                name: "b".to_string(),
                module_path: "mod1".to_string(),
                passed: false,
                ignored: false,
                failure_message: Some("boom".to_string()),
                duration_ms: None,
            },
        ];
        let path = write_outcomes_jsonl(tmp.path(), &records)
            .expect("write ok")
            .expect("path present");
        let body = std::fs::read_to_string(&path).expect("read back");
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2, "one line per record; got {body:?}");
        for (i, line) in lines.iter().enumerate() {
            let back: TestOutcomeRecord = serde_json::from_str(line).expect("line parses");
            assert_eq!(back, records[i]);
            // Required keys present (and in the struct-field
            // order serde emits).
            assert!(line.contains("\"name\""));
            assert!(line.contains("\"module_path\""));
            assert!(line.contains("\"passed\""));
            assert!(line.contains("\"ignored\""));
        }
        // Empty-records case → Ok(None), no file.
        let empty_tmp = tempfile::TempDir::new().expect("tempdir");
        let none_path = write_outcomes_jsonl(empty_tmp.path(), &[]).expect("write ok on empty");
        assert!(none_path.is_none());
        assert!(!empty_tmp.path().join("tests/test_outcomes.jsonl").exists());
    }
}
