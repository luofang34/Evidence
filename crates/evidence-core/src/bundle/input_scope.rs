//! Resolve declared in-scope Cargo **package names** to their
//! workspace-relative **manifest directories**, and assemble the set
//! of source + workspace-control inputs a bundle must hash.
//!
//! Package identity is not a repository path. A boundary config that
//! declares `in_scope = ["evidence-core"]` names a Cargo package whose
//! sources live wherever its `Cargo.toml` sits (here, `crates/…`).
//! Handing the bare name to `git ls-files` as a pathspec matches
//! nothing, which is how an empty `inputs_hashes.json` could accompany
//! a successful generation. Resolution therefore goes through
//! `cargo metadata`'s `manifest_path`, and every step fails closed:
//! an unknown package, a manifest escaping the workspace root, a unit
//! that resolves to zero tracked files, or a zero-input total are all
//! errors — never silent empties.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use walkdir::WalkDir;

use crate::git::{GitError, git_ls_files_in};
use crate::util::{CmdError, cmd_stdout};

/// Pathspecs for workspace-level controlled inputs that affect the
/// build or evidence result but live outside any single in-scope
/// crate directory. `git ls-files` returns only the tracked subset,
/// so absent candidates (e.g. a project with no `rust-toolchain.toml`)
/// drop out without error. Per-crate manifests are captured by the
/// unit directory walk, not here.
pub const WORKSPACE_CONTROL_PATHSPECS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "rust-toolchain",
    "cert",
];

/// A resolved in-scope unit: a declared package name paired with its
/// workspace-relative manifest directory (forward-slash, e.g.
/// `crates/evidence-core`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedUnit {
    /// The `in_scope` package name as declared in `boundary.toml`.
    pub name: String,
    /// Manifest directory relative to the workspace root, used as a
    /// `git ls-files` pathspec.
    pub rel_dir: String,
}

/// Why a given path is part of the hashed input set — recorded so an
/// auditor can see the provenance of every entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputReason {
    /// Tracked source under an in-scope package's manifest directory.
    InScopeUnit(String),
    /// An input explicitly declared required in `boundary.toml`'s
    /// `[inputs]` section — captured even when not git-tracked.
    DeclaredRequired,
    /// A workspace-level controlled input (root manifest/lockfile,
    /// toolchain pin, certification data, …).
    WorkspaceControl,
}

/// One planned input: a canonical workspace-relative path and the
/// reason it is in scope. `path` is a `git ls-files` output path, so
/// it is forward-slash and repo-relative by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputEntry {
    /// Workspace-relative, forward-slash path to hash.
    pub path: String,
    /// Provenance of this entry.
    pub reason: InputReason,
}

