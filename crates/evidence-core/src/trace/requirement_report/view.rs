//! Canonical requirement-report projection over the corpus graph.

use std::collections::BTreeMap;

use crate::corpus::{CorpusGraph, EdgeKind, Node, RequirementLayer, TraceMetadata};

use super::RequirementReportError;

pub(super) struct ReportView {
    pub(super) sys: Vec<ReportRequirement>,
    pub(super) hlrs: Vec<ReportRequirement>,
    pub(super) llrs: Vec<ReportRequirement>,
    pub(super) tests: Vec<ReportTest>,
    children: BTreeMap<String, Vec<String>>,
}

pub(super) struct ReportRequirement {
    pub(super) uid: String,
    pub(super) id: String,
    pub(super) traces_to: Vec<String>,
    pub(super) verification_methods: Vec<String>,
    pub(super) link_gap: Option<String>,
    sort_key: Option<i64>,
}

pub(super) struct ReportTest {
    pub(super) uid: String,
    pub(super) id: String,
    pub(super) selectors: Vec<String>,
    pub(super) traces_to: Vec<String>,
    pub(super) link_gap: Option<String>,
    sort_key: Option<i64>,
}

impl ReportView {
    pub(super) fn from_graph(graph: &CorpusGraph) -> Result<Self, RequirementReportError> {
        let mut view = Self {
            sys: Vec::new(),
            hlrs: Vec::new(),
            llrs: Vec::new(),
            tests: Vec::new(),
            children: BTreeMap::new(),
        };

        for node in graph.nodes() {
            match node {
                Node::Requirement(requirement) => {
                    if matches!(
                        requirement.layer,
                        RequirementLayer::Source | RequirementLayer::Derived
                    ) {
                        continue;
                    }
                    let metadata = match graph.trace_metadata(&requirement.uid) {
                        Some(TraceMetadata::Requirement(metadata)) => Some(metadata),
                        Some(TraceMetadata::Test(_)) | None => None,
                    };
                    let (traces_to, link_gap) = requirement_links(graph, requirement)?;
                    let projected = ReportRequirement {
                        uid: requirement.uid.clone(),
                        id: requirement.id.clone(),
                        traces_to,
                        verification_methods: metadata
                            .map_or_else(Vec::new, |value| value.verification_methods.clone()),
                        link_gap,
                        sort_key: metadata.and_then(|value| value.sort_key),
                    };
                    match requirement.layer {
                        RequirementLayer::Sys => view.sys.push(projected),
                        RequirementLayer::Hlr => view.hlrs.push(projected),
                        RequirementLayer::Llr => view.llrs.push(projected),
                        RequirementLayer::Source | RequirementLayer::Derived => {}
                    }
                }
                Node::Test(test) => {
                    let metadata = match graph.trace_metadata(&test.uid) {
                        Some(TraceMetadata::Test(metadata)) => Some(metadata),
                        Some(TraceMetadata::Requirement(_)) | None => None,
                    };
                    let (traces_to, link_gap) = test_links(graph, test)?;
                    view.tests.push(ReportTest {
                        uid: test.uid.clone(),
                        id: test.id.clone(),
                        selectors: test.selectors.clone(),
                        traces_to,
                        link_gap,
                        sort_key: metadata.and_then(|value| value.sort_key),
                    });
                }
                // Review decisions are not requirement-report rows.
                Node::Review(_) => {}
            }
        }

        view.sys.sort_by(requirement_order);
        view.hlrs.sort_by(requirement_order);
        view.llrs.sort_by(requirement_order);
        view.tests.sort_by(test_order);
        view.index_children();
        Ok(view)
    }

    pub(super) fn children_of(&self, uid: &str) -> &[String] {
        self.children.get(uid).map_or(&[], Vec::as_slice)
    }

    pub(super) fn selector_inputs(&self) -> Vec<(String, Vec<String>)> {
        self.tests
            .iter()
            .map(|test| (test.id.clone(), test.selectors.clone()))
            .collect()
    }

    fn index_children(&mut self) {
        for requirement in self
            .hlrs
            .iter()
            .chain(self.llrs.iter())
            .chain(self.sys.iter())
        {
            if requirement.link_gap.is_none() {
                for parent in &requirement.traces_to {
                    self.children
                        .entry(parent.clone())
                        .or_default()
                        .push(requirement.uid.clone());
                }
            }
        }
        for test in &self.tests {
            if test.link_gap.is_none() {
                for parent in &test.traces_to {
                    self.children
                        .entry(parent.clone())
                        .or_default()
                        .push(test.uid.clone());
                }
            }
        }
        for children in self.children.values_mut() {
            children.sort();
        }
    }
}

