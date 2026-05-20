//! Agent-context scaffolding emitted by `cargo evidence init`.
//!
//! When `--with-agent-context` (the default) is on, `cmd_init`
//! writes a starter root `CLAUDE.md` and `.claude/settings.json`
//! pointing the agent harness at `evidence-mcp`. Existing files
//! are preserved unconditionally — the user owns those files once
//! they exist. See HLR-075 / LLR-090.
//!
//! Lives in its own submodule so the parent `init.rs` stays under
//! the workspace 500-line file cap once trace template literals
//! and the agent-context emitter coexist.

use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::cli::init::emit_template_written;

/// Emit the agent-context scaffold under `root`. Returns the
/// number of files actually written (zero if both already exist).
/// Never overwrites: pre-existing `CLAUDE.md` or
/// `.claude/settings.json` are preserved unconditionally — the
/// user owns those files once they exist. When
/// `.claude/settings.json` is present, a single info line is
/// logged on stderr explaining what the user would merge in by
/// hand; the rest of the scaffold continues regardless.
pub fn write_agent_context_files(root: &Path, jsonl: bool) -> Result<u64> {
    let mut written = 0u64;

    let claude_md = root.join("CLAUDE.md");
    if !claude_md.exists() {
        let project_name = detect_project_name(root);
        fs::write(&claude_md, render_root_claude_md(&project_name))?;
        emit_template_written(jsonl, &claude_md)?;
        written += 1;
    }

    let dot_claude = root.join(".claude");
    let settings_path = dot_claude.join("settings.json");
    if !settings_path.exists() {
        fs::create_dir_all(&dot_claude)?;
        fs::write(&settings_path, AGENT_SETTINGS_JSON)?;
        emit_template_written(jsonl, &settings_path)?;
        written += 1;
    } else {
        // Stderr-only advisory: the user already has a settings
        // file. We do not touch it; we tell them what merging in
        // by hand would mean so the upgrade path is discoverable
        // without surprising them.
        eprintln!(
            "info: {} already exists; leaving it untouched. To wire up the \
             agent-context surface, merge an entry for `evidence-mcp` into \
             `mcpServers` and a `evidence/**` rule into `permissions.deny`.",
            settings_path.display()
        );
    }

    Ok(written)
}

/// Project-name heuristic for the starter `CLAUDE.md` title. Uses
/// the canonicalized basename of `root`; falls back to `"project"`
/// if the path is empty (e.g. `.` at a filesystem root) or
/// non-UTF-8. The value lands in a single Markdown title line —
/// not a load-bearing identifier — so a sensible default is fine.
fn detect_project_name(root: &Path) -> String {
    let absolute = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    absolute
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "project".to_string())
}

/// Build the starter root `CLAUDE.md` body. Kept lean by design
/// (≤30 lines): one title, one project-description placeholder,
/// one section pointing agents at the queryable per-module
/// context surface, one section instructing future humans to add
/// per-crate `CLAUDE.md` files. No project rules — those belong
/// to the downstream user, not this scaffold.
fn render_root_claude_md(project_name: &str) -> String {
    format!(
        r#"# {project_name} — agent context

<!-- Replace this line with a one-paragraph description of the project. -->

## Module-level context for agents

For per-module trace + boundary + floors context on any source file,
call `evidence_context` (MCP) or `cargo evidence context <path>`.
Don't grep `cert/trace/*.toml` manually — the query returns the
requirements governing the file, their parents, the tests that
verify them, the diagnostic codes the module owns, and the floors
it must respect. See `cert/trace/` for the underlying data.

## Per-crate conventions

Add a `crates/<x>/CLAUDE.md` per workspace crate carrying local
conventions and the scoped test command for that crate (e.g.
`cargo test -p <x>`). Keep each file focused on what is *local*
to that crate — workspace-wide rules belong here in the root.
"#
    )
}

/// Starter `.claude/settings.json`. Registers `evidence-mcp` as
/// an MCP server (the binary must be on PATH for the agent
/// harness to spawn it) and denies writes under `evidence/`, the
/// default bundle output dir, so a careless edit can't dirty
/// generated artifacts mid-session.
const AGENT_SETTINGS_JSON: &str = r#"{
  "mcpServers": {
    "evidence-mcp": {
      "command": "evidence-mcp",
      "args": []
    }
  },
  "permissions": {
    "deny": [
      "evidence/**"
    ]
  }
}
"#;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;

    #[test]
    fn detect_project_name_uses_dir_basename() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let sub = tmp.path().join("MyProject");
        fs::create_dir(&sub).expect("mkdir");
        assert_eq!(detect_project_name(&sub), "MyProject");
    }

    #[test]
    fn render_root_claude_md_under_thirty_lines() {
        let body = render_root_claude_md("demo");
        let lines = body.lines().count();
        assert!(
            lines <= 30,
            "starter CLAUDE.md must stay lean (<= 30 lines); got {lines}"
        );
        assert!(body.contains("evidence_context"));
        assert!(body.contains("cargo evidence context"));
        assert!(body.contains("crates/<x>/CLAUDE.md"));
    }

    #[test]
    fn settings_json_registers_evidence_mcp_and_deny_rule() {
        let v: serde_json::Value =
            serde_json::from_str(AGENT_SETTINGS_JSON).expect("settings.json must be valid JSON");
        assert_eq!(v["mcpServers"]["evidence-mcp"]["command"], "evidence-mcp");
        assert!(
            v["permissions"]["deny"]
                .as_array()
                .expect("deny array")
                .iter()
                .any(|s| s == "evidence/**"),
            "settings.json must deny `evidence/**`"
        );
    }
}
