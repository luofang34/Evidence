//! Integration tests for `cargo evidence context [<selector>] [--json|--format=jsonl]`.
//!
//! Pins three properties:
//!
//! 1. **Wire shape stability** — `context --json` against a known
//!    selector is byte-diffed against
//!    `tests/fixtures/golden_context.json`. The fixture is regenerated
//!    via `tools/regen-golden-fixtures.sh`; the byte-diff catches any
//!    accidental rename or reorder of report fields.
//! 2. **Graceful non-adopter path** — `context --format=jsonl` against
//!    an empty tempdir (no `cert/trace/`) emits
//!    `CONTEXT_NO_TRACE_CONFIGURED` (info) + `CONTEXT_OK` and exits 0.
//! 3. **Invalid selector path** — a typo'd selector emits
//!    `CONTEXT_SELECTOR_OUT_OF_SCOPE` + the `CONTEXT_FAIL` terminal
//!    and exits 2.
//!
//! Selector chosen for the golden run: the crate `cargo-evidence`.
//! Per-file selectors would tie the fixture to a single source path
//! that any future refactor breaks; per-module selectors don't
//! exercise the per-crate floor slice. The crate selector exercises
//! every report field including floors, boundary, requirements,
//! parents, tests, and diagnostic codes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

const GOLDEN_CONTEXT: &[u8] = include_bytes!("fixtures/golden_context.json");

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

/// Byte-diff `cargo evidence context --crate cargo-evidence --json`
/// against the committed fixture. Any field rename, order change, or
/// dropped row fires this with a line-numbered diff. Regenerate
/// intentionally via `tools/regen-golden-fixtures.sh`.
#[test]
fn golden_context_json_byte_diff() {
    let out = cargo_evidence()
        .current_dir(workspace_root())
        .args(["evidence", "context", "--crate", "cargo-evidence", "--json"])
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "context --crate cargo-evidence --json must exit 0; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    if out.stdout != GOLDEN_CONTEXT {
        let current = String::from_utf8_lossy(&out.stdout);
        let golden = String::from_utf8_lossy(GOLDEN_CONTEXT);
        let mut diverge_line: Option<(usize, String, String)> = None;
        for (idx, (a, b)) in current.lines().zip(golden.lines()).enumerate() {
            if a != b {
                diverge_line = Some((idx + 1, a.to_string(), b.to_string()));
                break;
            }
        }
        match diverge_line {
            Some((lineno, current_line, golden_line)) => panic!(
                "context --json diverged from golden at line {}:\n  \
                 current: {}\n  golden:  {}\n\n\
                 Regenerate with `tools/regen-golden-fixtures.sh` if the change is intentional.",
                lineno, current_line, golden_line
            ),
            None => panic!(
                "context --json length diverged from golden (current {} bytes, golden {} bytes). \
                 Regenerate with `tools/regen-golden-fixtures.sh` if the change is intentional.",
                out.stdout.len(),
                GOLDEN_CONTEXT.len()
            ),
        }
    }
}

/// The non-adopter graceful path: an empty tempdir (no `cert/trace/`)
/// must emit the info-level `CONTEXT_NO_TRACE_CONFIGURED` diagnostic
/// followed by the `CONTEXT_OK` terminal and exit 0. Mirrors the floors
/// path: downstream projects without a trace setup get a clean run.
#[test]
fn context_jsonl_non_adopter_graceful_path() {
    let tmp = TempDir::new().expect("tempdir");
    let out = cargo_evidence()
        .current_dir(tmp.path())
        .args(["evidence", "--format=jsonl", "context"])
        .output()
        .expect("spawn");
    assert_eq!(
        out.status.code(),
        Some(0),
        "non-adopter path must exit 0; stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("valid utf-8");
    let lines: Vec<Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each line is JSON"))
        .collect();
    assert!(
        lines.len() >= 2,
        "expected at least the info diagnostic + terminal, got {} lines: {:?}",
        lines.len(),
        lines
    );
    let info = lines
        .iter()
        .find(|v| v["code"] == "CONTEXT_NO_TRACE_CONFIGURED")
        .expect("CONTEXT_NO_TRACE_CONFIGURED diagnostic must be present");
    assert_eq!(info["severity"], "info");
    assert_eq!(info["subcommand"], "context");
    let terminal = lines.last().expect("last line is terminal");
    assert_eq!(terminal["code"], "CONTEXT_OK");
    assert_eq!(terminal["severity"], "info");
}

/// A selector that doesn't resolve to any file / crate / module
/// surfaces `CONTEXT_SELECTOR_OUT_OF_SCOPE` (error) followed by the
/// `CONTEXT_FAIL` terminal and exits 2.
#[test]
fn context_jsonl_invalid_selector_emits_fail_terminal() {
    let out = cargo_evidence()
        .current_dir(workspace_root())
        .args([
            "evidence",
            "--format=jsonl",
            "context",
            "completely-bogus-selector",
        ])
        .output()
        .expect("spawn");
    assert_eq!(
        out.status.code(),
        Some(2),
        "invalid selector must exit 2; stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("valid utf-8");
    let lines: Vec<Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each line is JSON"))
        .collect();
    assert!(
        lines.len() >= 2,
        "expected at least the error diagnostic + terminal, got {} lines",
        lines.len()
    );
    let first = &lines[0];
    assert_eq!(first["code"], "CONTEXT_SELECTOR_OUT_OF_SCOPE");
    assert_eq!(first["severity"], "error");
    let terminal = lines.last().expect("last line is terminal");
    assert_eq!(terminal["code"], "CONTEXT_FAIL");
    assert_eq!(terminal["subcommand"], "context");
}

/// Human-mode invocation against the workspace exits 0 and prints a
/// header line — smoke test that catches a panic in the human
/// renderer.
#[test]
fn context_human_mode_workspace_overview_exits_zero() {
    let out = cargo_evidence()
        .current_dir(workspace_root())
        .args(["evidence", "context"])
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "context (workspace, human) must exit 0; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("valid utf-8");
    assert!(stdout.contains("selector:"), "missing 'selector:' header");
    assert!(stdout.contains("crate:"), "missing 'crate:' header");
    assert!(stdout.contains("dal:"), "missing 'dal:' header");
}
