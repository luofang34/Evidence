//! Contract test for the `cargo evidence init` scaffold
//! (TEST-167, governing LLR-150 / LLR-151). Spawns the binary
//! against fresh tempdirs and pins the advertised adoption flow
//! `init -> doctor -> trace --validate -> generate` plus init's
//! own output contract. Re-run / `--force` / adoption-sequence
//! coverage lives in the sibling `init_scaffold_idempotency.rs`;
//! shared plumbing in `init_scaffold_helpers.rs` (the split keeps
//! every file under the workspace 500-line cap).
//!
//! State contract pinned here: a fresh scaffold is
//! adoption-incomplete by design — schema-valid everywhere, zero
//! live requirements, `DOCTOR_TRACE_NO_EVIDENCE` (warning) from
//! doctor, `TRACE_EVIDENCE_EMPTY` (non-success) from
//! `trace --validate`, and a dev bundle whose completeness states
//! record the empty graph honestly.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

#[path = "init_scaffold_helpers.rs"]
mod helpers;

use std::fs;

use tempfile::TempDir;

use helpers::{MANAGED_FILES, assert_no_live_entry_tables, cargo_evidence, parse_jsonl};

/// Fresh init scaffolds a schema-valid tree with zero live
/// requirements: every managed file exists, the trace files parse
/// through the library's own reader with empty entry lists, the
/// boundary and floors configs load through their own loaders,
/// and every example entry stays commented out.
#[test]
fn init_scaffold_is_schema_valid_and_contains_no_live_requirements() {
    let tmp = TempDir::new().expect("tempdir");

    cargo_evidence(tmp.path())
        .args(["evidence", "init"])
        .assert()
        .success();

    for rel in MANAGED_FILES {
        assert!(
            tmp.path().join(rel).is_file(),
            "managed file {rel} must exist after init"
        );
    }

    // Trace files parse against the CURRENT trace serde schema and
    // hold zero entries on every layer.
    let trace_root = tmp.path().join("cert").join("trace");
    let files =
        evidence_core::read_all_trace_files(trace_root.to_str().expect("trace path is UTF-8"))
            .expect("scaffold trace files must parse");
    assert!(files.sys.requirements.is_empty(), "sys must be empty");
    assert!(files.hlr.requirements.is_empty(), "hlr must be empty");
    assert!(files.llr.requirements.is_empty(), "llr must be empty");
    assert!(files.tests.tests.is_empty(), "tests must be empty");
    assert!(
        files
            .derived
            .expect("derived.toml present")
            .requirements
            .is_empty(),
        "derived must be empty"
    );

    // The boundary config loads through its own loader with the
    // scaffold's (empty) scope.
    let boundary = evidence_core::BoundaryConfig::load(&tmp.path().join("cert/boundary.toml"))
        .expect("scaffold boundary.toml must parse");
    assert!(boundary.scope.in_scope.is_empty());

    // The floors config loads as `Loaded` — never the missing
    // outcome doctor fails on.
    match evidence_core::FloorsConfig::load_or_missing(&tmp.path().join("cert/floors.toml")) {
        evidence_core::floors::LoadOutcome::Loaded(cfg) => {
            assert_eq!(
                cfg.schema_version,
                evidence_core::floors::FLOORS_SCHEMA_VERSION
            );
        }
        evidence_core::floors::LoadOutcome::Missing => {
            panic!("scaffold floors.toml must exist and load as Loaded, got Missing")
        }
        evidence_core::floors::LoadOutcome::Error(e) => {
            panic!("scaffold floors.toml must load as Loaded, got Error: {e}")
        }
    }

    // Non-evidence guarantee: no live entry tables anywhere.
    assert_no_live_entry_tables(&trace_root);
}

/// Init names the adoption-incomplete state itself on both output
/// shapes: the jsonl stream carries `INIT_ADOPTION_INCOMPLETE`
/// ahead of exactly one `INIT_OK` terminal, and the human output
/// prints the complete ordered next-step sequence — including the
/// backfill step the old output omitted.
#[test]
fn init_signals_adoption_incomplete_in_jsonl_and_human_output() {
    let tmp = TempDir::new().expect("tempdir");

    // jsonl: adoption diagnostic rides ahead of the single terminal.
    let out = cargo_evidence(tmp.path())
        .args(["evidence", "--format=jsonl", "init"])
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "jsonl init must exit 0; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let events = parse_jsonl(&out.stdout);
    let adoption: Vec<_> = events
        .iter()
        .filter(|e| e["code"] == "INIT_ADOPTION_INCOMPLETE")
        .collect();
    assert_eq!(
        adoption.len(),
        1,
        "exactly one INIT_ADOPTION_INCOMPLETE event:\n{events:?}"
    );
    assert_eq!(adoption[0]["severity"], "info");
    let message = adoption[0]["message"]
        .as_str()
        .expect("message is a string");
    for needle in [
        "adoption-incomplete",
        "cargo evidence trace --backfill-uuids",
        "cargo evidence trace --validate",
        "cargo evidence doctor",
        "cargo evidence generate --out-dir evidence",
    ] {
        assert!(
            message.contains(needle),
            "INIT_ADOPTION_INCOMPLETE message must carry `{needle}`:\n{message}"
        );
    }
    // Schema Rule 1: exactly one terminal, INIT_OK, and it is last;
    // the adoption finding is not a terminal.
    let terminals: Vec<_> = events
        .iter()
        .filter(|e| {
            let code = e["code"].as_str().unwrap_or("");
            code.ends_with("_OK") || code.ends_with("_FAIL") || code.ends_with("_ERROR")
        })
        .collect();
    assert_eq!(terminals.len(), 1, "exactly one terminal: {events:?}");
    assert_eq!(terminals[0]["code"], "INIT_OK");
    assert_eq!(
        events.last().expect("non-empty stream")["code"],
        "INIT_OK",
        "INIT_OK must be the terminal line"
    );
    // The refusal code is retired: a present cert/ tree no longer
    // fails init, and the code never appears on the wire.
    let out2 = cargo_evidence(tmp.path())
        .args(["evidence", "--format=jsonl", "init"])
        .output()
        .expect("spawn rerun");
    assert!(
        out2.status.success(),
        "re-run over an existing cert/ must exit 0; stderr:\n{}",
        String::from_utf8_lossy(&out2.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&out2.stdout).contains("INIT_CERT_DIR_EXISTS"),
        "INIT_CERT_DIR_EXISTS must never be emitted"
    );

    // Human: the printed sequence matches the advertised six steps.
    let tmp2 = TempDir::new().expect("tempdir");
    let human = cargo_evidence(tmp2.path())
        .args(["evidence", "init"])
        .output()
        .expect("spawn");
    assert!(human.status.success());
    let stdout = String::from_utf8_lossy(&human.stdout);
    for needle in [
        "adoption-incomplete",
        "1. Edit cert/boundary.toml",
        "2. Add real requirements to cert/trace/{sys,hlr,llr,tests}.toml",
        "3. Run: cargo evidence trace --backfill-uuids",
        "4. Run: cargo evidence trace --validate",
        "5. Run: cargo evidence doctor",
        "6. Run: cargo evidence generate --out-dir evidence",
    ] {
        assert!(
            stdout.contains(needle),
            "human next-steps must print `{needle}`:\n{stdout}"
        );
    }
}

