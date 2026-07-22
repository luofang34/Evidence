//! Capture-plane categories for [`compare_bundles`]: `tests`,
//! `coverage`, `commands`. Absence on exactly one side is
//! legitimate capture variance for tests (a `--skip-tests` bundle
//! against a full-suite bundle), so the tests category reports
//! Added / Removed rather than Unverifiable.

use std::collections::BTreeMap;

use super::categories_content::{presence_label, status_of};
use super::{
    CategoryDiff, DiffCategoryStatus, DiffError, Load, Side, file_exists, read_json_file,
    unverifiable,
};
use crate::bundle::{CommandRecord, TestOutcomeRecord, TestSummary};
use crate::coverage::{CoverageLevel, CoverageReport, Measurement};

/// One side's test evidence: the index-recorded summary, the
/// per-test outcome rows, and the captured-log presence flags.
struct TestSide {
    summary: Option<TestSummary>,
    outcomes: Load<Vec<TestOutcomeRecord>>,
    stdout_log: bool,
    stderr_log: bool,
}

impl TestSide {
    fn load(side: &Side) -> Result<Self, DiffError> {
        let summary = match &side.index {
            Load::Ok(index) => index.test_summary.clone(),
            _ => None,
        };
        Ok(TestSide {
            summary,
            outcomes: read_outcomes_jsonl(&side.root)?,
            stdout_log: file_exists(side, "tests/cargo_test_stdout.txt"),
            stderr_log: file_exists(side, "tests/cargo_test_stderr.txt"),
        })
    }

    /// Whether this side carries any test evidence at all.
    fn has_data(&self) -> bool {
        self.summary.is_some()
            || matches!(&self.outcomes, Load::Ok(rows) if !rows.is_empty())
            || self.stdout_log
            || self.stderr_log
    }

    /// Human summary of the evidence present on this side, used by
    /// the Added / Removed one-sided report.
    fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(s) = &self.summary {
            parts.push(format!(
                "summary total={} passed={} failed={}",
                s.total, s.passed, s.failed
            ));
        }
        if let Load::Ok(rows) = &self.outcomes {
            parts.push(format!("{} outcome row(s)", rows.len()));
        }
        let mut logs = Vec::new();
        if self.stdout_log {
            logs.push("stdout");
        }
        if self.stderr_log {
            logs.push("stderr");
        }
        if !logs.is_empty() {
            parts.push(format!("captured logs: {}", logs.join("+")));
        }
        if parts.is_empty() {
            "no recorded detail".to_string()
        } else {
            parts.join("; ")
        }
    }
}

/// Read `tests/test_outcomes.jsonl` — one `TestOutcomeRecord` per
/// line, not a JSON array. Absent → [`Load::Missing`]; any line
/// that fails to parse → [`Load::Unparseable`].
fn read_outcomes_jsonl(root: &std::path::Path) -> Result<Load<Vec<TestOutcomeRecord>>, DiffError> {
    let path = root.join("tests/test_outcomes.jsonl");
    if !path.exists() {
        return Ok(Load::Missing);
    }
    let content = std::fs::read_to_string(&path).map_err(|source| DiffError::Io {
        path: path.clone(),
        source,
    })?;
    let mut rows = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<TestOutcomeRecord>(line) {
            Ok(row) => rows.push(row),
            Err(_) => return Ok(Load::Unparseable),
        }
    }
    Ok(Load::Ok(rows))
}

/// Stable comparison key for one outcome row — the
/// libtest-qualified name the rest of the tool uses.
fn outcome_key(row: &TestOutcomeRecord) -> String {
    format!("{}::{}", row.module_path, row.name)
}

/// Status label for one row: ignored / passed / failed.
fn outcome_label(row: &TestOutcomeRecord) -> &'static str {
    if row.ignored {
        "ignored"
    } else if row.passed {
        "passed"
    } else {
        "failed"
    }
}