/// Fail-closed errors for input-scope resolution and planning.
#[derive(Debug, Error)]
pub enum InputScopeError {
    /// `cargo metadata` output was not the JSON shape this module reads.
    #[error("parsing cargo metadata JSON for scope resolution")]
    ParseMetadata(#[source] serde_json::Error),
    /// A declared in-scope package name is absent from `cargo metadata`.
    #[error("in-scope package '{name}' not found in cargo metadata")]
    MissingPackage {
        /// The unresolved package name.
        name: String,
    },
    /// A package's manifest directory is not under the workspace root.
    #[error("in-scope package '{name}' resolves to '{dir}', which escapes the workspace root")]
    PathEscape {
        /// The offending package name.
        name: String,
        /// The manifest directory that escaped.
        dir: String,
    },
    /// An in-scope unit resolved to zero tracked files.
    #[error("in-scope package '{name}' ({rel_dir}) resolved to zero tracked source files")]
    EmptyScope {
        /// The package that captured nothing.
        name: String,
        /// Its manifest directory.
        rel_dir: String,
    },
    /// The assembled plan captured no inputs at all.
    #[error(
        "in-scope resolution captured zero inputs; refusing to record an empty source baseline"
    )]
    NoInputs,
    /// A controlled input declared required in `boundary.toml` is not
    /// present on disk, so it cannot be hashed into the baseline.
    #[error("required controlled input '{path}' declared in boundary.toml is not present on disk")]
    MissingRequiredInput {
        /// The declared workspace-relative path that is missing.
        path: String,
    },
    /// A declared required input is absolute, contains `..`, or resolves
    /// (via a symlink) outside the workspace root.
    #[error("required controlled input '{path}' escapes the workspace root")]
    RequiredInputEscape {
        /// The offending declared path.
        path: String,
    },
    /// A declared required input could not be canonicalized for the
    /// containment check.
    #[error("canonicalizing required controlled input '{path}'")]
    RequiredInputIo {
        /// The path being canonicalized (or the workspace root).
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// `cargo metadata` failed to launch or exited non-zero.
    #[error("running `cargo metadata` for scope resolution")]
    CargoMetadata(#[source] CmdError),
    /// `git ls-files` failed while enumerating a unit or control set.
    #[error("running `git ls-files` for scope resolution")]
    GitLsFiles(#[source] GitError),
    /// The no-git filesystem-walk fallback failed to read a directory.
    #[error("walking '{path}' for scope resolution (no-git fallback)")]
    Walk {
        /// The directory being walked when the error occurred.
        path: String,
        /// Underlying walk error.
        #[source]
        source: walkdir::Error,
    },
}

/// Resolve in-scope packages and assemble the full input plan by
/// shelling out to `cargo metadata` and `git ls-files`. Blocks on I/O.
/// This is the production entry point; the pure helpers it composes
/// ([`resolve_in_scope_units`], [`assemble_input_plan`]) carry the unit
/// tests. `--no-deps` keeps `cargo metadata` to workspace packages —
/// the full dependency graph is not needed to map names to directories.
pub fn build_input_plan_blocking(
    in_scope: &[String],
    required_inputs: &[String],
) -> Result<Vec<InputEntry>, InputScopeError> {
    let json = cmd_stdout("cargo", &["metadata", "--format-version", "1", "--no-deps"])
        .map_err(InputScopeError::CargoMetadata)?;
    let root = workspace_root_from(&json)?;
    let units = resolve_in_scope_units(&json, in_scope)?;
    let mode = decide_enumeration(&root)?;
    let mut unit_files: Vec<(ResolvedUnit, Vec<String>)> = Vec::with_capacity(units.len());
    for unit in units {
        let files = enumerate_with(mode, &root, &[unit.rel_dir.as_str()])?;
        unit_files.push((unit, files));
    }
    check_required_inputs_exist(&root, required_inputs)?;
    let control = enumerate_with(mode, &root, WORKSPACE_CONTROL_PATHSPECS)?;
    assemble_input_plan(&unit_files, required_inputs, &control)
}

/// Whether the source baseline is enumerated by git or a filesystem walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Enumeration {
    /// `git ls-files` (honors `.gitignore`).
    Git,
    /// Filesystem walk (a packaged source with no git tracking).
    Walk,
}

/// Decide the enumeration mode once for the workspace. Git owns the
/// enumeration only when it actually TRACKS files under `root` — a
/// `.git` ancestor alone is not enough. A packaged workspace unpacked
/// under a repository's git-ignored `target/` has a `.git` ancestor yet
/// zero tracked files (`git ls-files` returns empty), and must be
/// walked. A git command that FAILS while a repository marker is present
/// (corrupt repo, permissions) propagates rather than silently walking;
/// a failure with no repository present is "not a repo" and walks.
fn decide_enumeration(root: &Path) -> Result<Enumeration, InputScopeError> {
    match git_ls_files_in(root, &["."]) {
        Ok(files) if !files.is_empty() => Ok(Enumeration::Git),
        Ok(_) => Ok(Enumeration::Walk),
        Err(e) if has_git_marker(root) => Err(InputScopeError::GitLsFiles(e)),
        Err(_) => Ok(Enumeration::Walk),
    }
}

fn enumerate_with(
    mode: Enumeration,
    root: &Path,
    pathspecs: &[&str],
) -> Result<Vec<String>, InputScopeError> {
    match mode {
        Enumeration::Git => git_ls_files_in(root, pathspecs).map_err(InputScopeError::GitLsFiles),
        Enumeration::Walk => walk_pathspecs(root, pathspecs),
    }
}

/// Combined decide-then-enumerate. Test-only: production decides the
/// mode once per workspace (`build_input_plan_blocking`) and reuses it.
#[cfg(test)]
fn enumerate_pathspecs(root: &Path, pathspecs: &[&str]) -> Result<Vec<String>, InputScopeError> {
    enumerate_with(decide_enumeration(root)?, root, pathspecs)
}

/// True iff `root` or any ancestor carries a `.git` marker. Used only to
/// tell a git *failure* inside a repository (propagate) from "not a
/// repository" (walk) — never as sole proof of git ownership, since a
/// git-ignored subtree still carries the ancestor marker.
fn has_git_marker(root: &Path) -> bool {
    let mut cur = Some(root);
    while let Some(dir) = cur {
        if dir.join(".git").exists() {
            return true;
        }
        cur = dir.parent();
    }
    false
}

fn walk_pathspecs(root: &Path, pathspecs: &[&str]) -> Result<Vec<String>, InputScopeError> {
    let mut files: Vec<String> = Vec::new();
    for spec in pathspecs {
        let abs = root.join(spec);
        if abs.is_file() {
            files.push(spec.replace('\\', "/"));
        } else if abs.is_dir() {
            for entry in WalkDir::new(&abs)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| !is_excluded(e))
            {
                let entry = entry.map_err(|source| InputScopeError::Walk {
                    path: abs.display().to_string(),
                    source,
                })?;
                if entry.file_type().is_file()
                    && let Ok(rel) = entry.path().strip_prefix(root)
                {
                    files.push(to_forward_slash(rel));
                }
            }
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// Directories the no-git walk never descends into: build output and
/// the git metadata dir itself.
fn is_excluded(entry: &walkdir::DirEntry) -> bool {
    entry.file_type().is_dir()
        && matches!(entry.file_name().to_str(), Some("target") | Some(".git"))
}

/// Verify every declared required input exists under `root`. Declared
/// inputs may be git-ignored (generated code), so existence — not git
/// tracking — is the capture precondition. Fails closed on the first
/// absent path so a controlled input can never be silently dropped.
fn check_required_inputs_exist(root: &Path, required: &[String]) -> Result<(), InputScopeError> {
    if required.is_empty() {
        return Ok(());
    }
    let canon_root = root
        .canonicalize()
        .map_err(|source| InputScopeError::RequiredInputIo {
            path: root.display().to_string(),
            source,
        })?;
    for p in required {
        let rel = Path::new(p);
        // Reject absolute paths and any parent-dir escape lexically, up
        // front — a declared controlled input must live inside the
        // workspace.
        if rel.is_absolute() || rel.components().any(|c| c == Component::ParentDir) {
            return Err(InputScopeError::RequiredInputEscape { path: p.clone() });
        }
        let full = root.join(rel);
        if !full.exists() {
            return Err(InputScopeError::MissingRequiredInput { path: p.clone() });
        }
        // Canonicalize and confirm containment — defends against a
        // symlink that survives the lexical check and points out of tree.
        let canon = full
            .canonicalize()
            .map_err(|source| InputScopeError::RequiredInputIo {
                path: p.clone(),
                source,
            })?;
        if !canon.starts_with(&canon_root) {
            return Err(InputScopeError::RequiredInputEscape { path: p.clone() });
        }
    }
    Ok(())
}

/// Parse only `workspace_root` from raw `cargo metadata` JSON. Both the
/// unit pathspecs and the control pathspecs are relative to this root,
/// so `git ls-files` must run from here regardless of the caller's CWD.
fn workspace_root_from(metadata_json: &str) -> Result<PathBuf, InputScopeError> {
    let meta: RawMetadata =
        serde_json::from_str(metadata_json).map_err(InputScopeError::ParseMetadata)?;
    Ok(PathBuf::from(meta.workspace_root))
}

/// Resolve every declared in-scope package name to a workspace-relative
/// manifest directory using raw `cargo metadata --format-version 1`
/// JSON. Fails closed on an unknown package or a manifest that escapes
/// the workspace root. Preserves the declared order of `in_scope`.
pub fn resolve_in_scope_units(
    metadata_json: &str,
    in_scope: &[String],
) -> Result<Vec<ResolvedUnit>, InputScopeError> {
    let meta: RawMetadata =
        serde_json::from_str(metadata_json).map_err(InputScopeError::ParseMetadata)?;
    let root = Path::new(&meta.workspace_root);
    in_scope
        .iter()
        .map(|name| resolve_one(name, root, &meta.packages))
        .collect()
}

fn resolve_one(
    name: &str,
    root: &Path,
    packages: &[RawPackage],
) -> Result<ResolvedUnit, InputScopeError> {
    let pkg = packages.iter().find(|p| p.name == name).ok_or_else(|| {
        InputScopeError::MissingPackage {
            name: name.to_string(),
        }
    })?;
    let manifest_dir = Path::new(&pkg.manifest_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let rel = manifest_dir
        .strip_prefix(root)
        .map_err(|_| InputScopeError::PathEscape {
            name: name.to_string(),
            dir: manifest_dir.display().to_string(),
        })?;
    Ok(ResolvedUnit {
        name: name.to_string(),
        rel_dir: to_forward_slash(rel),
    })
}

/// Assemble the deduplicated, provenance-tagged input plan from the
/// per-unit `git ls-files` results, the declared required inputs, and
/// the workspace-control results. Fails closed if any unit captured
/// nothing, or if the total is zero. Each path appears exactly once;
/// provenance precedence is unit > declared-required > control. Sorted
/// for deterministic bundle bytes.
pub fn assemble_input_plan(
    unit_files: &[(ResolvedUnit, Vec<String>)],
    required_files: &[String],
    control_files: &[String],
) -> Result<Vec<InputEntry>, InputScopeError> {
    let mut by_path: BTreeMap<String, InputReason> = BTreeMap::new();
    for (unit, files) in unit_files {
        if files.is_empty() {
            return Err(InputScopeError::EmptyScope {
                name: unit.name.clone(),
                rel_dir: unit.rel_dir.clone(),
            });
        }
        for f in files {
            by_path
                .entry(f.clone())
                .or_insert_with(|| InputReason::InScopeUnit(unit.name.clone()));
        }
    }
    for f in required_files {
        by_path
            .entry(f.clone())
            .or_insert(InputReason::DeclaredRequired);
    }
    for f in control_files {
        by_path
            .entry(f.clone())
            .or_insert(InputReason::WorkspaceControl);
    }
    if by_path.is_empty() {
        return Err(InputScopeError::NoInputs);
    }
    Ok(by_path
        .into_iter()
        .map(|(path, reason)| InputEntry { path, reason })
        .collect())
}

fn to_forward_slash(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Raw `cargo metadata` subset needed for scope resolution. Private —
/// only [`resolve_in_scope_units`] constructs it.
#[derive(Debug, Deserialize)]
struct RawMetadata {
    workspace_root: String,
    packages: Vec<RawPackage>,
}

#[derive(Debug, Deserialize)]
struct RawPackage {
    name: String,
    manifest_path: String,
}

#[cfg(test)]
#[path = "input_scope/tests.rs"]
mod tests;
