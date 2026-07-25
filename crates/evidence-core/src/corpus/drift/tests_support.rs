//! Shared fixtures for the drift comparison unit tests
//! (TEST-192..TEST-194): a committed corpus with a section and
//! paragraph, an equal candidate plane, and patch/evaluation
//! builders. `#[cfg(test)]`-only; constructors panic on fixture
//! bugs.

use std::collections::{BTreeMap, BTreeSet};

use super::super::digest::StructuralContentDigest;
use super::super::graph::{CorpusGraph, Node};
use super::super::ingest::recipe::IngesterRecipe;
use super::super::patch_lifecycle::{PatchLifecycle, PatchLifecycleEvaluation};
use super::super::patch_testkit;
use super::super::source_graph::locator::{SafeRelPath, SourceLocator};
use super::super::source_graph::normalization::{content_digest, fingerprint};
use super::super::source_graph::{SourceGraph, SourceNode, SourceNodeKind};
use super::super::source_patch::PatchOperation;
use super::super::source_patch::digest::{reviewed_content_digest, source_graph_digest};
use super::super::source_patch::records::SourcePatchRecord;
use super::{DriftBaseline, DriftCategory, ReingestionCandidate};

pub(crate) const REVISION: &str = patch_testkit::REVISION;
pub(crate) const SEC: &str = "snode_00000000-0000-4000-8000-0000000000d1";
pub(crate) const PARA: &str = "snode_00000000-0000-4000-8000-0000000000d2";
pub(crate) const EXTRA: &str = "snode_00000000-0000-4000-8000-0000000000d3";
pub(crate) const PATCH_A: &str = patch_testkit::PATCH_A;

pub(crate) fn structural(hex: &str) -> StructuralContentDigest {
    patch_testkit::structural(hex)
}

pub(crate) fn fixture_recipe() -> IngesterRecipe {
    IngesterRecipe {
        parser: "pulldown-cmark".to_string(),
        parser_version: "0.13.4".to_string(),
        extensions: BTreeSet::new(),
        adapter_version: "1".to_string(),
        normalization_contract: "1".to_string(),
    }
}

pub(crate) fn locator(byte_range: (u64, u64)) -> SourceLocator {
    SourceLocator::Markdown {
        path: SafeRelPath::new("docs/doc.md").unwrap(),
        git_blob: None,
        anchor: None,
        heading_path: Vec::new(),
        byte_range,
    }
}

/// Build one node with digests computed from the already-built
/// ancestry in `graph`.
pub(crate) fn node(
    graph: &SourceGraph,
    uid: &str,
    parent: Option<&str>,
    kind: SourceNodeKind,
    ordinal: u32,
    label: Option<&str>,
    text: &str,
) -> SourceNode {
    let mut ancestry = Vec::new();
    let mut current = parent;
    while let Some(parent_uid) = current {
        let ancestor = graph.get(parent_uid).expect("parent built first");
        ancestry.push((ancestor.kind, ancestor.label.clone()));
        current = ancestor.parent_uid.as_deref();
    }
    ancestry.reverse();
    let ancestry_refs: Vec<(SourceNodeKind, Option<&str>)> = ancestry
        .iter()
        .map(|(kind, label)| (*kind, label.as_deref()))
        .collect();
    SourceNode {
        uid: uid.to_string(),
        source_revision_uid: REVISION.to_string(),
        parent_uid: parent.map(str::to_string),
        kind,
        ordinal,
        label: label.map(str::to_string),
        canonical_text: text.to_string(),
        content_sha256: content_digest(kind, text),
        fingerprint: fingerprint(kind, label, &ancestry_refs),
        locator: locator((0, 10)),
    }
}

/// The base parser graph: one section with one paragraph child.
pub(crate) fn base_graph() -> SourceGraph {
    let mut graph = SourceGraph::new();
    graph
        .insert(node(
            &graph,
            SEC,
            None,
            SourceNodeKind::Section,
            0,
            Some("1 Intro"),
            "",
        ))
        .unwrap();
    graph
        .insert(node(
            &graph,
            PARA,
            Some(SEC),
            SourceNodeKind::Paragraph,
            0,
            None,
            "hello",
        ))
        .unwrap();
    graph
}

