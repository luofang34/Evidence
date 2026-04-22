//! Golden fixture for `generate_compliance_report` output.
//!
//! Pins the on-disk wire format of `compliance/<crate>.json` for a
//! canonical `(DAL::B, mixed evidence)` input. The input is
//! deliberately chosen so the produced report exercises every
//! [`ObjectiveStatusKind`] variant — `Met`, `NotMet`, `Partial`,
//! `NotApplicable`, and `ManualReviewRequired` — in one document:
//!
//! - **Met**: A3-6, A4-6, A7-5 (trace links valid + tests passed).
//! - **Partial**: A6-5, A7-1..A7-4, A7-6, A7-7 (aggregate evidence
//!   but no per-requirement / per-LLR mapping available to the tool).
//! - **NotMet**: A7-8..A7-10 (no coverage data + MC/DC not supported).
//! - **NotApplicable**: objectives excluded at DAL-B by the Annex A
//!   applicability table.
//! - **ManualReviewRequired**: non-traceability objectives across
//!   Tables A-3, A-4, A-5, A-6 — the ~20 audits that always require
//!   a human reviewer regardless of what the tool collects.
//!
//! To regenerate after an *intentional* wire-format change, run:
//!
//! ```sh
//! EVIDENCE_UPDATE_FIXTURES=1 cargo test -p evidence \
//!     --test compliance_fixture
//! ```
//!
//! …and commit the updated JSON. An accidental serializer drift will
//! fail loudly here instead of silently breaking downstream audit
//! tooling that reads the bundle.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration test — panic on fixture I/O failure is fine"
)]

use std::fs;
use std::path::PathBuf;

use evidence_core::{CrateEvidence, Dal, ObjectiveStatusKind, generate_compliance_report};

const FIXTURE_PATH: &str = "tests/fixtures/compliance/dal_b_mixed_evidence.json";

fn fixture_full_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_PATH)
}

/// Input for the canonical fixture. Chosen to exercise every
/// [`ObjectiveStatusKind`] variant — see module rustdoc.
fn canonical_evidence() -> CrateEvidence {
    CrateEvidence {
        has_trace_data: true,
        trace_validation_passed: true,
        has_test_results: true,
        tests_passed: Some(true),
        has_coverage_data: false,
        has_per_test_outcomes: false,
        coverage_statement_percent: None,
        coverage_branch_percent: None,
    }
}

/// Render the canonical report as the exact bytes written to disk by
/// `cmd_generate` (`serde_json::to_string_pretty` + trailing LF).
fn render_canonical_report() -> String {
    let report = generate_compliance_report("fixture-crate", Dal::B, &canonical_evidence());
    let mut json = serde_json::to_string_pretty(&report).expect("serialize report");
    json.push('\n');
    json
}

/// Core byte-identity check: regenerate the canonical report and
/// compare to the committed fixture. The trailing newline mirrors
/// what `fs::write` of pretty JSON produces in the CLI.
#[test]
fn generated_report_matches_committed_fixture() {
    let rendered = render_canonical_report();
    let path = fixture_full_path();

    if std::env::var_os("EVIDENCE_UPDATE_FIXTURES").is_some() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixtures dir");
        }
        fs::write(&path, &rendered).expect("write fixture");
        eprintln!("updated fixture: {}", path.display());
        return;
    }

    let committed = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing or unreadable fixture {}: {e}\n\
             hint: run with EVIDENCE_UPDATE_FIXTURES=1 to write it",
            path.display()
        )
    });

    if committed != rendered {
        // Write a sibling `.actual` for diffing without touching the
        // committed fixture. Keeps CI output readable even when the
        // diff is large.
        let actual = path.with_extension("actual");
        fs::write(&actual, &rendered).ok();
        panic!(
            "compliance fixture drift.\n\
             fixture: {}\n\
             actual:  {}\n\
             If intentional: EVIDENCE_UPDATE_FIXTURES=1 cargo test -p evidence --test compliance_fixture",
            path.display(),
            actual.display()
        );
    }
}

/// Sanity: the canonical input really does hit every variant. If a
/// future generator tweak stops producing one of them the fixture
/// would still round-trip but would no longer be a useful regression
/// shield — this test fails loudly instead.
#[test]
fn canonical_report_exercises_every_variant() {
    let report = generate_compliance_report("fixture-crate", Dal::B, &canonical_evidence());

    let seen = |k: ObjectiveStatusKind| report.objectives.iter().any(|o| o.status == k);

    assert!(seen(ObjectiveStatusKind::Met), "no Met variant in report");
    assert!(
        seen(ObjectiveStatusKind::NotMet),
        "no NotMet variant in report"
    );
    assert!(
        seen(ObjectiveStatusKind::Partial),
        "no Partial variant in report"
    );
    assert!(
        seen(ObjectiveStatusKind::NotApplicable),
        "no NotApplicable variant in report"
    );
    assert!(
        seen(ObjectiveStatusKind::ManualReviewRequired),
        "no ManualReviewRequired variant in report"
    );
}
