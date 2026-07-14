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

use serde_json::Value;

use crate::bundle::TestSummary;
use crate::bundle::outcome_record::TestOutcomeRecord;

/// Result of parsing a nextest libtest-json-plus stream.
pub struct NextestRun {
    /// One record per executed or ignored test, sorted by identity.
    pub records: Vec<TestOutcomeRecord>,
    /// Aggregate counts summed across every suite.
    pub summary: TestSummary,
}

/// Parse a nextest `--message-format libtest-json-plus` stream. Lines
/// that are not JSON, or JSON without the fields this reads, are
/// skipped — a robustness choice so a future additive event type does
/// not break capture. Records are sorted for deterministic output.
pub fn parse_nextest_libtest_json(stdout: &str) -> NextestRun {
    let mut records: Vec<TestOutcomeRecord> = Vec::new();
    let (mut passed, mut failed, mut ignored, mut filtered_out) = (0u32, 0u32, 0u32, 0u32);

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
                passed += count(&v, "passed");
                failed += count(&v, "failed");
                ignored += count(&v, "ignored");
                filtered_out += count(&v, "filtered_out");
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

    let total = passed
        .saturating_add(failed)
        .saturating_add(ignored)
        .saturating_add(filtered_out);
    NextestRun {
        records,
        summary: TestSummary {
            total,
            passed,
            failed,
            ignored,
            filtered_out,
        },
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
