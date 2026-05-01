//! Trace-root discovery: pick the project's trace location when the
//! caller hasn't passed `--trace-roots` explicitly.
//!
//! Single source of truth for every consumer — `cargo evidence trace
//! --validate`, `cargo evidence check`, `cargo evidence floors`, and
//! the corresponding `evidence_core::floors::count_trace_per_layer`.
//! All routes through one function so a downstream project's
//! observable trace count is identical across verbs.
//!
//! Discovery order (first existing wins), all resolved relative to
//! `workspace_root`:
//!
//! 1. `<workspace_root>/cert/trace/` — the canonical project layout
//!    (lives alongside `cert/boundary.toml`, `cert/floors.toml`).
//! 2. Fall back to
//!    `load_trace_roots(<workspace_root>/cert/boundary.toml)` which
//!    reads the `scope.trace_roots` array for projects whose layout
//!    cannot follow the convention.
//!
//! Explicit `--trace-roots` always wins and never reaches this
//! function — the CLI's `cmd_*` paths short-circuit on the flag
//! before consulting discovery.

use std::path::{Path, PathBuf};

use crate::policy::load_trace_roots;

/// Resolve trace roots when the caller has no explicit override.
/// See module docs for discovery semantics.
pub fn default_trace_roots(workspace_root: &Path) -> Vec<String> {
    let is_cwd = workspace_root == Path::new(".") || workspace_root == Path::new("");
    let candidate = if is_cwd {
        PathBuf::from("cert/trace")
    } else {
        workspace_root.join("cert/trace")
    };
    if candidate.is_dir() {
        tracing::info!(
            "trace: auto-discovered trace root '{}' (no --trace-roots given)",
            candidate.display()
        );
        return vec![candidate.to_string_lossy().into_owned()];
    }
    let boundary_path = if is_cwd {
        PathBuf::from("cert/boundary.toml")
    } else {
        workspace_root.join("cert/boundary.toml")
    };
    tracing::info!(
        "trace: cert/trace/ not found; falling back to {}",
        boundary_path.display()
    );
    let raw = load_trace_roots(&boundary_path);
    raw.into_iter()
        .map(|s| {
            let p = Path::new(&s);
            if p.is_absolute() || is_cwd {
                s
            } else {
                workspace_root.join(p).to_string_lossy().into_owned()
            }
        })
        .collect()
}
