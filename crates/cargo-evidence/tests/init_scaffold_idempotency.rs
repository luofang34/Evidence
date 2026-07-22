//! Idempotency and adoption-sequence coverage for
//! `cargo evidence init` (TEST-167, governing LLR-151). Sibling
//! of `init_scaffold_contract.rs`; shared plumbing in
//! `init_scaffold_helpers.rs` (the split keeps every file under
//! the workspace 500-line cap).
//!
//! Pinned semantics: without `--force`, re-running init preserves
//! every existing file byte-for-byte (user evidence is never
//! overwritten) and exits 0; with `--force`, exactly the managed
//! template set is rewritten and nothing outside it. The
//! documented adoption sequence — add a real requirement, run
//! `trace --backfill-uuids`, run `trace --validate` — is
//! executable as printed, with no undocumented step.

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

use helpers::{MANAGED_FILES, cargo_evidence, parse_jsonl};

/// Idempotency: a second init without `--force` exits 0, leaves
/// every file byte-identical, and preserves a hand-edited managed
/// file — re-running never overwrites user evidence.
#[test]
fn init_rerun_preserves_existing_files_without_force() {
    let tmp = TempDir::new().expect("tempdir");
    cargo_evidence(tmp.path())
        .args(["evidence", "init"])
        .assert()
        .success();

    // Hand-edit one managed file the way an adopter would.
    let hlr_path = tmp.path().join("cert/trace/hlr.toml");
    let edited = format!(
        "{}\n# hand-authored note that must survive a re-run\n",
        fs::read_to_string(&hlr_path).expect("read hlr.toml")
    );
    fs::write(&hlr_path, &edited).expect("edit hlr.toml");

    let snapshot: Vec<(String, String)> = MANAGED_FILES
        .iter()
        .map(|rel| {
            (
                rel.to_string(),
                fs::read_to_string(tmp.path().join(rel)).expect("read managed file"),
            )
        })
        .collect();

    let out = cargo_evidence(tmp.path())
        .args(["evidence", "init"])
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "second init must exit 0; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("preserved"),
        "second init must report per-file preservation:\n{stdout}"
    );

    for (rel, before) in &snapshot {
        let after = fs::read_to_string(tmp.path().join(rel)).expect("read managed file");
        assert_eq!(*before, after, "{rel} must be byte-identical after re-run");
    }
    assert_eq!(
        fs::read_to_string(&hlr_path).expect("read hlr.toml"),
        edited,
        "a hand-edited managed file must be preserved verbatim"
    );
}

/// `--force` rewrites exactly the managed template set — edits to
/// managed files are discarded — while user evidence outside the
/// managed set stays untouched.
#[test]
fn init_force_rewrites_managed_templates_only() {
    let tmp = TempDir::new().expect("tempdir");
    cargo_evidence(tmp.path())
        .args(["evidence", "init"])
        .assert()
        .success();

    // A pristine copy of each managed template for comparison.
    let pristine: Vec<(String, String)> = MANAGED_FILES
        .iter()
        .map(|rel| {
            (
                rel.to_string(),
                fs::read_to_string(tmp.path().join(rel)).expect("read managed file"),
            )
        })
        .collect();

    // Edits inside the managed set + evidence outside it.
    let boundary_path = tmp.path().join("cert/boundary.toml");
    fs::write(
        &boundary_path,
        format!(
            "{}\n# hand edit --force must discard\n",
            fs::read_to_string(&boundary_path).expect("read boundary.toml")
        ),
    )
    .expect("edit boundary.toml");
    let user_notes = tmp.path().join("cert/USER_NOTES.md");
    fs::write(&user_notes, "adopter-owned notes\n").expect("write user notes");

    cargo_evidence(tmp.path())
        .args(["evidence", "init", "--force"])
        .assert()
        .success();

    for (rel, before) in &pristine {
        let after = fs::read_to_string(tmp.path().join(rel)).expect("read managed file");
        assert_eq!(
            *before, after,
            "--force must rewrite {rel} to the pristine template"
        );
    }
    assert_eq!(
        fs::read_to_string(&user_notes).expect("read user notes"),
        "adopter-owned notes\n",
        "files outside the managed set are never touched"
    );
}

/// The documented adoption sequence is executable as printed —
/// no undocumented step: add a real entry (placeholder uid), run
/// `trace --backfill-uuids`, then `trace --validate` terminates
/// `VERIFY_OK`. A second backfill is a no-op.
#[test]
fn documented_adoption_sequence_executes_end_to_end() {
    let tmp = TempDir::new().expect("tempdir");
    cargo_evidence(tmp.path())
        .args(["evidence", "init"])
        .assert()
        .success();

    // Adopt the way the template comment instructs: delete the
    // `requirements = []` line and add a first real entry with a
    // placeholder uid.
    let sys_path = tmp.path().join("cert/trace/sys.toml");
    let scaffold = fs::read_to_string(&sys_path).expect("read sys.toml");
    let adopted = format!(
        "{}\n[[requirements]]\nuid = \"SYS-001\"\nid = \"sys-first\"\n\
         title = \"First real requirement\"\nowner = \"team@example.com\"\n\
         verification_methods = [\"review\"]\ntraces_to = []\n",
        scaffold.replace("requirements = []\n\n", "")
    );
    fs::write(&sys_path, adopted).expect("write adopted sys.toml");

    cargo_evidence(tmp.path())
        .args(["evidence", "trace", "--backfill-uuids"])
        .assert()
        .success();

    // The placeholder uid was replaced with a real UUID.
    let backfilled = fs::read_to_string(&sys_path).expect("read backfilled sys.toml");
    assert!(
        !backfilled.contains("uid = \"SYS-001\""),
        "backfill must replace the placeholder uid:\n{backfilled}"
    );

    // Validate now terminates VERIFY_OK over the adopted entry.
    let validate = cargo_evidence(tmp.path())
        .args(["evidence", "trace", "--validate", "--format=jsonl"])
        .output()
        .expect("spawn");
    assert_eq!(
        validate.status.code(),
        Some(0),
        "validate after the documented backfill step must succeed; stdout:\n{}",
        String::from_utf8_lossy(&validate.stdout)
    );
    let events = parse_jsonl(&validate.stdout);
    assert_eq!(
        events.last().expect("non-empty stream")["code"],
        "VERIFY_OK"
    );

    // Second backfill is a no-op: nothing to assign, file untouched.
    cargo_evidence(tmp.path())
        .args(["evidence", "trace", "--backfill-uuids"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(&sys_path).expect("read sys.toml"),
        backfilled,
        "second backfill must not modify an already-valid trace file"
    );
}
