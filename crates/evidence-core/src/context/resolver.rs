//! Selector classification — turn the caller's raw string into a
//! [`ResolvedSelector`].
//!
//! Priority on ambiguity: File > Crate > Module. When the same input
//! matches more than one kind, the higher-priority match wins and
//! the resolver records the alternates so the lookup phase can attach
//! a `CONTEXT_AMBIGUOUS_SELECTOR` warning naming what was skipped.
//!
//! Workspace-overview path: `raw = None` short-circuits to
//! [`ResolvedSelector::Workspace`] without touching the filesystem.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use walkdir::WalkDir;

use super::error::ContextError;

/// Resolved selector — what the resolver decided the caller meant.
///
/// `ambiguities` is the list of *other* kinds that also matched the
/// input. The chosen variant always wins; the ambiguous-warning
/// surface uses `ambiguities` to name the alternates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedSelector {
    /// No selector supplied — return the workspace overview.
    Workspace,
    /// File path under `crates/<crate>/...`. Carried as workspace-
    /// relative with forward slashes; never absolute.
    File {
        /// Raw input the caller passed.
        raw: String,
        /// Workspace-relative path (e.g.
        /// `crates/evidence-core/src/trace.rs`).
        path: String,
        /// Crate the file lives in.
        crate_name: String,
        /// Other selector kinds the same input matched.
        ambiguities: Vec<String>,
    },
    /// Workspace crate name (matches `[package].name` in
    /// `crates/*/Cargo.toml`).
    Crate {
        /// Raw input the caller passed.
        raw: String,
        /// Resolved `[package].name`.
        crate_name: String,
        /// Other selector kinds the same input matched.
        ambiguities: Vec<String>,
    },
    /// Rust module path (`evidence_core::trace`). Matched as a prefix
    /// against any LLR's `modules` field — the actual lookup happens
    /// in the lookup phase, so we only carry the dotted string here.
    Module {
        /// Raw input the caller passed.
        raw: String,
        /// Module path (already normalized to `::`-separated form).
        path: String,
        /// Other selector kinds the same input matched.
        ambiguities: Vec<String>,
    },
}

impl ResolvedSelector {
    /// Stable kind label used in the `selector.kind` JSON field.
    pub fn kind(&self) -> &'static str {
        match self {
            ResolvedSelector::Workspace => "workspace",
            ResolvedSelector::File { .. } => "file",
            ResolvedSelector::Crate { .. } => "crate",
            ResolvedSelector::Module { .. } => "module",
        }
    }

    /// Other kinds the same input matched; informational only.
    pub fn ambiguities(&self) -> &[String] {
        match self {
            ResolvedSelector::Workspace => &[],
            ResolvedSelector::File { ambiguities, .. }
            | ResolvedSelector::Crate { ambiguities, .. }
            | ResolvedSelector::Module { ambiguities, .. } => ambiguities,
        }
    }
}

/// One workspace crate as found under `crates/*/`.
#[derive(Debug, Clone)]
pub struct WorkspaceCrate {
    /// `[package].name` from the manifest.
    pub name: String,
    /// Workspace-relative directory (`crates/<name>` form). Forward
    /// slashes only.
    pub dir: String,
}

/// Discover every workspace crate by walking `crates/*/Cargo.toml`.
/// Empty map if the `crates/` directory is missing — downstream
/// projects that haven't adopted the layout pattern see no crates.
pub fn discover_workspace_crates(
    workspace_root: &Path,
) -> Result<BTreeMap<String, WorkspaceCrate>, ContextError> {
    let crates_dir = workspace_root.join("crates");
    let mut out: BTreeMap<String, WorkspaceCrate> = BTreeMap::new();
    if !crates_dir.is_dir() {
        return Ok(out);
    }
    let entries = WalkDir::new(&crates_dir)
        .follow_links(false)
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_dir());
    for entry in entries {
        let dir_path = entry.into_path();
        let manifest_path = dir_path.join("Cargo.toml");
        if !manifest_path.is_file() {
            continue;
        }
        let name = read_package_name(&manifest_path)?;
        let dir_rel = workspace_relative(workspace_root, &dir_path);
        out.insert(name.clone(), WorkspaceCrate { name, dir: dir_rel });
    }
    Ok(out)
}

