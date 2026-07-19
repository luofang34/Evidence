//! Legacy `cert/trace` → corpus graph adapter (HLR-081, LLR-099).
//!
//! Maps a parsed [`TraceFiles`] into graph nodes without renaming,
//! dropping, or synthesizing entries. Identities, titles, edge sets,
//! and selectors match the legacy parser; graph insertion canonicalizes
//! edge order and applies the same invariants as native records.

use crate::trace::{DerivedEntry, HlrEntry, LlrEntry, TestEntry, TraceFiles};

use super::error::CorpusError;
use super::graph::{CorpusGraph, EdgeKind, Node, RequirementLayer, RequirementNode, TestNode};

/// Build a corpus graph from the four-file legacy trace (plus
/// `derived.toml` when present).
///
/// # Errors
///
/// Returns [`CorpusError::LegacyMissingUid`] for an entry without a
/// uid and the relevant identity or edge error when insertion rejects
/// an entry. Edge resolution is the caller's choice via
/// [`CorpusGraph::validate`].
pub fn graph_from_trace_files(files: &TraceFiles) -> Result<CorpusGraph, CorpusError> {
    let mut graph = CorpusGraph::new();
    for entry in &files.sys.requirements {
        graph.insert(requirement_from_hlr_entry(entry, RequirementLayer::Sys)?)?;
    }
    for entry in &files.hlr.requirements {
        graph.insert(requirement_from_hlr_entry(entry, RequirementLayer::Hlr)?)?;
    }
    for entry in &files.llr.requirements {
        graph.insert(requirement_from_llr_entry(entry)?)?;
    }
    if let Some(derived) = &files.derived {
        for entry in &derived.requirements {
            graph.insert(requirement_from_derived_entry(entry)?)?;
        }
    }
    for entry in &files.tests.tests {
        graph.insert(test_from_entry(entry)?)?;
    }
    Ok(graph)
}

/// SYS and HLR share the [`HlrEntry`] shape; the layer is the caller's
/// statement of which file the entry came from.
fn requirement_from_hlr_entry(
    entry: &HlrEntry,
    layer: RequirementLayer,
) -> Result<Node, CorpusError> {
    let uid = require_uid(entry.uid.as_deref(), &entry.id)?;
    Ok(Node::Requirement(RequirementNode {
        uid,
        id: entry.id.clone(),
        title: entry.title.clone(),
        layer,
        edges: derives_from_edges(&entry.traces_to),
    }))
}

fn requirement_from_llr_entry(entry: &LlrEntry) -> Result<Node, CorpusError> {
    let uid = require_uid(entry.uid.as_deref(), &entry.id)?;
    Ok(Node::Requirement(RequirementNode {
        uid,
        id: entry.id.clone(),
        title: entry.title.clone(),
        layer: RequirementLayer::Llr,
        edges: derives_from_edges(&entry.traces_to),
    }))
}

/// Derived requirements have no parent by definition — no edges.
fn requirement_from_derived_entry(entry: &DerivedEntry) -> Result<Node, CorpusError> {
    let uid = require_uid(entry.uid.as_deref(), &entry.id)?;
    Ok(Node::Requirement(RequirementNode {
        uid,
        id: entry.id.clone(),
        title: entry.title.clone(),
        layer: RequirementLayer::Derived,
        edges: Vec::new(),
    }))
}

fn test_from_entry(entry: &TestEntry) -> Result<Node, CorpusError> {
    let uid = require_uid(entry.uid.as_deref(), &entry.id)?;
    Ok(Node::Test(TestNode {
        uid,
        id: entry.id.clone(),
        title: entry.title.clone(),
        selectors: entry.all_selectors(),
        edges: entry
            .traces_to
            .iter()
            .map(|target| (EdgeKind::Verifies, target.clone()))
            .collect(),
    }))
}

fn require_uid(uid: Option<&str>, id: &str) -> Result<String, CorpusError> {
    uid.map(str::to_string)
        .ok_or_else(|| CorpusError::LegacyMissingUid { id: id.to_string() })
}

fn derives_from_edges(traces_to: &[String]) -> Vec<(EdgeKind, String)> {
    traces_to
        .iter()
        .map(|target| (EdgeKind::DerivesFrom, target.clone()))
        .collect()
}
