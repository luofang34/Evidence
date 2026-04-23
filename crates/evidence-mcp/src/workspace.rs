//! Workspace-path resolution + the synthetic
//! `MCP_WORKSPACE_FALLBACK` Warning diagnostic that surfaces when
//! an agent omits `workspace_path`. Split out of `lib.rs` to
//! keep the facade under the 80-line target and to group
//! agent-trust concerns (how an MCP call's working directory was
//! chosen) in one module.

use std::path::PathBuf;

/// Classification of how a tool call's working directory was
/// chosen, returned alongside the resolved path by
/// [`resolve_workspace`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkspaceResolution {
    /// The request supplied an explicit `workspace_path`. No
    /// agent-facing signal is needed.
    Given,
    /// The request omitted `workspace_path`; the handler fell
    /// back to the server's CWD. Callers that produce a
    /// user-visible diagnostic stream prepend a
    /// `MCP_WORKSPACE_FALLBACK` Warning so an agent typo of
    /// "omitted" vs an intentional no-argument call is
    /// distinguishable from the response (HLR-054 / LLR-054).
    Fallback,
}

/// Resolve an optional workspace-path request field against the
/// server's CWD. Absolute paths pass through; relative paths are
/// joined onto `current_dir`. The resolved path is returned
/// without canonicalization — the CLI itself handles existence
/// checks and emits structured errors on missing paths. The
/// [`WorkspaceResolution`] return value distinguishes an
/// explicit path from a CWD fallback so the caller can emit
/// `MCP_WORKSPACE_FALLBACK` on the `Fallback` arm.
pub(crate) fn resolve_workspace(
    path: Option<&str>,
) -> Result<(PathBuf, WorkspaceResolution), String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cannot resolve server CWD: {e}"))?;
    match path {
        None => Ok((cwd, WorkspaceResolution::Fallback)),
        Some(p) => {
            let requested = PathBuf::from(p);
            let resolved = if requested.is_absolute() {
                requested
            } else {
                cwd.join(requested)
            };
            Ok((resolved, WorkspaceResolution::Given))
        }
    }
}

/// Build a synthetic `MCP_WORKSPACE_FALLBACK` Warning diagnostic
/// shaped like the JSONL entries the CLI emits, so agents can
/// pattern-match on `.code` uniformly across both MCP-layer and
/// CLI-layer diagnostics. `cwd` is embedded in the message so
/// the agent sees which directory actually ran — turning a
/// silent fallback into an observable contract (HLR-054).
pub(crate) fn workspace_fallback_diagnostic(cwd: &std::path::Path) -> serde_json::Value {
    serde_json::json!({
        "code": "MCP_WORKSPACE_FALLBACK",
        "severity": "warning",
        "message": format!(
            "workspace_path omitted; using MCP server CWD {:?}. \
             Pass an explicit workspace_path to silence this warning.",
            cwd
        ),
        "subcommand": "mcp",
    })
}