/// A `replace_content` patch bound to `graph` and the fixture
/// recipe, with a recomputed reviewed-content digest.
pub(crate) fn patch_for(graph: &SourceGraph, new_text: &str) -> SourcePatchRecord {
    let paragraph = graph.get(PARA).expect("paragraph present");
    let mut record = SourcePatchRecord {
        uid: PATCH_A.to_string(),
        human_id: "fix-para".to_string(),
        source_revision_uid: REVISION.to_string(),
        recipe_digest: fixture_recipe().digest(),
        input_digest: structural(patch_testkit::INPUT_HEX),
        pre_patch_graph_digest: source_graph_digest(graph),
        reviewed_content_digest: structural(&"0".repeat(64)),
        author: "curator@example.com".to_string(),
        rationale: "fix the paragraph".to_string(),
        created_at: "2026-07-01T10:00:00Z".to_string(),
        operations: vec![PatchOperation::ReplaceContent {
            ordinal: 0,
            target_uid: PARA.to_string(),
            expected_content_sha256: paragraph.content_sha256.clone(),
            new_canonical_text: Some(new_text.to_string()),
            new_label: None,
        }],
    };
    record.reviewed_content_digest = reviewed_content_digest(&record);
    record
}

/// One evaluation entry for `patch`.
pub(crate) fn evaluation(
    patch: &SourcePatchRecord,
    state: PatchLifecycle,
) -> PatchLifecycleEvaluation {
    PatchLifecycleEvaluation {
        patch_uid: patch.uid.clone(),
        state,
        current_digest: patch.reviewed_content_digest.clone(),
        effective_review_uids: Vec::new(),
    }
}

/// A committed corpus holding the base graph, optionally with the
/// patch and an approving review (so the committed effective graph
/// applies it).
pub(crate) fn committed_corpus(
    graph: &SourceGraph,
    approved_patch: Option<&SourcePatchRecord>,
) -> CorpusGraph {
    let mut corpus = CorpusGraph::new();
    corpus.insert(patch_testkit::revision_node()).unwrap();
    for source_node in graph.nodes() {
        corpus.insert_source_node(source_node.clone()).unwrap();
    }
    if let Some(patch) = approved_patch {
        corpus.insert_source_patch(patch.clone()).unwrap();
        corpus
            .insert(Node::Review(patch_testkit::patch_review(
                patch_testkit::REV_1,
                "REV-1",
                &patch.uid,
                patch.reviewed_content_digest.as_str(),
                super::super::graph::ReviewDecision::Approve,
                "reviewer@example.com",
                None,
            )))
            .unwrap();
    }
    corpus
}

/// The equal-planes inputs over a corpus without patches.
pub(crate) fn fixture() -> (
    CorpusGraph,
    SourceGraph,
    StructuralContentDigest,
    BTreeMap<String, PatchLifecycleEvaluation>,
) {
    let graph = base_graph();
    let corpus = committed_corpus(&graph, None);
    let input = structural(patch_testkit::INPUT_HEX);
    (corpus, graph, input, BTreeMap::new())
}

pub(crate) fn make_baseline<'a>(
    corpus: &'a CorpusGraph,
    recipe_digest: StructuralContentDigest,
    input_digest: StructuralContentDigest,
    evaluations: &'a BTreeMap<String, PatchLifecycleEvaluation>,
) -> DriftBaseline<'a> {
    DriftBaseline {
        corpus,
        source_revision_uid: REVISION,
        recipe_digest,
        input_digest,
        extractor_output_digest: None,
        patch_evaluations: evaluations,
    }
}

pub(crate) fn make_candidate<'a>(
    recipe: &'a IngesterRecipe,
    input_digest: StructuralContentDigest,
    graph: &'a SourceGraph,
    patches: &'a [SourcePatchRecord],
    evaluations: &'a BTreeMap<String, PatchLifecycleEvaluation>,
) -> ReingestionCandidate<'a> {
    ReingestionCandidate {
        source_document: "docs/doc.md",
        recipe: Some(recipe),
        verified_input_digest: Some(input_digest),
        extractor_output_digest: None,
        parser_graph: graph,
        patches,
        patch_evaluations: evaluations,
    }
}

pub(crate) fn categories(report: &super::DriftReport) -> BTreeSet<DriftCategory> {
    report
        .findings
        .iter()
        .map(|finding| finding.category)
        .collect()
}
