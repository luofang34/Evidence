//! Parser for cargo-nextest's `libtest-json-plus` event stream.
//!
//! nextest emits one JSON object per line. A `suite` event carries the
//! test binary's identity (`{crate, test_binary, kind}`) and, on
//! completion, the per-binary counts; a `test` event carries each
//! test's fully-qualified name `{crate}::{binary}${module::path::name}`
//! and its outcome. This module turns that stream into per-test
//! [`TestOutcomeRecord`]s that preserve package + binary + harness
//! identity — the identity that was lost as `__unknown_binary__` when
//! generate parsed plain libtest text — plus an aggregate
//! [`TestSummary`].
//!
//! Determinism (SYS-003): `test_outcomes.jsonl` is a hashed bundle
//! file, so this parser must produce byte-identical output across runs.
//! nextest emits test events in completion order, so records are sorted
//! by identity; per-test `exec_time` is deliberately dropped (wall-clock
//! timings vary run to run and would rotate the bundle hash).

use std::collections::BTreeMap;
use std::fmt;

use serde_json::Value;

use crate::bundle::TestSummary;
use crate::bundle::outcome_record::TestOutcomeRecord;

/// Result of parsing a nextest libtest-json-plus stream.
pub struct NextestRun {
    /// One record per executed or ignored test, sorted by identity.
    pub records: Vec<TestOutcomeRecord>,
    /// Aggregate counts: each binary's suite summaries reduced per
    /// binary identity (partitions do not double-count), then summed
    /// across distinct binaries.
    pub summary: TestSummary,
}

/// One outcome dimension where the suite-level [`TestSummary`] and the
/// per-test [`NextestRun::records`] disagree. The two are derived from
/// independent event families in the stream, so a discrepancy means an
/// event was dropped or the stream was truncated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryDiscrepancy {
    /// The outcome dimension (`passed` / `failed` / `ignored`).
    pub dimension: &'static str,
    /// Count from the suite-completion summary events.
    pub summary: u32,
    /// Count tallied from the per-test records.
    pub records: u32,
}

impl fmt::Display for SummaryDiscrepancy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: summary={} records={}",
            self.dimension, self.summary, self.records
        )
    }
}

impl NextestRun {
    /// Reconcile the suite-level [`TestSummary`] against the per-test
    /// [`records`](Self::records). nextest derives the two from
    /// independent event families — `suite` completion counts vs.
    /// individual `test` events — so a mismatch means the stream was
    /// truncated or an event dropped, and the captured test evidence
    /// cannot be trusted. Returns one [`SummaryDiscrepancy`] per
    /// disagreeing dimension; an empty vec means the capture is
    /// internally consistent.
    ///
    /// Two categories are deliberately out of scope. `filtered_out`
    /// tests emit no `test` event, so they have no record to reconcile
    /// against. Doctests are not run by `cargo nextest` at all, so
    /// neither tally counts them — the reconciliation is exact over
    /// nextest's executed-test scope, and doctest evidence, if required,
    /// is a separate capture rather than a silent inclusion here.
    pub fn reconcile(&self) -> Vec<SummaryDiscrepancy> {
        let mut rec_passed = 0u32;
        let mut rec_failed = 0u32;
        let mut rec_ignored = 0u32;
        for r in &self.records {
            if r.ignored {
                rec_ignored = rec_ignored.wrapping_add(1);
            } else if r.passed {
                rec_passed = rec_passed.wrapping_add(1);
            } else {
                rec_failed = rec_failed.wrapping_add(1);
            }
        }
        [
            ("passed", self.summary.passed, rec_passed),
            ("failed", self.summary.failed, rec_failed),
            ("ignored", self.summary.ignored, rec_ignored),
        ]
        .into_iter()
        .filter(|(_, summary, records)| summary != records)
        .map(|(dimension, summary, records)| SummaryDiscrepancy {
            dimension,
            summary,
            records,
        })
        .collect()
    }
}

/// Parse a nextest `--message-format libtest-json-plus` stream. Lines
/// that are not JSON, or JSON without the fields this reads, are
/// skipped — a robustness choice so a future additive event type does
/// not break capture. Records are sorted for deterministic output.
///
/// nextest can emit more than one `suite` "ok"/"failed" summary for the
/// same test binary when it partitions that binary's tests across
/// execution groups. `passed`/`failed` are disjoint across a binary's
/// partitions (their sum is the binary's true count), but `ignored` and
/// `filtered_out` are whole-binary tallies each partition restates, so
/// summing them across partitions double-counts — the source of a
/// spurious `reconcile` mismatch (`ignored: summary=2 records=1`) that
/// fails a cert bundle closed. Suite counts are therefore reduced per
/// binary identity before the cross-binary sum (see `SuiteTally`).
pub fn parse_nextest_libtest_json(stdout: &str) -> NextestRun {
    let mut records: Vec<TestOutcomeRecord> = Vec::new();
    let mut suites: BTreeMap<String, SuiteTally> = BTreeMap::new();
    let mut noident_seq: u32 = 0;

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ty = v.get("type").and_then(Value::as_str).unwrap_or_default();
        let event = v.get("event").and_then(Value::as_str).unwrap_or_default();
        match (ty, event) {
            ("suite", "ok" | "failed") => {
                let key = suite_identity(&v).unwrap_or_else(|| {
                    // A summary without nextest identity cannot be a
                    // partition of any other — give it a unique key so
                    // it stands alone (summed, never reduced away).
                    let k = format!("\0noident\0{noident_seq}");
                    noident_seq = noident_seq.wrapping_add(1);
                    k
                });
                suites.entry(key).or_default().fold(&v);
            }
            ("test", ev @ ("ok" | "failed" | "ignored")) => {
                if let Some(name) = v.get("name").and_then(Value::as_str) {
                    records.push(build_record(name, ev, &v));
                }
            }
            _ => {}
        }
    }

    records.sort_by(|a, b| {
        (a.module_path.as_str(), a.name.as_str()).cmp(&(b.module_path.as_str(), b.name.as_str()))
    });

    NextestRun {
        records,
        summary: summarize_suites(&suites),
    }
}

