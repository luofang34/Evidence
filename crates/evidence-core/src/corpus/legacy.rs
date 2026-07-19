//! Legacy `cert/trace` → corpus graph adapter (HLR-081, LLR-099).
//!
//! Maps a parsed [`TraceFiles`] into graph nodes without renaming,
//! dropping, or synthesizing entries. Identities, titles, edge sets,
//! and selectors match the legacy parser; graph insertion canonicalizes
//! edge order and applies the same invariants as native records.

use crate::trace::{DerivedEntry, HlrEntry, LlrEntry, TestEntry, TraceFiles};

use super::error::CorpusError;
use super::graph::{
    CorpusGraph, EdgeKind, Node, RequirementLayer, RequirementMetadata, RequirementNode,
    TestMetadata, TestNode, TraceMetadata,
};

struct AdaptedNode {
    node: Node,
    metadata: TraceMetadata,
}

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
    graph_from_trace_parts(
        &files.sys.requirements,
        &files.hlr.requirements,
        &files.llr.requirements,
        &files.tests.tests,
        files
            .derived
            .as_ref()
            .map_or(&[], |derived| derived.requirements.as_slice()),
    )
}

pub(crate) fn graph_from_trace_parts(
    sys: &[HlrEntry],
    hlrs: &[HlrEntry],
    llrs: &[LlrEntry],
    tests: &[TestEntry],
    derived: &[DerivedEntry],
) -> Result<CorpusGraph, CorpusError> {
    let mut graph = CorpusGraph::new();
    for entry in sys {
        insert_adapted(
            &mut graph,
            requirement_from_hlr_entry(entry, RequirementLayer::Sys)?,
        )?;
    }
    for entry in hlrs {
        insert_adapted(
            &mut graph,
            requirement_from_hlr_entry(entry, RequirementLayer::Hlr)?,
        )?;
    }
    for entry in llrs {
        insert_adapted(&mut graph, requirement_from_llr_entry(entry)?)?;
    }
    for entry in derived {
        insert_adapted(&mut graph, requirement_from_derived_entry(entry)?)?;
    }
    for entry in tests {
        insert_adapted(&mut graph, test_from_entry(entry)?)?;
    }
    Ok(graph)
}

fn insert_adapted(graph: &mut CorpusGraph, adapted: AdaptedNode) -> Result<(), CorpusError> {
    graph.insert_with_trace_metadata(adapted.node, adapted.metadata)
}

/// SYS and HLR share the [`HlrEntry`] shape; the layer is the caller's
/// statement of which file the entry came from.
fn requirement_from_hlr_entry(
    entry: &HlrEntry,
    layer: RequirementLayer,
) -> Result<AdaptedNode, CorpusError> {
    let uid = require_uid(entry.uid.as_deref(), &entry.id)?;
    Ok(AdaptedNode {
        node: Node::Requirement(RequirementNode {
            uid,
            id: entry.id.clone(),
            title: entry.title.clone(),
            layer,
            edges: derives_from_edges(&entry.traces_to),
        }),
        metadata: TraceMetadata::Requirement(RequirementMetadata {
            namespace: entry.ns.clone(),
            sort_key: entry.sort_key,
            scope: entry.scope.clone(),
            category: entry.category.clone(),
            source: entry.source.clone(),
            modules: Vec::new(),
            surfaces: canonical_claims(&entry.surfaces),
            emits: Vec::new(),
            verification_methods: canonical_claims(&entry.verification_methods),
        }),
    })
}

fn requirement_from_llr_entry(entry: &LlrEntry) -> Result<AdaptedNode, CorpusError> {
    let uid = require_uid(entry.uid.as_deref(), &entry.id)?;
    Ok(AdaptedNode {
        node: Node::Requirement(RequirementNode {
            uid,
            id: entry.id.clone(),
            title: entry.title.clone(),
            layer: RequirementLayer::Llr,
            edges: derives_from_edges(&entry.traces_to),
        }),
        metadata: TraceMetadata::Requirement(RequirementMetadata {
            namespace: entry.ns.clone(),
            sort_key: entry.sort_key,
            scope: None,
            category: None,
            source: entry.source.clone(),
            modules: entry.modules.clone(),
            surfaces: Vec::new(),
            emits: canonical_claims(&entry.emits),
            verification_methods: canonical_claims(&entry.verification_methods),
        }),
    })
}

/// Derived requirements have no parent by definition — no edges.
fn requirement_from_derived_entry(entry: &DerivedEntry) -> Result<AdaptedNode, CorpusError> {
    let uid = require_uid(entry.uid.as_deref(), &entry.id)?;
    Ok(AdaptedNode {
        node: Node::Requirement(RequirementNode {
            uid,
            id: entry.id.clone(),
            title: entry.title.clone(),
            layer: RequirementLayer::Derived,
            edges: Vec::new(),
        }),
        metadata: TraceMetadata::Requirement(RequirementMetadata {
            sort_key: entry.sort_key,
            source: entry.source.clone(),
            ..RequirementMetadata::default()
        }),
    })
}

fn test_from_entry(entry: &TestEntry) -> Result<AdaptedNode, CorpusError> {
    let uid = require_uid(entry.uid.as_deref(), &entry.id)?;
    Ok(AdaptedNode {
        node: Node::Test(TestNode {
            uid,
            id: entry.id.clone(),
            title: entry.title.clone(),
            selectors: entry.all_selectors(),
            edges: entry
                .traces_to
                .iter()
                .map(|target| (EdgeKind::Verifies, target.clone()))
                .collect(),
        }),
        metadata: TraceMetadata::Test(TestMetadata {
            namespace: entry.ns.clone(),
            sort_key: entry.sort_key,
            category: entry.category.clone(),
            source: entry.source.clone(),
            primary_selector: entry.test_selector.clone(),
        }),
    })
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

fn canonical_claims(claims: &[String]) -> Vec<String> {
    let mut claims = claims.to_vec();
    claims.sort();
    claims.dedup();
    claims
}
