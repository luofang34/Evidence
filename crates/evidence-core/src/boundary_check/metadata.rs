//! `cargo metadata --format-version 1` deserialization subset for
//! the boundary checks. Only the fields the checks actually read
//! are declared; serde's default behavior drops everything else.
//!
//! Pulled out of the parent `boundary_check.rs` so the orchestrator
//! stays under the workspace 500-line limit.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct Metadata {
    pub(super) packages: Vec<Package>,
    pub(super) workspace_members: Vec<String>,
    pub(super) resolve: Resolve,
}

#[derive(Debug, Deserialize)]
pub(super) struct Package {
    pub(super) name: String,
    pub(super) id: String,
    #[serde(default)]
    pub(super) targets: Vec<Target>,
    #[serde(default)]
    pub(super) links: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Target {
    #[serde(default)]
    pub(super) kind: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Resolve {
    pub(super) nodes: Vec<Node>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Node {
    pub(super) id: String,
    pub(super) deps: Vec<NodeDep>,
}

#[derive(Debug, Deserialize)]
pub(super) struct NodeDep {
    pub(super) pkg: String,
}
