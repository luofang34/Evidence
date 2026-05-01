//! End-to-end check that `cargo-evidence --help` (direct binary
//! invocation, not `cargo evidence --help`) lists every subcommand.
//!
//! The intercept in `main.rs` reuses clap's render tree on
//! `EvidenceArgs`, so a new subcommand declared via `#[derive(
//! Subcommand)]` appears automatically. The hand-curated list in
//! `EXPECTED_SUBCOMMANDS` is the drift guard: adding a subcommand
//! without updating this list fails this test, prompting the
//! author to deliberately confirm the help surface.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use assert_cmd::Command;

/// Every user-facing subcommand declared in
/// `cli::args::Commands`. Update this when adding a new variant.
const EXPECTED_SUBCOMMANDS: &[&str] = &[
    "generate", "verify", "check", "diff", "doctor", "init", "schema", "trace", "rules", "floors",
];

fn cargo_evidence_bin() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

#[test]
fn test_cargo_evidence_help_lists_subcommands() {
    let output = cargo_evidence_bin().arg("--help").output().unwrap();
    assert!(
        output.status.success(),
        "cargo-evidence --help exited non-zero: {:?}",
        output.status
    );
    let stdout = String::from_utf8(output.stdout).expect("help is utf-8");

    let banner_present = stdout.contains("DO-178C / DO-330 evidence bundles");
    assert!(
        banner_present,
        "expected banner naming the tool's purpose; got:\n{}",
        stdout
    );

    let dual_form = stdout.contains("cargo evidence") && stdout.contains("cargo-evidence");
    assert!(
        dual_form,
        "expected both invocation forms named in the help banner; got:\n{}",
        stdout
    );

    for sub in EXPECTED_SUBCOMMANDS {
        assert!(
            stdout.contains(sub),
            "expected subcommand `{}` to appear in `--help` output, but it was missing. \
             Either the subcommand was renamed (update EXPECTED_SUBCOMMANDS) or the \
             intercept in `main.rs` regressed.\n\nFull stdout:\n{}",
            sub,
            stdout
        );
    }
}

#[test]
fn test_cargo_evidence_dash_h_exits_zero() {
    let output = cargo_evidence_bin().arg("-h").output().unwrap();
    assert!(
        output.status.success(),
        "cargo-evidence -h exited non-zero: {:?}",
        output.status
    );
    assert!(
        !output.stdout.is_empty(),
        "cargo-evidence -h produced empty stdout"
    );
}
