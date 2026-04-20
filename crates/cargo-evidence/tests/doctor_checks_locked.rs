//! Lock tests for the `cargo evidence doctor` check set + wiring.
//!
//! Two gates:
//!
//! - `cmd_doctor_invokes_every_required_check`: the fixed-order
//!   check list in `cli/doctor.rs::CHECKS` must contain every
//!   name in [`REQUIRED_CHECK_NAMES`], and `run_named_check`
//!   must have a matching dispatch arm for each. A future PR
//!   that silently drops a check (e.g. deletes the merge-style
//!   branch) fires this with a file:line reference.
//! - `cmd_generate_calls_doctor_precheck_for_cert_modes`:
//!   `cli/generate.rs` must contain `doctor::precheck_doctor`
//!   guarded by `Profile::Cert | Profile::Record`. Removing
//!   either piece silently disables the cert-profile rigor gate.
//!
//! Pattern mirrors `walker_usage_locked` / `rot_prone_markers_locked`:
//! source-text grep with a named `REQUIRED_*` const. No
//! `Diagnostic` wire shape — the assertion message is the diagnostic.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Every check name that MUST appear in `cli/doctor.rs::CHECKS` +
/// `run_named_check`. Adding a check is fine; removing one
/// requires an explicit edit here + a written justification in
/// the commit message (same pattern as the `ALLOWED_READ_DIR_FILES`
/// allowlist).
const REQUIRED_CHECK_NAMES: &[&str] = &[
    "trace validity",
    "floors config",
    "boundary config",
    "CI integration",
    "merge-style policy",
    "override protocol docs",
];

#[test]
fn cmd_doctor_invokes_every_required_check() {
    let src_path = workspace_root()
        .join("crates")
        .join("cargo-evidence")
        .join("src")
        .join("cli")
        .join("doctor.rs");
    let src = std::fs::read_to_string(&src_path)
        .unwrap_or_else(|e| panic!("reading {}: {}", src_path.display(), e));

    for check in REQUIRED_CHECK_NAMES {
        // The `CHECKS` const carries a quoted name; the `match` arm
        // in `run_named_check` carries the same quoted name. A missing
        // check in either place fires this assertion.
        let quoted = format!("\"{}\"", check);
        let occurrences = src.matches(&quoted).count();
        assert!(
            occurrences >= 2,
            "doctor.rs must reference `{}` in both CHECKS and \
             run_named_check (found {} occurrences of `{}`). \
             Removing a check requires editing REQUIRED_CHECK_NAMES \
             in tests/doctor_checks_locked.rs with written justification \
             — silent deletion would let a cert-profile gate lapse \
             unnoticed.",
            check,
            occurrences,
            quoted
        );
    }
}

#[test]
fn every_doctor_code_emitted_in_source() {
    // Meta-bijection: every `DOCTOR_*` code registered in
    // `evidence_core::HAND_EMITTED_CLI_CODES` must appear as a literal
    // string in the doctor source tree (`cli/doctor.rs` or
    // `cli/doctor/checks.rs`). Prevents the pseudo-code
    // anti-pattern the trace-schema hardening work closed:
    // a code declared in RULES but never actually emitted silently
    // expands the manifest without improving observable behavior.
    //
    // Scope is the doctor subtree because DOCTOR_* codes are
    // hand-emitted exclusively from there. Terminal codes
    // (DOCTOR_OK / DOCTOR_FAIL) live in TERMINAL_CODES, not in
    // HAND_EMITTED_CLI_CODES, so they're naturally out of scope.
    let doctor_rs = std::fs::read_to_string(
        workspace_root()
            .join("crates")
            .join("cargo-evidence")
            .join("src")
            .join("cli")
            .join("doctor.rs"),
    )
    .expect("read doctor.rs");
    let checks_rs = std::fs::read_to_string(
        workspace_root()
            .join("crates")
            .join("cargo-evidence")
            .join("src")
            .join("cli")
            .join("doctor")
            .join("checks.rs"),
    )
    .expect("read doctor/checks.rs");
    let haystack = format!("{}\n{}", doctor_rs, checks_rs);

    let doctor_codes: Vec<&'static str> = evidence_core::HAND_EMITTED_CLI_CODES
        .iter()
        .copied()
        .filter(|c| c.starts_with("DOCTOR_"))
        .collect();
    let mut missing: Vec<&'static str> = Vec::new();
    for code in &doctor_codes {
        let quoted = format!("\"{}\"", code);
        if !haystack.contains(&quoted) {
            missing.push(code);
        }
    }
    assert!(
        missing.is_empty(),
        "the following DOCTOR_* code(s) are in RULES + HAND_EMITTED_CLI_CODES \
         but never emitted in doctor source (pseudo-code anti-pattern): {:?}. \
         Either emit them or remove from the registry.",
        missing
    );
}

#[test]
fn cmd_generate_calls_doctor_precheck_for_cert_modes() {
    let src_path = workspace_root()
        .join("crates")
        .join("cargo-evidence")
        .join("src")
        .join("cli")
        .join("generate.rs");
    let src = std::fs::read_to_string(&src_path)
        .unwrap_or_else(|e| panic!("reading {}: {}", src_path.display(), e));

    assert!(
        src.contains("doctor::precheck_doctor"),
        "generate.rs must call `doctor::precheck_doctor` on cert/record \
         profiles; removing this line silently disables downstream rigor \
         enforcement. Produced bundles would claim cert-profile status \
         without passing the audit."
    );

    assert!(
        src.contains("Profile::Cert | Profile::Record"),
        "generate.rs must gate the doctor precheck on \
         `Profile::Cert | Profile::Record`. Running the precheck on \
         dev-profile would slow down the iteration hot path; skipping \
         it on cert-profile would lose the gate."
    );
}