/// `tests` — the recorded test summary plus row-level per-test
/// outcomes plus captured-log presence. Rows compare pass/fail
/// and presence only: `duration_ms` (host timing noise) and
/// `failure_message` text (diagnostic prose) are excluded.
pub(crate) fn tests(a: &Side, b: &Side) -> Result<CategoryDiff, DiffError> {
    let ta = TestSide::load(a)?;
    let tb = TestSide::load(b)?;
    let (has_a, has_b) = (ta.has_data(), tb.has_data());
    if !has_a && !has_b {
        return Ok(CategoryDiff {
            category: "tests",
            status: DiffCategoryStatus::Equal,
            details: vec!["no test artifacts on either side".to_string()],
        });
    }
    if has_a != has_b {
        let (status, present, label) = if has_b {
            (DiffCategoryStatus::Added, &tb, "B")
        } else {
            (DiffCategoryStatus::Removed, &ta, "A")
        };
        return Ok(CategoryDiff {
            category: "tests",
            status,
            details: vec![format!(
                "test evidence present only in bundle {label}: {}",
                present.describe()
            )],
        });
    }

    let mut details = Vec::new();
    let mut changed = false;
    let mut unexaminable = false;

    // Summaries (index-recorded). Absent on one side is a real
    // delta — the runs captured different evidence.
    match (&ta.summary, &tb.summary) {
        (Some(sa), Some(sb)) => {
            for (label, va, vb) in [
                ("total", sa.total, sb.total),
                ("passed", sa.passed, sb.passed),
                ("failed", sa.failed, sb.failed),
                ("ignored", sa.ignored, sb.ignored),
                ("filtered_out", sa.filtered_out, sb.filtered_out),
            ] {
                if va != vb {
                    changed = true;
                    details.push(format!("~ test_summary.{label}: {va} -> {vb}"));
                }
            }
        }
        (sa, sb) if sa.is_some() != sb.is_some() => {
            changed = true;
            details.push(format!(
                "~ test_summary: {} -> {}",
                presence_label(sa.is_some()),
                presence_label(sb.is_some())
            ));
        }
        _ => {}
    }

    // Outcome rows, keyed by libtest-qualified name.
    match (&ta.outcomes, &tb.outcomes) {
        (Load::Unparseable, _) => {
            unexaminable = true;
            details.push("! tests/test_outcomes.jsonl unparseable in bundle A".to_string());
        }
        (_, Load::Unparseable) => {
            unexaminable = true;
            details.push("! tests/test_outcomes.jsonl unparseable in bundle B".to_string());
        }
        (Load::Missing, Load::Missing) => {}
        (Load::Missing, Load::Ok(rows)) => {
            changed = true;
            details.push("! tests/test_outcomes.jsonl absent in bundle A".to_string());
            for row in rows {
                details.push(format!("+ {} ({})", outcome_key(row), outcome_label(row)));
            }
        }
        (Load::Ok(rows), Load::Missing) => {
            changed = true;
            details.push("! tests/test_outcomes.jsonl absent in bundle B".to_string());
            for row in rows {
                details.push(format!("- {} ({})", outcome_key(row), outcome_label(row)));
            }
        }
        (Load::Ok(ra), Load::Ok(rb)) => {
            let map_a: BTreeMap<String, &TestOutcomeRecord> =
                ra.iter().map(|r| (outcome_key(r), r)).collect();
            let map_b: BTreeMap<String, &TestOutcomeRecord> =
                rb.iter().map(|r| (outcome_key(r), r)).collect();
            for (key, row_a) in &map_a {
                match map_b.get(key) {
                    None => {
                        changed = true;
                        details.push(format!("- {key} ({})", outcome_label(row_a)));
                    }
                    Some(row_b)
                        if (row_a.passed, row_a.ignored) != (row_b.passed, row_b.ignored) =>
                    {
                        changed = true;
                        details.push(format!(
                            "~ {key}: {} -> {}",
                            outcome_label(row_a),
                            outcome_label(row_b)
                        ));
                    }
                    Some(_) => {}
                }
            }
            for (key, row_b) in &map_b {
                if !map_a.contains_key(key) {
                    changed = true;
                    details.push(format!("+ {key} ({})", outcome_label(row_b)));
                }
            }
        }
    }

    // Captured-log presence.
    for (file, la, lb) in [
        ("tests/cargo_test_stdout.txt", ta.stdout_log, tb.stdout_log),
        ("tests/cargo_test_stderr.txt", ta.stderr_log, tb.stderr_log),
    ] {
        if la != lb {
            changed = true;
            details.push(format!(
                "~ {file}: {} -> {}",
                presence_label(la),
                presence_label(lb)
            ));
        }
    }

    Ok(CategoryDiff {
        category: "tests",
        status: status_of(changed, unexaminable),
        details,
    })
}