/// The full advertised flow on a fresh scaffold pins the intended
/// states: doctor exits 0 (`DOCTOR_OK`) with the trace row at
/// `DOCTOR_TRACE_NO_EVIDENCE` warning severity and zero
/// error-severity rows; `trace --validate` fails closed with
/// `TRACE_EVIDENCE_EMPTY`; `generate --profile dev` succeeds and
/// records the empty graph honestly on the bundle's completeness
/// states — and copies no live requirements into the bundle.
#[test]
fn fresh_scaffold_doctor_validate_generate_contract() {
    let tmp = TempDir::new().expect("tempdir");
    cargo_evidence(tmp.path())
        .args(["evidence", "init"])
        .assert()
        .success();

    // doctor: adoption warning, no error-severity row, DOCTOR_OK.
    let doctor = cargo_evidence(tmp.path())
        .args(["evidence", "doctor", "--format=jsonl"])
        .output()
        .expect("spawn");
    assert_eq!(
        doctor.status.code(),
        Some(0),
        "doctor on a fresh scaffold must exit 0; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&doctor.stdout),
        String::from_utf8_lossy(&doctor.stderr)
    );
    let events = parse_jsonl(&doctor.stdout);
    let errors: Vec<_> = events.iter().filter(|e| e["severity"] == "error").collect();
    assert!(
        errors.is_empty(),
        "fresh scaffold must produce no error-severity doctor rows: {errors:?}"
    );
    let trace_rows: Vec<_> = events
        .iter()
        .filter(|e| e["code"] == "DOCTOR_TRACE_NO_EVIDENCE")
        .collect();
    assert_eq!(
        trace_rows.len(),
        1,
        "trace row must be the adoption-state warning: {events:?}"
    );
    assert_eq!(trace_rows[0]["severity"], "warning");
    assert_eq!(
        events.last().expect("non-empty stream")["code"],
        "DOCTOR_OK"
    );

    // trace --validate: the typed adoption-incomplete signal.
    let validate = cargo_evidence(tmp.path())
        .args(["evidence", "trace", "--validate", "--format=jsonl"])
        .output()
        .expect("spawn");
    assert_eq!(
        validate.status.code(),
        Some(2),
        "validate over a zero-requirement tree must exit 2 (verification failure)"
    );
    let events = parse_jsonl(&validate.stdout);
    assert_eq!(
        events.first().expect("non-empty stream")["code"],
        "TRACE_EVIDENCE_EMPTY",
        "the old fake-uid register failure is replaced by the typed adoption state"
    );
    assert_eq!(
        events.last().expect("non-empty stream")["code"],
        "VERIFY_FAIL"
    );

    // generate --profile dev: succeeds on the zero-scope scaffold
    // (dev warns and continues by design) and records the empty
    // capture honestly in the bundle's completeness states.
    let generate = cargo_evidence(tmp.path())
        .args([
            "evidence",
            "generate",
            "--profile",
            "dev",
            "--skip-tests",
            "--out-dir",
            "evidence",
        ])
        .output()
        .expect("spawn");
    assert_eq!(
        generate.status.code(),
        Some(0),
        "dev generate on a fresh scaffold must succeed; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&generate.stdout),
        String::from_utf8_lossy(&generate.stderr)
    );
    let bundle_dir = walkdir::WalkDir::new(tmp.path().join("evidence"))
        .follow_links(false)
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(Result::ok)
        .map(|e| e.into_path())
        .find(|p| p.is_dir())
        .expect("a bundle directory was written");
    let index: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(bundle_dir.join("index.json")).expect("read index.json"),
    )
    .expect("index.json parses");
    assert_eq!(
        index["completeness"]["graph_validity"], "incomplete",
        "the bundle must record the empty trace graph as incomplete, never complete"
    );

    // Non-evidence guarantee, bundle side: the trace copies inside
    // the bundle carry zero live entry tables.
    assert_no_live_entry_tables(&bundle_dir.join("trace"));
}
