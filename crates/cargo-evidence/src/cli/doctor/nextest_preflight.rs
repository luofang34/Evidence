//! `check_nextest` — verify `cargo nextest` is installed (LLR-048).
//!
//! Test-execution identity capture (`generate`'s
//! `run_tests_and_capture`) shells out to `cargo nextest run
//! --message-format libtest-json-plus`. A cert/record bundle whose
//! tests could not run under nextest omits per-test execution
//! evidence, so cert-profile `generate` — which runs
//! `precheck_doctor` — refuses before doing any work when the binary
//! is absent, turning a phase-5 spawn failure into an up-front
//! diagnostic. Standalone `doctor` surfaces the same gap as a
//! warning.
//!
//! Split into its own sibling module to keep `checks.rs` under the
//! 500-line workspace limit.

use std::process::Command;

use super::CheckResult;

const FIX_HINT: &str = "cargo-nextest is required to capture per-test execution \
     identity. Install it with `cargo install cargo-nextest`.";

/// Probe `cargo nextest --version`. Pass if it runs and exits zero;
/// otherwise `DOCTOR_NEXTEST_MISSING` (a warning that becomes a
/// cert/record `generate` blocker via `precheck_doctor`).
pub(super) fn check_nextest() -> CheckResult {
    check_nextest_via("cargo")
}

/// Inner probe parameterized on the launcher program so the failure
/// path is testable without uninstalling nextest: a bogus program
/// name always spawns-errors.
fn check_nextest_via(program: &str) -> CheckResult {
    match Command::new(program)
        .args(["nextest", "--version"])
        .output()
    {
        Ok(out) if out.status.success() => CheckResult::Pass,
        Ok(out) => CheckResult::Fail(
            "DOCTOR_NEXTEST_MISSING",
            format!(
                "`{program} nextest --version` exited non-zero (code {:?}) — {FIX_HINT}",
                out.status.code()
            ),
        ),
        Err(e) => CheckResult::Fail(
            "DOCTOR_NEXTEST_MISSING",
            format!("`{program} nextest` could not be run ({e}) — {FIX_HINT}"),
        ),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::cli::doctor::CheckResult;

    /// A launcher program that does not exist spawns-errors, which the
    /// probe reports as `DOCTOR_NEXTEST_MISSING`. Deterministic — does
    /// not depend on whether nextest is installed on the host.
    #[test]
    fn absent_launcher_reports_nextest_missing() {
        let result = check_nextest_via("definitely-not-a-real-launcher-binary-xyz");
        match result {
            CheckResult::Fail(code, msg) => {
                assert_eq!(code, "DOCTOR_NEXTEST_MISSING");
                assert!(
                    msg.contains("cargo install cargo-nextest"),
                    "message should carry the install hint, got: {msg}"
                );
            }
            CheckResult::Pass => panic!("a nonexistent launcher must not pass"),
        }
    }
}
