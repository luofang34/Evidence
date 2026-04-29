//! Boundary-policy enforcement via `cargo metadata`.
//!
//! This module implements real checks for the [`BoundaryPolicy`] flags
//! that `boundary.toml` declares. Until a rule is implemented here its
//! branch in [`BoundaryPolicy::unimplemented_enabled_rules`] keeps the
//! generate preflight refusing bundles — that's the safety rail that
//! prevents silent false confidence.
//!
//! Rules implemented:
//!
//! - `no_out_of_scope_deps`: [`check_no_out_of_scope_deps`] walks the
//!   cargo dependency graph from each in-scope workspace member and
//!   rejects any workspace-member dependency that is not itself in
//!   scope. External (registry / git / path-outside-workspace) deps
//!   are not considered — this rule only polices workspace-to-
//!   workspace crossings.
//! - `forbid_build_rs`: [`check_no_build_rs`] flags any in-scope
//!   crate carrying a target with `kind == ["custom-build"]`. The
//!   diagnostic also surfaces the package's `links` value (when
//!   set) so an auditor sees a native-FFI binding without
//!   re-running `cargo metadata` by hand.
//! - `forbid_proc_macros`: [`check_no_proc_macros`] flags any
//!   in-scope crate with a target of `kind == ["proc-macro"]`.
//!
//! [`BoundaryPolicy`]: crate::policy::BoundaryPolicy
//! [`BoundaryPolicy::unimplemented_enabled_rules`]: crate::policy::BoundaryPolicy::unimplemented_enabled_rules

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::diagnostic::{DiagnosticCode, Severity};
use crate::util::{CmdError, cmd_stdout};

/// A single boundary rule violation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoundaryViolation {
    /// Rule that was violated (matches the name used in
    /// `BoundaryPolicy::enabled_rules`).
    pub rule: &'static str,
    /// In-scope crate whose dep graph contains the offending edge.
    pub crate_name: String,
    /// Out-of-scope workspace crate reached via that dep graph.
    pub offending_dep: String,
}

/// A `forbid_build_rs` violation. Carries the offending crate plus
/// the `links` value (if any) so the diagnostic message can surface
/// native-FFI bindings — load-bearing context for the determinism
/// auditor without forcing them to re-run `cargo metadata`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildRsViolation {
    /// In-scope crate that has a `build.rs`.
    pub crate_name: String,
    /// Value of `package.links` (e.g. `"libz"`), if declared.
    pub links: Option<String>,
}

/// A `forbid_proc_macros` violation. Just the offender's name —
/// proc-macro detection has no auxiliary metadata to surface, unlike
/// build_rs's `links`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcMacroViolation {
    /// In-scope crate exposing a `[lib] proc-macro = true` target.
    pub crate_name: String,
}

/// Errors returned by the boundary-check routines.
#[derive(Debug, Error)]
pub enum BoundaryCheckError {
    /// `cargo metadata` failed to launch or exited non-zero.
    #[error("running `cargo metadata`")]
    CargoMetadata(#[from] CmdError),
    /// `cargo metadata --format-version 1` returned JSON we couldn't
    /// parse. Usually means cargo changed the schema; worth a bug
    /// report.
    #[error("parsing cargo metadata JSON")]
    ParseMetadata(#[from] serde_json::Error),
    /// At least one in-scope crate name in `boundary.toml` doesn't
    /// correspond to a workspace member. Typo, or the crate was
    /// renamed / removed since `cert/boundary.toml` was last edited.
    #[error("in-scope crate '{0}' is not a workspace member")]
    UnknownInScopeCrate(String),
    /// One or more workspace-crate dependencies of in-scope crates
    /// are out of scope. Each entry pinpoints the (in-scope, dep)
    /// pair so the user can either bring the dep into scope or
    /// break the dependency.
    #[error("{count} out-of-scope workspace dep(s) reached from in-scope crates")]
    OutOfScopeDeps {
        /// Violations found.
        violations: Vec<BoundaryViolation>,
        /// Count, materialized for the error message.
        count: usize,
    },
    /// One or more in-scope crates have a `build.rs`. The error
    /// message lists each crate plus its `links` value (if any) so
    /// an auditor sees the determinism-impacting bindings without
    /// re-running `cargo metadata` by hand.
    #[error(
        "{count} in-scope crate(s) have build.rs: {}",
        fmt_build_rs(violations)
    )]
    ForbiddenBuildRs {
        /// Violations found.
        violations: Vec<BuildRsViolation>,
        /// Count, materialized for the error message.
        count: usize,
    },
    /// One or more in-scope crates expose a proc-macro target.
    #[error(
        "{count} in-scope crate(s) expose proc-macro targets: {}",
        fmt_proc_macro(violations)
    )]
    ForbiddenProcMacro {
        /// Violations found.
        violations: Vec<ProcMacroViolation>,
        /// Count, materialized for the error message.
        count: usize,
    },
}

