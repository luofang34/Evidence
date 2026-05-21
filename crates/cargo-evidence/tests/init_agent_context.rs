//! Integration tests for `cargo evidence init --with-agent-context`
//! (TEST-097, governing LLR-090). Spawns the binary against fresh
//! tempdirs and asserts the agent-context scaffold (root
//! `CLAUDE.md` + `.claude/settings.json`) appears, opts out
//! cleanly, preserves existing files, and shows up in the
//! `--json`/`--format=jsonl` written-files stream.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::Path;

use assert_cmd::Command as AssertCommand;
use tempfile::TempDir;

fn cargo_evidence(cwd: &Path) -> AssertCommand {
    #[allow(deprecated)]
    let mut cmd = AssertCommand::cargo_bin("cargo-evidence").unwrap();
    cmd.current_dir(cwd);
    cmd
}

/// Happy path: fresh tempdir + `init` (default-on agent-context)
/// writes both root `CLAUDE.md` and `.claude/settings.json`. The
/// `CLAUDE.md` carries the pointer to `evidence_context` /
/// `cargo evidence context`; `settings.json` parses as JSON and
/// registers `evidence-mcp` + the `evidence/**` deny rule. Pins
/// the default-on contract: a bare `init` produces the scaffold
/// without the user needing to pass `--with-agent-context`.
#[test]
fn init_with_agent_context_writes_root_files() {
    let tmp = TempDir::new().expect("tempdir");

    cargo_evidence(tmp.path())
        .args(["evidence", "init"])
        .assert()
        .success();

    let claude_md = tmp.path().join("CLAUDE.md");
    assert!(
        claude_md.exists(),
        "expected {} to be written",
        claude_md.display()
    );
    let body = fs::read_to_string(&claude_md).expect("read CLAUDE.md");
    assert!(
        body.contains("agent context"),
        "expected starter title in CLAUDE.md:\n{body}"
    );
    assert!(
        body.contains("evidence_context"),
        "expected MCP-tool pointer in CLAUDE.md:\n{body}"
    );
    assert!(
        body.contains("cargo evidence context"),
        "expected CLI-verb pointer in CLAUDE.md:\n{body}"
    );
    assert!(
        body.contains("Per-crate conventions"),
        "expected per-crate guidance section in CLAUDE.md:\n{body}"
    );
    assert!(
        body.lines().count() <= 30,
        "starter CLAUDE.md must stay lean (<= 30 lines); got {}",
        body.lines().count()
    );

    let settings = tmp.path().join(".claude").join("settings.json");
    assert!(
        settings.exists(),
        "expected {} to be written",
        settings.display()
    );
    let settings_body = fs::read_to_string(&settings).expect("read settings.json");
    let parsed: serde_json::Value =
        serde_json::from_str(&settings_body).expect("settings.json must be valid JSON");
    assert_eq!(
        parsed["mcpServers"]["evidence-mcp"]["command"],
        "evidence-mcp"
    );
    assert!(
        parsed["permissions"]["deny"]
            .as_array()
            .expect("deny is an array")
            .iter()
            .any(|s| s == "evidence/**"),
        "expected evidence/** in permissions.deny:\n{settings_body}"
    );
}

/// Opt-out: `init --no-agent-context` writes the `cert/` tree but
/// skips the agent-context scaffold entirely. Neither root
/// `CLAUDE.md` nor `.claude/settings.json` appears.
#[test]
fn init_no_agent_context_skips_scaffold() {
    let tmp = TempDir::new().expect("tempdir");

    cargo_evidence(tmp.path())
        .args(["evidence", "init", "--no-agent-context"])
        .assert()
        .success();

    // The cert tree is still written.
    assert!(
        tmp.path().join("cert").join("boundary.toml").exists(),
        "init without agent-context must still write cert/"
    );
    // But the scaffold is absent.
    assert!(
        !tmp.path().join("CLAUDE.md").exists(),
        "--no-agent-context must skip root CLAUDE.md"
    );
    assert!(
        !tmp.path().join(".claude").exists(),
        "--no-agent-context must skip .claude/"
    );
}

/// Idempotency: a pre-existing `CLAUDE.md` is preserved verbatim,
/// and a pre-existing `.claude/settings.json` is also preserved.
/// `init` never clobbers downstream-authored conventions.
#[test]
fn init_with_agent_context_preserves_existing_files() {
    let tmp = TempDir::new().expect("tempdir");

    let claude_md = tmp.path().join("CLAUDE.md");
    let original_claude = "# Hand-written\n\nDownstream-owned conventions.\n";
    fs::write(&claude_md, original_claude).expect("write CLAUDE.md");

    let dot_claude = tmp.path().join(".claude");
    fs::create_dir(&dot_claude).expect("mkdir .claude");
    let settings = dot_claude.join("settings.json");
    let original_settings = r#"{"hand":"written"}"#;
    fs::write(&settings, original_settings).expect("write settings.json");

    cargo_evidence(tmp.path())
        .args(["evidence", "init", "--with-agent-context"])
        .assert()
        .success();

    let claude_after = fs::read_to_string(&claude_md).expect("read CLAUDE.md");
    assert_eq!(
        claude_after, original_claude,
        "existing CLAUDE.md must be left untouched"
    );

    let settings_after = fs::read_to_string(&settings).expect("read settings.json");
    assert_eq!(
        settings_after, original_settings,
        "existing .claude/settings.json must be left untouched"
    );
}

/// JSON-format output: every emitted file shows up as an
/// `INIT_TEMPLATE_WRITTEN` line in the JSONL stream, including the
/// two new agent-context files. The terminal line is `INIT_OK`.
#[test]
fn init_with_agent_context_jsonl_lists_new_files() {
    let tmp = TempDir::new().expect("tempdir");

    let out = cargo_evidence(tmp.path())
        .args(["evidence", "--format=jsonl", "init", "--with-agent-context"])
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "init --with-agent-context --format=jsonl must exit 0; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let written_paths: Vec<String> = stdout
        .lines()
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            if v["code"] != "INIT_TEMPLATE_WRITTEN" {
                return None;
            }
            v["location"]["file"].as_str().map(|s| s.to_string())
        })
        .collect();

    let has_claude = written_paths.iter().any(|p| p.ends_with("CLAUDE.md"));
    let has_settings = written_paths
        .iter()
        .any(|p| p.replace('\\', "/").ends_with(".claude/settings.json"));
    assert!(
        has_claude,
        "CLAUDE.md must appear in JSONL written-files; got:\n{stdout}"
    );
    assert!(
        has_settings,
        ".claude/settings.json must appear in JSONL written-files; got:\n{stdout}"
    );

    let last_nonempty = stdout
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .expect("at least one stdout line");
    let terminal: serde_json::Value =
        serde_json::from_str(last_nonempty).expect("terminal line must be JSON");
    assert_eq!(terminal["code"], "INIT_OK");
}

/// `--with-agent-context` and `--no-agent-context` are mutually
/// exclusive (clap `conflicts_with`). Passing both fires the CLI
/// argument-error path; exit code is non-zero.
#[test]
fn init_rejects_both_flags() {
    let tmp = TempDir::new().expect("tempdir");

    let out = cargo_evidence(tmp.path())
        .args([
            "evidence",
            "init",
            "--with-agent-context",
            "--no-agent-context",
        ])
        .output()
        .expect("spawn");
    assert!(
        !out.status.success(),
        "passing both --with-agent-context and --no-agent-context must fail; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