/// Levels in canonical report order.
const LEVEL_ORDER: &[CoverageLevel] = &[
    CoverageLevel::Statement,
    CoverageLevel::Branch,
    CoverageLevel::PatternDecision,
    CoverageLevel::Mcdc,
];

/// Snake-case label of a coverage level, matching its wire name.
fn level_label(level: CoverageLevel) -> &'static str {
    match level {
        CoverageLevel::Statement => "statement",
        CoverageLevel::Branch => "branch",
        CoverageLevel::PatternDecision => "pattern_decision",
        CoverageLevel::Mcdc => "mcdc",
    }
}

/// Aggregate `(covered, total)` lines for one measurement.
fn aggregate_lines(m: &Measurement) -> (u64, u64) {
    (
        m.per_file.iter().map(|f| f.lines.covered).sum(),
        m.per_file.iter().map(|f| f.lines.total).sum(),
    )
}

/// Aggregate `(covered, total)` branches, `None` when no file
/// carries branch data.
fn aggregate_branches(m: &Measurement) -> Option<(u64, u64)> {
    let mut any = false;
    let (mut covered, mut total) = (0u64, 0u64);
    for f in &m.per_file {
        if let Some(b) = &f.branches {
            any = true;
            covered += b.covered;
            total += b.total;
        }
    }
    any.then_some((covered, total))
}

/// `coverage` — aggregate covered/total per measurement level,
/// plus `lcov.info` presence; the category reports the numbers a
/// reviewer quotes. Absent on both sides is equal-by-absence
/// (coverage capture is opt-in on dev); absent on exactly one
/// side is unverifiable — the category cannot claim the runs
/// captured the same thing.
pub(crate) fn coverage(a: &Side, b: &Side) -> Result<CategoryDiff, DiffError> {
    let ra = read_json_file::<CoverageReport>(&a.root, "coverage/coverage_summary.json")?;
    let rb = read_json_file::<CoverageReport>(&b.root, "coverage/coverage_summary.json")?;
    let lcov_a = file_exists(a, "coverage/lcov.info");
    let lcov_b = file_exists(b, "coverage/lcov.info");

    let mut details = Vec::new();
    let mut changed = false;
    let mut unexaminable = false;

    match (&ra, &rb) {
        (Load::Unparseable, _) => {
            unexaminable = true;
            details.push("! coverage/coverage_summary.json unparseable in bundle A".to_string());
        }
        (_, Load::Unparseable) => {
            unexaminable = true;
            details.push("! coverage/coverage_summary.json unparseable in bundle B".to_string());
        }
        (Load::Missing, Load::Missing) => {}
        (Load::Missing, _) => {
            unexaminable = true;
            details.push("! coverage/coverage_summary.json absent in bundle A".to_string());
        }
        (_, Load::Missing) => {
            unexaminable = true;
            details.push("! coverage/coverage_summary.json absent in bundle B".to_string());
        }
        (Load::Ok(ca), Load::Ok(cb)) => {
            for level in LEVEL_ORDER {
                let ma = ca.measurements.iter().find(|m| m.level == *level);
                let mb = cb.measurements.iter().find(|m| m.level == *level);
                match (ma, mb) {
                    (None, None) => {}
                    (Some(_), None) => {
                        changed = true;
                        details.push(format!("- {} measurement", level_label(*level)));
                    }
                    (None, Some(_)) => {
                        changed = true;
                        details.push(format!("+ {} measurement", level_label(*level)));
                    }
                    (Some(ma), Some(mb)) => {
                        let (cla, tla) = aggregate_lines(ma);
                        let (clb, tlb) = aggregate_lines(mb);
                        if (cla, tla) != (clb, tlb) {
                            changed = true;
                            details.push(format!(
                                "~ {} line coverage: {cla}/{tla} -> {clb}/{tlb}",
                                level_label(*level)
                            ));
                        }
                        match (aggregate_branches(ma), aggregate_branches(mb)) {
                            (Some(ba), Some(bb)) if ba != bb => {
                                changed = true;
                                details.push(format!(
                                    "~ {} branch coverage: {}/{} -> {}/{}",
                                    level_label(*level),
                                    ba.0,
                                    ba.1,
                                    bb.0,
                                    bb.1
                                ));
                            }
                            (ba, bb) if ba.is_some() != bb.is_some() => {
                                changed = true;
                                details.push(format!(
                                    "~ {} branch data: {} -> {}",
                                    level_label(*level),
                                    presence_label(ba.is_some()),
                                    presence_label(bb.is_some())
                                ));
                            }
                            _ => {}
                        }
                        if ma.engine_version != mb.engine_version {
                            changed = true;
                            details.push(format!(
                                "~ {} engine_version: {} -> {}",
                                level_label(*level),
                                ma.engine_version,
                                mb.engine_version
                            ));
                        }
                    }
                }
            }
        }
    }

    if lcov_a != lcov_b {
        changed = true;
        details.push(format!(
            "~ coverage/lcov.info: {} -> {}",
            presence_label(lcov_a),
            presence_label(lcov_b)
        ));
    }

    // The summary file is the category's primary artifact: when
    // it is missing or unparseable on a side the plane cannot be
    // compared, so Unverifiable dominates any secondary (lcov
    // presence) delta — the `!` details keep the parts visible.
    let status = if unexaminable {
        DiffCategoryStatus::Unverifiable
    } else {
        status_of(changed, false)
    };
    if status == DiffCategoryStatus::Equal && details.is_empty() {
        details.push("no coverage data on either side".to_string());
    }
    Ok(CategoryDiff {
        category: "coverage",
        status,
        details,
    })
}