/// Classify `raw` (or `None` for the workspace overview) into a
/// [`ResolvedSelector`]. Priority on ambiguity: File > Crate >
/// Module.
pub fn resolve_selector(
    workspace_root: &Path,
    raw: Option<&str>,
) -> Result<ResolvedSelector, ContextError> {
    let Some(input) = raw else {
        return Ok(ResolvedSelector::Workspace);
    };
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(ResolvedSelector::Workspace);
    }

    let crates = discover_workspace_crates(workspace_root)?;
    let mut ambiguities: Vec<String> = Vec::new();

    let file_match = match_file(workspace_root, trimmed, &crates);
    let crate_match = match_crate(trimmed, &crates);
    let module_match = match_module(trimmed);

    if let Some((rel_path, crate_name)) = file_match {
        if crate_match.is_some() {
            ambiguities.push("crate".to_string());
        }
        if module_match.is_some() {
            ambiguities.push("module".to_string());
        }
        return Ok(ResolvedSelector::File {
            raw: input.to_string(),
            path: rel_path,
            crate_name,
            ambiguities,
        });
    }
    if let Some(crate_name) = crate_match {
        if module_match.is_some() {
            ambiguities.push("module".to_string());
        }
        return Ok(ResolvedSelector::Crate {
            raw: input.to_string(),
            crate_name,
            ambiguities,
        });
    }
    if let Some(path) = module_match {
        return Ok(ResolvedSelector::Module {
            raw: input.to_string(),
            path,
            ambiguities,
        });
    }
    Err(ContextError::SelectorOutOfScope(input.to_string()))
}

/// Resolve a candidate file path under the workspace.
///
/// Accepts absolute paths (must live under `workspace_root`) and
/// workspace-relative paths. Returns `(rel_path, crate_name)` if the
/// path resolves to a file under `crates/<crate>/...`; `None`
/// otherwise.
fn match_file(
    workspace_root: &Path,
    input: &str,
    crates: &BTreeMap<String, WorkspaceCrate>,
) -> Option<(String, String)> {
    let candidate = PathBuf::from(input);
    let abs = if candidate.is_absolute() {
        candidate
    } else {
        workspace_root.join(&candidate)
    };
    if !abs.is_file() {
        return None;
    }
    let canon_root = workspace_root.canonicalize().ok()?;
    let canon_abs = abs.canonicalize().ok()?;
    let rel = canon_abs.strip_prefix(&canon_root).ok()?;
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    let crate_name = crate_for_relative_path(&rel_str, crates)?;
    Some((rel_str, crate_name))
}

/// Match a crate-name selector against the discovered crate set.
fn match_crate(input: &str, crates: &BTreeMap<String, WorkspaceCrate>) -> Option<String> {
    crates.contains_key(input).then(|| input.to_string())
}

/// A bare-string selector is a module candidate iff it contains
/// `::` and parses as a sequence of valid Rust identifiers separated
/// by `::`. Empty / file-shaped / single-token / boundary-violating
/// inputs don't match — those are either crate-name candidates (the
/// crate matcher handles them) or genuinely out of scope.
///
/// The `::` requirement is load-bearing for `CONTEXT_SELECTOR_OUT_OF_SCOPE`:
/// without it, every typo'd word ("not-a-crate") classifies as a
/// Module with an empty trace slice, hiding the user's mistake.
fn match_module(input: &str) -> Option<String> {
    if input.is_empty() {
        return None;
    }
    if input.contains('/') || input.contains('\\') || input.contains('.') {
        return None;
    }
    let normalized = input.replace('-', "_");
    if !normalized.contains("::") {
        return None;
    }
    let segments: Vec<&str> = normalized.split("::").collect();
    let all_idents = segments.iter().all(|seg| is_valid_rust_ident(seg));
    all_idents.then_some(normalized)
}

fn is_valid_rust_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Given a workspace-relative path of the form
/// `crates/<crate-dir>/...`, return the `[package].name` registered
/// for that crate.
fn crate_for_relative_path(rel: &str, crates: &BTreeMap<String, WorkspaceCrate>) -> Option<String> {
    let stripped = rel.strip_prefix("crates/")?;
    let dir = stripped.split('/').next()?;
    let needle = format!("crates/{dir}");
    crates
        .values()
        .find(|c| c.dir == needle)
        .map(|c| c.name.clone())
}

/// Read `[package].name` from a `Cargo.toml`.
fn read_package_name(manifest_path: &Path) -> Result<String, ContextError> {
    #[derive(Deserialize)]
    struct Manifest {
        package: Package,
    }
    #[derive(Deserialize)]
    struct Package {
        name: String,
    }
    let text =
        std::fs::read_to_string(manifest_path).map_err(|err| ContextError::CargoManifestRead {
            path: manifest_path.to_path_buf(),
            err,
        })?;
    let manifest: Manifest =
        toml::from_str(&text).map_err(|err| ContextError::CargoManifestParse {
            path: manifest_path.to_path_buf(),
            err,
        })?;
    Ok(manifest.package.name)
}

/// Workspace-relative path representation. Falls back to a
/// best-effort lossy form if `strip_prefix` fails (which would only
/// happen if the discovered crate dir wasn't under `workspace_root`,
/// e.g. a symlinked path — already a no-follow violation per the
/// walker contract).
fn workspace_relative(workspace_root: &Path, candidate: &Path) -> String {
    candidate
        .strip_prefix(workspace_root)
        .unwrap_or(candidate)
        .to_string_lossy()
        .replace('\\', "/")
}
