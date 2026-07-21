//! Canonical input projection for traceability matrix rendering.

use crate::corpus::{CorpusGraph, EdgeKind, Node, RequirementLayer, TraceMetadata};

use super::super::entries::{HlrEntry, HlrFile, LlrEntry, LlrFile, TestEntry, TestsFile};

pub(super) struct MatrixView {
    pub(super) hlrs: Vec<MatrixRequirement>,
    pub(super) llrs: Vec<MatrixRequirement>,
    pub(super) tests: Vec<MatrixTest>,
}

pub(super) struct MatrixRequirement {
    pub(super) uid: Option<String>,
    pub(super) ns: Option<String>,
    pub(super) id: String,
    pub(super) title: String,
    pub(super) sort_key: Option<i64>,
    pub(super) scope: Option<String>,
    pub(super) category: Option<String>,
    pub(super) source: Option<String>,
    pub(super) modules: Vec<String>,
    pub(super) traces_to: Vec<String>,
}

pub(super) struct MatrixTest {
    pub(super) ns: Option<String>,
    pub(super) id: String,
    pub(super) title: String,
    pub(super) sort_key: Option<i64>,
    pub(super) category: Option<String>,
    pub(super) test_selector: Option<String>,
    pub(super) source: Option<String>,
    pub(super) traces_to: Vec<String>,
}

impl MatrixView {
    pub(super) fn from_trace_files(hlr: &HlrFile, llr: &LlrFile, tests: &TestsFile) -> Self {
        Self::new(
            hlr.requirements.iter().map(requirement_from_hlr).collect(),
            llr.requirements.iter().map(requirement_from_llr).collect(),
            tests.tests.iter().map(test_from_entry).collect(),
        )
    }

    pub(super) fn from_graph(graph: &CorpusGraph) -> Self {
        let mut hlrs = Vec::new();
        let mut llrs = Vec::new();
        let mut tests = Vec::new();
        for node in graph.nodes() {
            match node {
                Node::Requirement(requirement) => {
                    let metadata = match graph.trace_metadata(&requirement.uid) {
                        Some(TraceMetadata::Requirement(metadata)) => Some(metadata),
                        Some(TraceMetadata::Test(_)) | None => None,
                    };
                    let projected = MatrixRequirement {
                        uid: Some(requirement.uid.clone()),
                        ns: metadata.and_then(|value| value.namespace.clone()),
                        id: requirement.id.clone(),
                        title: requirement.title.clone(),
                        sort_key: metadata.and_then(|value| value.sort_key),
                        scope: metadata.and_then(|value| value.scope.clone()),
                        category: metadata.and_then(|value| value.category.clone()),
                        source: metadata.and_then(|value| value.source.clone()),
                        modules: metadata.map_or_else(Vec::new, |value| value.modules.clone()),
                        traces_to: edge_targets(&requirement.edges, EdgeKind::DerivesFrom),
                    };
                    match requirement.layer {
                        RequirementLayer::Hlr => hlrs.push(projected),
                        RequirementLayer::Llr => llrs.push(projected),
                        RequirementLayer::Source
                        | RequirementLayer::Sys
                        | RequirementLayer::Derived => {}
                    }
                }
                Node::Test(test) => {
                    let metadata = match graph.trace_metadata(&test.uid) {
                        Some(TraceMetadata::Test(metadata)) => Some(metadata),
                        Some(TraceMetadata::Requirement(_)) | None => None,
                    };
                    tests.push(MatrixTest {
                        ns: metadata.and_then(|value| value.namespace.clone()),
                        id: test.id.clone(),
                        title: test.title.clone(),
                        sort_key: metadata.and_then(|value| value.sort_key),
                        category: metadata.and_then(|value| value.category.clone()),
                        test_selector: metadata.and_then(|value| value.primary_selector.clone()),
                        source: metadata.and_then(|value| value.source.clone()),
                        traces_to: edge_targets(&test.edges, EdgeKind::Verifies),
                    });
                }
                // Review decisions and source revisions are not
                // traceability-matrix rows.
                Node::Review(_) | Node::SourceRevision(_) => {}
            }
        }
        Self::new(hlrs, llrs, tests)
    }

    fn new(
        mut hlrs: Vec<MatrixRequirement>,
        mut llrs: Vec<MatrixRequirement>,
        mut tests: Vec<MatrixTest>,
    ) -> Self {
        hlrs.sort_by(requirement_order);
        llrs.sort_by(requirement_order);
        tests.sort_by(|a, b| {
            a.sort_key
                .unwrap_or(0)
                .cmp(&b.sort_key.unwrap_or(0))
                .then_with(|| a.id.cmp(&b.id))
        });
        Self { hlrs, llrs, tests }
    }
}

fn requirement_from_hlr(entry: &HlrEntry) -> MatrixRequirement {
    MatrixRequirement {
        uid: entry.uid.clone(),
        ns: entry.ns.clone(),
        id: entry.id.clone(),
        title: entry.title.clone(),
        sort_key: entry.sort_key,
        scope: entry.scope.clone(),
        category: entry.category.clone(),
        source: entry.source.clone(),
        modules: Vec::new(),
        traces_to: entry.traces_to.clone(),
    }
}

fn requirement_from_llr(entry: &LlrEntry) -> MatrixRequirement {
    MatrixRequirement {
        uid: entry.uid.clone(),
        ns: entry.ns.clone(),
        id: entry.id.clone(),
        title: entry.title.clone(),
        sort_key: entry.sort_key,
        scope: None,
        category: None,
        source: entry.source.clone(),
        modules: entry.modules.clone(),
        traces_to: entry.traces_to.clone(),
    }
}

fn test_from_entry(entry: &TestEntry) -> MatrixTest {
    MatrixTest {
        ns: entry.ns.clone(),
        id: entry.id.clone(),
        title: entry.title.clone(),
        sort_key: entry.sort_key,
        category: entry.category.clone(),
        test_selector: entry.test_selector.clone(),
        source: entry.source.clone(),
        traces_to: entry.traces_to.clone(),
    }
}

fn requirement_order(a: &MatrixRequirement, b: &MatrixRequirement) -> std::cmp::Ordering {
    a.sort_key
        .unwrap_or(0)
        .cmp(&b.sort_key.unwrap_or(0))
        .then_with(|| a.id.cmp(&b.id))
}

fn edge_targets(edges: &[(EdgeKind, String)], kind: EdgeKind) -> Vec<String> {
    edges
        .iter()
        .filter(|(edge_kind, _)| *edge_kind == kind)
        .map(|(_, target)| target.clone())
        .collect()
}