fn requirement_links(
    graph: &CorpusGraph,
    requirement: &crate::corpus::RequirementNode,
) -> Result<(Vec<String>, Option<String>), RequirementReportError> {
    let expected = match requirement.layer {
        RequirementLayer::Sys => None,
        RequirementLayer::Hlr => Some(RequirementLayer::Sys),
        RequirementLayer::Llr => Some(RequirementLayer::Hlr),
        RequirementLayer::Source | RequirementLayer::Derived => return Ok((Vec::new(), None)),
    };
    let mut targets = Vec::new();
    let mut gap = None;
    for (kind, target_uid) in &requirement.edges {
        if *kind != EdgeKind::DerivesFrom {
            return Err(RequirementReportError::UnsupportedEdge {
                from: requirement.uid.clone(),
                kind: *kind,
            });
        }
        targets.push(target_uid.clone());
        if gap.is_some() {
            continue;
        }
        gap = match (expected, graph.get(target_uid)) {
            (None, _) => Some(format!(
                "SYS {} has an inapplicable parent edge to {}",
                requirement.id, target_uid
            )),
            (Some(_), None) => Some(format!(
                "{} {} links to missing requirement {}",
                layer_label(requirement.layer),
                requirement.id,
                target_uid
            )),
            (Some(expected_layer), Some(Node::Requirement(target)))
                if target.layer != expected_layer =>
            {
                Some(format!(
                    "{} {} links to {} layer; expected {}",
                    layer_label(requirement.layer),
                    requirement.id,
                    layer_label(target.layer),
                    layer_label(expected_layer)
                ))
            }
            (Some(_), Some(Node::Test(_))) => Some(format!(
                "{} {} parent edge targets a TEST node",
                layer_label(requirement.layer),
                requirement.id
            )),
            (Some(_), Some(Node::Review(_))) => Some(format!(
                "{} {} parent edge targets a REVIEW node",
                layer_label(requirement.layer),
                requirement.id
            )),
            (Some(_), Some(Node::Requirement(_))) => None,
        };
    }
    Ok((targets, gap))
}

fn test_links(
    graph: &CorpusGraph,
    test: &crate::corpus::TestNode,
) -> Result<(Vec<String>, Option<String>), RequirementReportError> {
    let mut targets = Vec::new();
    let mut gap = None;
    for (kind, target_uid) in &test.edges {
        if *kind != EdgeKind::Verifies {
            return Err(RequirementReportError::UnsupportedEdge {
                from: test.uid.clone(),
                kind: *kind,
            });
        }
        targets.push(target_uid.clone());
        if gap.is_some() {
            continue;
        }
        gap = match graph.get(target_uid) {
            None => Some(format!(
                "TEST {} links to missing requirement {}",
                test.id, target_uid
            )),
            Some(Node::Requirement(target)) if target.layer != RequirementLayer::Llr => {
                Some(format!(
                    "TEST {} verifies {} layer; expected LLR",
                    test.id,
                    layer_label(target.layer)
                ))
            }
            Some(Node::Test(_)) => Some(format!("TEST {} verifies a TEST node", test.id)),
            Some(Node::Review(_)) => Some(format!("TEST {} verifies a REVIEW node", test.id)),
            Some(Node::Requirement(_)) => None,
        };
    }
    Ok((targets, gap))
}

fn requirement_order(a: &ReportRequirement, b: &ReportRequirement) -> std::cmp::Ordering {
    a.sort_key
        .unwrap_or(0)
        .cmp(&b.sort_key.unwrap_or(0))
        .then_with(|| a.id.cmp(&b.id))
        .then_with(|| a.uid.cmp(&b.uid))
}

fn test_order(a: &ReportTest, b: &ReportTest) -> std::cmp::Ordering {
    a.sort_key
        .unwrap_or(0)
        .cmp(&b.sort_key.unwrap_or(0))
        .then_with(|| a.id.cmp(&b.id))
        .then_with(|| a.uid.cmp(&b.uid))
}

fn layer_label(layer: RequirementLayer) -> &'static str {
    match layer {
        RequirementLayer::Source => "SOURCE",
        RequirementLayer::Sys => "SYS",
        RequirementLayer::Hlr => "HLR",
        RequirementLayer::Llr => "LLR",
        RequirementLayer::Derived => "DERIVED",
    }
}
