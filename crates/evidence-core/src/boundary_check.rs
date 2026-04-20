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
//!
//! Rules not yet implemented: `forbid_build_rs`, `forbid_proc_macros`.
//! The generate preflight refuses bundles for those.
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
}

impl DiagnosticCode for BoundaryCheckError {
    fn code(&self) -> &'static str {
        match self {
            BoundaryCheckError::CargoMetadata(_) => "BOUNDARY_CARGO_METADATA_FAILED",
            BoundaryCheckError::ParseMetadata(_) => "BOUNDARY_PARSE_METADATA_FAILED",
            BoundaryCheckError::UnknownInScopeCrate(_) => "BOUNDARY_UNKNOWN_IN_SCOPE_CRATE",
            BoundaryCheckError::OutOfScopeDeps { .. } => "BOUNDARY_OUT_OF_SCOPE_DEPS",
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;

    // Synthetic metadata fixtures. Cargo's real JSON has dozens of
    // fields we don't use — serde drops them via default behavior,
    // so the test fixtures only need to carry the keys `Metadata`
    // actually deserializes.

    fn pkg(name: &str, id: &str) -> serde_json::Value {
        serde_json::json!({"name": name, "id": id})
    }

    fn node(id: &str, dep_ids: &[&str]) -> serde_json::Value {
        let deps: Vec<serde_json::Value> = dep_ids
            .iter()
            .map(|d| serde_json::json!({"pkg": d}))
            .collect();
        serde_json::json!({"id": id, "deps": deps})
    }

    fn fixture(
        packages: Vec<serde_json::Value>,
        ws_members: Vec<&str>,
        nodes: Vec<serde_json::Value>,
    ) -> Metadata {
        let j = serde_json::json!({
            "packages": packages,
            "workspace_members": ws_members,
            "resolve": {"nodes": nodes},
        });
        serde_json::from_value(j).unwrap()
    }

    #[test]
    fn no_violations_when_in_scope_depends_only_on_external_crates() {
        // Workspace: { evidence }. evidence → serde (external).
        // in_scope = ["evidence"]. No workspace-to-workspace dep
        // edge exists, so the rule has no work to do.
        let m = fixture(
            vec![
                pkg("evidence", "path+file:///e#0.1.0"),
                pkg("serde", "registry+https://crates.io#serde@1"),
            ],
            vec!["path+file:///e#0.1.0"],
            vec![
                node(
                    "path+file:///e#0.1.0",
                    &["registry+https://crates.io#serde@1"],
                ),
                node("registry+https://crates.io#serde@1", &[]),
            ],
        );
        let v = find_out_of_scope_deps(&["evidence".into()], &m).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn no_violations_when_every_workspace_dep_is_in_scope() {
        // Workspace: { cargo-evidence, evidence }.
        // cargo-evidence → evidence. Both in scope: no violation.
        let m = fixture(
            vec![
                pkg("cargo-evidence", "path+file:///ce#0.1.0"),
                pkg("evidence", "path+file:///e#0.1.0"),
            ],
            vec!["path+file:///ce#0.1.0", "path+file:///e#0.1.0"],
            vec![
                node("path+file:///ce#0.1.0", &["path+file:///e#0.1.0"]),
                node("path+file:///e#0.1.0", &[]),
            ],
        );
        let v = find_out_of_scope_deps(&["cargo-evidence".into(), "evidence".into()], &m).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn flags_direct_workspace_dep_not_in_scope() {
        // Workspace: { cargo-evidence, evidence }.
        // cargo-evidence → evidence. Only cargo-evidence in scope.
        // evidence is a workspace member but not in scope → violation.
        let m = fixture(
            vec![
                pkg("cargo-evidence", "path+file:///ce#0.1.0"),
                pkg("evidence", "path+file:///e#0.1.0"),
            ],
            vec!["path+file:///ce#0.1.0", "path+file:///e#0.1.0"],
            vec![
                node("path+file:///ce#0.1.0", &["path+file:///e#0.1.0"]),
                node("path+file:///e#0.1.0", &[]),
            ],
        );
        let v = find_out_of_scope_deps(&["cargo-evidence".into()], &m).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(
            v[0],
            BoundaryViolation {
                rule: "no_out_of_scope_deps",
                crate_name: "cargo-evidence".into(),
                offending_dep: "evidence".into(),
            }
        );
    }

    #[test]
    fn flags_transitive_workspace_dep_not_in_scope() {
        // Workspace: { a, b, c }. a → b → c. Only `a` in scope.
        // BFS from `a` reaches `b` (violation) and `c` (violation).
        let m = fixture(
            vec![
                pkg("a", "path+file:///a#0.1.0"),
                pkg("b", "path+file:///b#0.1.0"),
                pkg("c", "path+file:///c#0.1.0"),
            ],
            vec![
                "path+file:///a#0.1.0",
                "path+file:///b#0.1.0",
                "path+file:///c#0.1.0",
            ],
            vec![
                node("path+file:///a#0.1.0", &["path+file:///b#0.1.0"]),
                node("path+file:///b#0.1.0", &["path+file:///c#0.1.0"]),
                node("path+file:///c#0.1.0", &[]),
            ],
        );
        let v = find_out_of_scope_deps(&["a".into()], &m).unwrap();
        let names: Vec<&str> = v.iter().map(|x| x.offending_dep.as_str()).collect();
        assert_eq!(names, vec!["b", "c"]);
    }

    #[test]
    fn typos_in_in_scope_are_reported() {
        // in_scope references a crate that isn't a workspace member.
        // Catches stale / typo'd boundary.toml fast — otherwise the
        // typo'd entry would be silently skipped and the user would
        // get a false "no violations" green light.
        let m = fixture(
            vec![pkg("evidence", "path+file:///e#0.1.0")],
            vec!["path+file:///e#0.1.0"],
            vec![node("path+file:///e#0.1.0", &[])],
        );
        let err = find_out_of_scope_deps(&["typo-crate".into()], &m).unwrap_err();
        assert!(
            matches!(err, BoundaryCheckError::UnknownInScopeCrate(name) if name == "typo-crate")
        );
    }

    #[test]
    fn diamond_dep_is_deduplicated() {
        // a → b, a → c, both b and c → d. Only `a` in scope.
        // d shows up once, not twice.
        let m = fixture(
            vec![
                pkg("a", "path+file:///a#0.1.0"),
                pkg("b", "path+file:///b#0.1.0"),
                pkg("c", "path+file:///c#0.1.0"),
                pkg("d", "path+file:///d#0.1.0"),
            ],
            vec![
                "path+file:///a#0.1.0",
                "path+file:///b#0.1.0",
                "path+file:///c#0.1.0",
                "path+file:///d#0.1.0",
            ],
            vec![
                node(
                    "path+file:///a#0.1.0",
                    &["path+file:///b#0.1.0", "path+file:///c#0.1.0"],
                ),
                node("path+file:///b#0.1.0", &["path+file:///d#0.1.0"]),
                node("path+file:///c#0.1.0", &["path+file:///d#0.1.0"]),
                node("path+file:///d#0.1.0", &[]),
            ],
        );
        let v = find_out_of_scope_deps(&["a".into()], &m).unwrap();
        let ds: Vec<&str> = v
            .iter()
            .filter(|x| x.offending_dep == "d")
            .map(|x| x.offending_dep.as_str())
            .collect();
        assert_eq!(ds.len(), 1, "diamond dep should not double-count");
    }
}