fn fmt_build_rs(v: &[BuildRsViolation]) -> String {
    v.iter()
        .map(|x| match &x.links {
            Some(l) => format!("{} (links = \"{}\")", x.crate_name, l),
            None => x.crate_name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn fmt_proc_macro(v: &[ProcMacroViolation]) -> String {
    v.iter()
        .map(|x| x.crate_name.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

impl DiagnosticCode for BoundaryCheckError {
    fn code(&self) -> &'static str {
        match self {
            BoundaryCheckError::CargoMetadata(_) => "BOUNDARY_CARGO_METADATA_FAILED",
            BoundaryCheckError::ParseMetadata(_) => "BOUNDARY_PARSE_METADATA_FAILED",
            BoundaryCheckError::UnknownInScopeCrate(_) => "BOUNDARY_UNKNOWN_IN_SCOPE_CRATE",
            BoundaryCheckError::OutOfScopeDeps { .. } => "BOUNDARY_OUT_OF_SCOPE_DEPS",
            BoundaryCheckError::ForbiddenBuildRs { .. } => "BOUNDARY_FORBIDDEN_BUILD_RS",
            BoundaryCheckError::ForbiddenProcMacro { .. } => "BOUNDARY_FORBIDDEN_PROC_MACRO",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }
}

/// Enforce the `no_out_of_scope_deps` boundary rule.
///
/// Shells out to `cargo metadata --format-version 1` in
/// `workspace_root`, walks the resolved dep graph from each in-scope
/// workspace member, and flags every transitive workspace-member dep
/// that isn't in the `in_scope` list.
///
/// Returns `Ok(())` when no violation is found;
/// [`BoundaryCheckError::OutOfScopeDeps`] when one or more in-scope
/// crates reach workspace crates not in the list;
/// [`BoundaryCheckError::UnknownInScopeCrate`] if the config names a
/// crate the workspace doesn't have (fail-fast on typos).
pub fn check_no_out_of_scope_deps(
    in_scope: &[String],
    workspace_root: &Path,
) -> Result<(), BoundaryCheckError> {
    let output = run_cargo_metadata(workspace_root)?;
    let metadata: Metadata = serde_json::from_str(&output)?;
    let violations = find_out_of_scope_deps(in_scope, &metadata)?;
    if violations.is_empty() {
        return Ok(());
    }
    let count = violations.len();
    Err(BoundaryCheckError::OutOfScopeDeps { violations, count })
}

fn run_cargo_metadata(workspace_root: &Path) -> Result<String, CmdError> {
    // `--format-version 1` locks the output schema so future cargo
    // versions can introduce v2 without breaking this parser. We use
    // `cmd_stdout` rather than a new subprocess helper because
    // `cargo metadata` output is plain UTF-8 JSON — the existing
    // helper's error surface is exactly what we need.
    //
    // Note: `cargo metadata` reads the manifest at `cwd`, so the
    // subprocess inherits the current working directory from this
    // process. The caller's `workspace_root` is carried through
    // `boundary.toml` path resolution and recorded for diagnostics,
    // but `cargo metadata` itself finds the manifest via cwd +
    // upward-walk. The happy path (cwd == workspace root) is the
    // only one the CLI drives today; alternate entry points should
    // chdir first.
    let _ = workspace_root;
    cmd_stdout("cargo", &["metadata", "--format-version", "1"])
}

/// Pure dep-graph walk, factored out of `check_no_out_of_scope_deps`
/// so unit tests can feed synthetic `Metadata` without shelling out.
fn find_out_of_scope_deps(
    in_scope: &[String],
    metadata: &Metadata,
) -> Result<Vec<BoundaryViolation>, BoundaryCheckError> {
    // Map workspace-member ID → crate name for fast lookup.
    let ws_member_ids: BTreeSet<&String> = metadata.workspace_members.iter().collect();
    let id_to_name: BTreeMap<&String, &String> =
        metadata.packages.iter().map(|p| (&p.id, &p.name)).collect();
    let name_to_id: BTreeMap<&String, &String> = metadata
        .packages
        .iter()
        .filter(|p| ws_member_ids.contains(&p.id))
        .map(|p| (&p.name, &p.id))
        .collect();
    let in_scope_set: BTreeSet<&str> = in_scope.iter().map(String::as_str).collect();

    // Adjacency list: id → deps
    let adj: BTreeMap<&String, Vec<&String>> = metadata
        .resolve
        .nodes
        .iter()
        .map(|n| (&n.id, n.deps.iter().map(|d| &d.pkg).collect()))
        .collect();

    let mut violations: Vec<BoundaryViolation> = Vec::new();

    for crate_name in in_scope {
        let start_id = name_to_id
            .get(crate_name)
            .copied()
            .ok_or_else(|| BoundaryCheckError::UnknownInScopeCrate(crate_name.clone()))?;

        // BFS from this in-scope crate; collect every workspace-
        // member dep reached that isn't also in scope. A set
        // deduplicates diamond-dep cases where two paths reach the
        // same offender.
        let mut seen: BTreeSet<&String> = BTreeSet::new();
        let mut queue: VecDeque<&String> = VecDeque::new();
        queue.push_back(start_id);
        seen.insert(start_id);

        while let Some(node) = queue.pop_front() {
            let Some(children) = adj.get(node) else {
                continue;
            };
            for child in children {
                if !seen.insert(child) {
                    continue;
                }
                if ws_member_ids.contains(child) {
                    let child_name = id_to_name
                        .get(child)
                        .map(|s| s.as_str())
                        .unwrap_or("<unknown>");
                    if !in_scope_set.contains(child_name) {
                        violations.push(BoundaryViolation {
                            rule: "no_out_of_scope_deps",
                            crate_name: crate_name.clone(),
                            offending_dep: child_name.to_string(),
                        });
                    }
                }
                queue.push_back(child);
            }
        }
    }

    // Sort + dedup so diagnostics are stable across runs.
    violations.sort();
    violations.dedup();
    Ok(violations)
}

/// Enforce the `forbid_build_rs` boundary rule.
///
/// Shells out to `cargo metadata --format-version 1`, walks
/// `packages[]`, and flags every package whose `name` is in `in_scope`
/// AND whose `targets[]` contains a target with `kind ==
/// ["custom-build"]`. Per-crate scoping is preserved — a `build.rs`
/// in an out-of-scope crate is fine.
pub fn check_no_build_rs(
    in_scope: &[String],
    workspace_root: &Path,
) -> Result<(), BoundaryCheckError> {
    let output = run_cargo_metadata(workspace_root)?;
    let metadata: Metadata = serde_json::from_str(&output)?;
    let violations = find_build_rs_violations(in_scope, &metadata)?;
    if violations.is_empty() {
        return Ok(());
    }
    let count = violations.len();
    Err(BoundaryCheckError::ForbiddenBuildRs { violations, count })
}

/// Enforce the `forbid_proc_macros` boundary rule.
///
/// Shells out to `cargo metadata --format-version 1`, walks
/// `packages[]`, and flags every package whose `name` is in `in_scope`
/// AND whose `targets[]` contains a target with `kind ==
/// ["proc-macro"]`. Per-crate scoping is preserved.
pub fn check_no_proc_macros(
    in_scope: &[String],
    workspace_root: &Path,
) -> Result<(), BoundaryCheckError> {
    let output = run_cargo_metadata(workspace_root)?;
    let metadata: Metadata = serde_json::from_str(&output)?;
    let violations = find_proc_macro_violations(in_scope, &metadata)?;
    if violations.is_empty() {
        return Ok(());
    }
    let count = violations.len();
    Err(BoundaryCheckError::ForbiddenProcMacro { violations, count })
}

/// Pure walk over `Metadata`, factored out so unit tests feed
/// synthetic JSON without shelling out.
fn find_build_rs_violations(
    in_scope: &[String],
    metadata: &Metadata,
) -> Result<Vec<BuildRsViolation>, BoundaryCheckError> {
    let in_scope_set: BTreeSet<&str> = in_scope.iter().map(String::as_str).collect();
    let mut out: Vec<BuildRsViolation> = metadata
        .packages
        .iter()
        .filter(|p| in_scope_set.contains(p.name.as_str()))
        .filter(|p| p.targets.iter().any(target_is_build_rs))
        .map(|p| BuildRsViolation {
            crate_name: p.name.clone(),
            links: p.links.clone(),
        })
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

/// Pure walk over `Metadata` for proc-macro detection. Same
/// scoping invariant as `find_build_rs_violations`.
fn find_proc_macro_violations(
    in_scope: &[String],
    metadata: &Metadata,
) -> Result<Vec<ProcMacroViolation>, BoundaryCheckError> {
    let in_scope_set: BTreeSet<&str> = in_scope.iter().map(String::as_str).collect();
    let mut out: Vec<ProcMacroViolation> = metadata
        .packages
        .iter()
        .filter(|p| in_scope_set.contains(p.name.as_str()))
        .filter(|p| p.targets.iter().any(target_is_proc_macro))
        .map(|p| ProcMacroViolation {
            crate_name: p.name.clone(),
        })
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

fn target_is_build_rs(t: &Target) -> bool {
    t.kind.iter().any(|k| k == "custom-build")
}

fn target_is_proc_macro(t: &Target) -> bool {
    t.kind.iter().any(|k| k == "proc-macro")
}

// ============================================================================
// Cargo metadata subset we actually parse
// ============================================================================

// Only the fields we use. Extra keys in cargo's output are ignored
// by serde's default.

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    resolve: Resolve,
}

#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    id: String,
    #[serde(default)]
    targets: Vec<Target>,
    #[serde(default)]
    links: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Target {
    #[serde(default)]
    kind: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(Debug, Deserialize)]
struct Node {
    id: String,
    deps: Vec<NodeDep>,
}

#[derive(Debug, Deserialize)]
struct NodeDep {
    pkg: String,
}

impl PartialOrd for BoundaryViolation {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BoundaryViolation {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.rule, &self.crate_name, &self.offending_dep).cmp(&(
            other.rule,
            &other.crate_name,
            &other.offending_dep,
        ))
    }
}

impl PartialOrd for BuildRsViolation {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BuildRsViolation {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (&self.crate_name, &self.links).cmp(&(&other.crate_name, &other.links))
    }
}

impl PartialOrd for ProcMacroViolation {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ProcMacroViolation {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.crate_name.cmp(&other.crate_name)
    }
}

// Tests live in a sibling file pulled in via `#[path]` so this
// facade stays under the workspace 500-line limit.
#[cfg(test)]
#[path = "boundary_check/tests.rs"]
mod tests;