/// `commands` — the `commands.json` rows. Compared per row index:
/// argv and exit code, plus the presence of the captured
/// stdout/stderr files each row references. `cwd` is excluded —
/// it is host-specific path noise, not recipe content.
pub(crate) fn commands(a: &Side, b: &Side) -> Result<CategoryDiff, DiffError> {
    let ca = read_json_file::<Vec<CommandRecord>>(&a.root, "commands.json")?;
    let cb = read_json_file::<Vec<CommandRecord>>(&b.root, "commands.json")?;
    let (Load::Ok(ra), Load::Ok(rb)) = (&ca, &cb) else {
        let reason = match (&ca, &cb) {
            (Load::Missing, Load::Missing) => "commands.json missing in both bundles",
            (Load::Missing, _) => "commands.json missing in bundle A",
            (_, Load::Missing) => "commands.json missing in bundle B",
            (Load::Unparseable, _) => "commands.json unparseable in bundle A",
            (_, Load::Unparseable) => "commands.json unparseable in bundle B",
            _ => "commands.json unavailable",
        };
        return Ok(unverifiable("commands", reason.to_string()));
    };

    let mut details = Vec::new();
    let common = ra.len().min(rb.len());
    for (i, (row_a, row_b)) in ra.iter().zip(rb.iter()).enumerate().take(common) {
        if row_a.argv != row_b.argv {
            details.push(format!(
                "~ row {i} argv: {} -> {}",
                row_a.argv.join(" "),
                row_b.argv.join(" ")
            ));
        }
        if row_a.exit_code != row_b.exit_code {
            details.push(format!(
                "~ row {i} exit_code: {} -> {}",
                row_a.exit_code, row_b.exit_code
            ));
        }
        for (label, pa, pb) in [
            ("stdout", &row_a.stdout_path, &row_b.stdout_path),
            ("stderr", &row_a.stderr_path, &row_b.stderr_path),
        ] {
            let present_a = pa.as_ref().is_some_and(|p| a.root.join(p).is_file());
            let present_b = pb.as_ref().is_some_and(|p| b.root.join(p).is_file());
            if present_a != present_b {
                details.push(format!(
                    "~ row {i} captured {label}: {} -> {}",
                    presence_label(present_a),
                    presence_label(present_b)
                ));
            }
        }
    }
    for (i, row) in ra.iter().enumerate().skip(common) {
        details.push(format!("- row {i}: {}", row.argv.join(" ")));
    }
    for (i, row) in rb.iter().enumerate().skip(common) {
        details.push(format!("+ row {i}: {}", row.argv.join(" ")));
    }

    Ok(CategoryDiff {
        category: "commands",
        status: if details.is_empty() {
            DiffCategoryStatus::Equal
        } else {
            DiffCategoryStatus::Changed
        },
        details,
    })
}