/// Per-binary reduction of nextest `suite` summaries. Within one binary's
/// (possibly partitioned) summaries, `passed`/`failed` accumulate
/// (disjoint executions), `ignored` takes the maximum (a whole-binary
/// tally each partition restates — max is robust if a partition
/// under-reports it), and `filtered_out` takes the minimum (a partition's
/// internal execution filter is not the binary's user-facing filtered-out
/// count; the least-filtered partition ran the most of the binary).
#[derive(Default)]
struct SuiteTally {
    passed: u32,
    failed: u32,
    ignored: u32,
    filtered_out: Option<u32>,
}

impl SuiteTally {
    fn fold(&mut self, v: &Value) {
        self.passed = self.passed.saturating_add(count(v, "passed"));
        self.failed = self.failed.saturating_add(count(v, "failed"));
        self.ignored = self.ignored.max(count(v, "ignored"));
        let fo = count(v, "filtered_out");
        self.filtered_out = Some(self.filtered_out.map_or(fo, |m| m.min(fo)));
    }
}

/// Identity of the binary a `suite` summary belongs to, from the
/// `nextest` extension object. `None` when absent (plain libtest JSON),
/// so such a summary is treated as standalone rather than merged.
fn suite_identity(v: &Value) -> Option<String> {
    let n = v.get("nextest")?;
    let field = |k: &str| n.get(k).and_then(Value::as_str);
    Some(format!(
        "{}\0{}\0{}",
        field("crate")?,
        field("test_binary")?,
        field("kind")?
    ))
}

/// Fold the per-binary tallies into one workspace-wide [`TestSummary`]:
/// sum every dimension across *distinct* binaries (each already reduced
/// over its own partitions).
fn summarize_suites(suites: &BTreeMap<String, SuiteTally>) -> TestSummary {
    let (mut passed, mut failed, mut ignored, mut filtered_out) = (0u32, 0u32, 0u32, 0u32);
    for t in suites.values() {
        passed = passed.saturating_add(t.passed);
        failed = failed.saturating_add(t.failed);
        ignored = ignored.saturating_add(t.ignored);
        filtered_out = filtered_out.saturating_add(t.filtered_out.unwrap_or(0));
    }
    let total = passed
        .saturating_add(failed)
        .saturating_add(ignored)
        .saturating_add(filtered_out);
    TestSummary {
        total,
        passed,
        failed,
        ignored,
        filtered_out,
    }
}

fn count(v: &Value, key: &str) -> u32 {
    v.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0)
}

fn build_record(name: &str, event: &str, v: &Value) -> TestOutcomeRecord {
    let id = parse_identity(name);
    let failure_message = if event == "failed" {
        v.get("stdout")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    } else {
        None
    };
    TestOutcomeRecord {
        name: id.test_name,
        module_path: id.module_path,
        package: id.package,
        binary: id.binary,
        harness: "libtest".to_string(),
        passed: event == "ok",
        ignored: event == "ignored",
        failure_message,
        // Intentionally not populated from `exec_time` — see module docs.
        duration_ms: None,
        requirement_uids: Vec::new(),
    }
}

struct Identity {
    package: String,
    binary: String,
    module_path: String,
    test_name: String,
}

/// Split a nextest test name `{crate}::{binary}${module::path::name}`
/// into its parts. `module_path` is reconstructed to begin with the
/// binary (`{binary}` or `{binary}::{module}`) so `{module_path}::{name}`
/// equals the `test_selector` format the trace uses, and tests with the
/// same name in different binaries stay distinguishable.
///
/// The binary is normalized to its crate-identifier form (`-` → `_`).
/// nextest reports a bin target's name verbatim (e.g. `cargo-evidence`),
/// but the module-path convention and the libtest-text capture path both
/// use the underscored identifier (`cargo_evidence`, from the
/// `deps/cargo_evidence-<hash>` filename). Normalizing here keeps one
/// canonical key so `check` (text) and `verify` (nextest) resolve the
/// same selectors.
fn parse_identity(name: &str) -> Identity {
    let (prefix, rest) = name.split_once('$').unwrap_or(("", name));
    let (package, binary) = match prefix.rsplit_once("::") {
        Some((pkg, bin)) => (pkg.to_string(), bin.replace('-', "_")),
        None => (String::new(), prefix.replace('-', "_")),
    };
    let (module_chain, test_name) = match rest.rsplit_once("::") {
        Some((m, t)) => (m, t),
        None => ("", rest),
    };
    let module_path = match (binary.is_empty(), module_chain.is_empty()) {
        (true, true) => String::new(),
        (true, false) => module_chain.to_string(),
        (false, true) => binary.clone(),
        (false, false) => format!("{binary}::{module_chain}"),
    };
    Identity {
        package,
        binary,
        module_path,
        test_name: test_name.to_string(),
    }
}

#[cfg(test)]
#[path = "nextest/tests.rs"]
mod tests;
