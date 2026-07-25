//! Shared fixtures for the re-ingestion drift comparison
//! integration tests (TEST-192..TEST-194): a committed Markdown
//! corpus — one section with a paragraph and a note, plus one
//! approved `replace_content` patch on the paragraph — written as
//! two equivalent linked layouts, candidate-plane builders, and
//! TOML serializers.
//!
//! Included into each drift test binary via
//! `#[path = "corpus_drift_testkit.rs"] mod testkit;`, mirroring
//! `helpers.rs`; every item is `pub` with a crate-level dead-code
//! allowance so each file imports only what it needs.

#![allow(
    dead_code,
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers may be partially used by any given test binary"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use evidence_core::corpus::{
    CorpusGraph, CorpusIndex, DriftBaseline, DriftCategory, DriftFinding, IngesterRecipe,
    PatchLifecycleEvaluation, ReingestionCandidate, SafeRelPath, SourceGraph, SourceLocator,
    SourceNode, SourceNodeKind, SourcePatchRecord, StructuralContentDigest, content_digest,
    fingerprint, reviewed_content_digest, source_graph_digest,
};

pub const REVISION: &str = "src_00000000-0000-4000-8000-0000000000a1";
pub const SEC: &str = "snode_00000000-0000-4000-8000-0000000000d1";
pub const PARA: &str = "snode_00000000-0000-4000-8000-0000000000d2";
pub const NOTE: &str = "snode_00000000-0000-4000-8000-0000000000d3";
pub const EXTRA: &str = "snode_00000000-0000-4000-8000-0000000000d4";
pub const PATCH_A: &str = "patch_00000000-0000-4000-8000-0000000000b1";
pub const REV_1: &str = "rev_00000000-0000-4000-8000-0000000000a1";
pub const REV_2: &str = "rev_00000000-0000-4000-8000-0000000000a2";
pub const INPUT_HEX: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

pub fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus")
}

pub fn structural(hex: &str) -> StructuralContentDigest {
    StructuralContentDigest::from_hex(hex).expect("hex")
}

pub fn fixture_recipe() -> IngesterRecipe {
    IngesterRecipe {
        parser: "pulldown-cmark".to_string(),
        parser_version: "0.13.4".to_string(),
        extensions: BTreeSet::new(),
        adapter_version: "1".to_string(),
        normalization_contract: "1".to_string(),
    }
}

pub fn locator(byte_range: (u64, u64)) -> SourceLocator {
    SourceLocator::Markdown {
        path: SafeRelPath::new("docs/doc.md").expect("safe path"),
        git_blob: None,
        anchor: None,
        heading_path: Vec::new(),
        byte_range,
    }
}

/// One fixture-node specification, in build order (parents before
/// children).
pub struct NodeSpec<'a> {
    pub uid: &'a str,
    pub parent: Option<&'a str>,
    pub kind: SourceNodeKind,
    pub ordinal: u32,
    pub label: Option<&'a str>,
    pub text: &'a str,
    pub byte_range: (u64, u64),
}

/// One fixture node; parents come earlier in `built`.
pub fn node(built: &[SourceNode], spec: &NodeSpec) -> SourceNode {
    let mut ancestry = Vec::new();
    let mut current = spec.parent;
    while let Some(parent_uid) = current {
        let ancestor = built
            .iter()
            .find(|node| node.uid == parent_uid)
            .expect("parent built first");
        ancestry.push((ancestor.kind, ancestor.label.clone()));
        current = ancestor.parent_uid.as_deref();
    }
    ancestry.reverse();
    let ancestry_refs: Vec<(SourceNodeKind, Option<&str>)> = ancestry
        .iter()
        .map(|(kind, label)| (*kind, label.as_deref()))
        .collect();
    SourceNode {
        uid: spec.uid.to_string(),
        source_revision_uid: REVISION.to_string(),
        parent_uid: spec.parent.map(str::to_string),
        kind: spec.kind,
        ordinal: spec.ordinal,
        label: spec.label.map(str::to_string),
        canonical_text: spec.text.to_string(),
        content_sha256: content_digest(spec.kind, spec.text),
        fingerprint: fingerprint(spec.kind, spec.label, &ancestry_refs),
        locator: locator(spec.byte_range),
    }
}

/// The fixture forest: one section with a paragraph and a note.
pub fn base_nodes() -> Vec<SourceNode> {
    let section = node(
        &[],
        &NodeSpec {
            uid: SEC,
            parent: None,
            kind: SourceNodeKind::Section,
            ordinal: 0,
            label: Some("1 Intro"),
            text: "",
            byte_range: (0, 50),
        },
    );
    let paragraph = node(
        std::slice::from_ref(&section),
        &NodeSpec {
            uid: PARA,
            parent: Some(SEC),
            kind: SourceNodeKind::Paragraph,
            ordinal: 0,
            label: None,
            text: "hello",
            byte_range: (51, 60),
        },
    );
    let note = node(
        &[section.clone(), paragraph.clone()],
        &NodeSpec {
            uid: NOTE,
            parent: Some(SEC),
            kind: SourceNodeKind::Note,
            ordinal: 1,
            label: None,
            text: "a note",
            byte_range: (61, 70),
        },
    );
    vec![section, paragraph, note]
}

pub fn graph_from(nodes: Vec<SourceNode>) -> SourceGraph {
    let mut graph = SourceGraph::new();
    for node in nodes {
        graph.insert(node).expect("fixture graph inserts");
    }
    graph
}

/// The fixture patch: `replace_content` on the paragraph, bound to
/// the committed graph and the fixture recipe and input.
pub fn patch_record() -> SourcePatchRecord {
    let paragraph = graph_from(base_nodes());
    let expected = paragraph
        .get(PARA)
        .expect("paragraph")
        .content_sha256
        .clone();
    let mut record = SourcePatchRecord {
        uid: PATCH_A.to_string(),
        human_id: "fix-paragraph".to_string(),
        source_revision_uid: REVISION.to_string(),
        recipe_digest: fixture_recipe().digest(),
        input_digest: structural(INPUT_HEX),
        pre_patch_graph_digest: source_graph_digest(&paragraph),
        reviewed_content_digest: structural(&"0".repeat(64)),
        author: "curator@example.com".to_string(),
        rationale: "restore the intended wording".to_string(),
        created_at: "2026-07-01T10:00:00Z".to_string(),
        operations: vec![evidence_core::corpus::PatchOperation::ReplaceContent {
            ordinal: 0,
            target_uid: PARA.to_string(),
            expected_content_sha256: expected,
            new_canonical_text: Some("hello!".to_string()),
            new_label: None,
        }],
    };
    record.reviewed_content_digest = reviewed_content_digest(&record);
    record
}

pub fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(path, content).expect("write");
}

pub fn source_toml() -> String {
    format!(
        "schema_version = 1\n\n[[sources]]\nuid = \"{REVISION}\"\nid = \"DOC-1\"\n\
         document_key = \"doc\"\ntitle = \"fixture document\"\nmedia_type = \"text/markdown\"\n\
         canonical_location = \"https://example.org/doc/rev-a\"\n\n\
         [sources.material]\nstate = \"unavailable\"\nreason = \"fixture\"\n"
    )
}

pub fn node_toml(node: &SourceNode) -> String {
    let mut out = String::from("[[nodes]]\n");
    out.push_str(&format!("uid = \"{}\"\n", node.uid));
    out.push_str(&format!(
        "source_revision_uid = \"{}\"\n",
        node.source_revision_uid
    ));
    if let Some(parent) = &node.parent_uid {
        out.push_str(&format!("parent_uid = \"{parent}\"\n"));
    }
    out.push_str(&format!("kind = \"{}\"\n", node.kind.as_str()));
    out.push_str(&format!("ordinal = {}\n", node.ordinal));
    if let Some(label) = &node.label {
        out.push_str(&format!("label = \"{label}\"\n"));
    }
    out.push_str(&format!("canonical_text = \"{}\"\n", node.canonical_text));
    out.push_str(&format!("content_sha256 = \"{}\"\n", node.content_sha256));
    out.push_str(&format!("fingerprint = \"{}\"\n", node.fingerprint));
    let SourceLocator::Markdown { byte_range, .. } = &node.locator else {
        panic!("fixture nodes carry markdown locators only");
    };
    out.push_str(&format!(
        "\n[nodes.locator]\nformat = \"markdown\"\npath = \"docs/doc.md\"\n\
         byte_range = [{}, {}]\n",
        byte_range.0, byte_range.1
    ));
    out
}

pub fn graphs_toml(nodes: &[SourceNode]) -> String {
    let mut out = String::from("schema_version = 1\n\n");
    for node in nodes {
        out.push_str(&node_toml(node));
        out.push('\n');
    }
    out
}

pub fn patch_toml(record: &SourcePatchRecord) -> String {
    format!(
        "schema_version = 1\n\n[patch]\nuid = \"{}\"\nhuman_id = \"{}\"\n\
         source_revision_uid = \"{}\"\nrecipe_digest = \"{}\"\ninput_digest = \"{}\"\n\
         pre_patch_graph_digest = \"{}\"\nreviewed_content_digest = \"{}\"\n\
         author = \"{}\"\nrationale = \"{}\"\ncreated_at = \"{}\"\n\
         \n[[patch.operations]]\nop = \"replace_content\"\nordinal = 0\n\
         target_uid = \"{PARA}\"\nexpected_content_sha256 = \"{}\"\n\
         new_canonical_text = \"hello!\"\n",
        record.uid,
        record.human_id,
        record.source_revision_uid,
        record.recipe_digest,
        record.input_digest,
        record.pre_patch_graph_digest,
        record.reviewed_content_digest,
        record.author,
        record.rationale,
        record.created_at,
        graph_from(base_nodes())
            .get(PARA)
            .expect("paragraph")
            .content_sha256,
    )
}

pub fn review_toml(uid: &str, id: &str, reviewer: &str, digest: &str) -> String {
    format!(
        "\n[[reviews]]\nuid = \"{uid}\"\nid = \"{id}\"\n\
         target = {{ kind = \"curated_patch\", uid = \"{PATCH_A}\" }}\n\
         content_schema = 1\nreviewed_content_sha256 = \"{digest}\"\ndecision = \"approve\"\n\
         reviewer = \"{reviewer}\"\nreviewed_at = \"2026-07-01T10:00:00Z\"\n"
    )
}

/// Write the fixture corpus in the given layout and load it.
/// `split` selects layout B: graphs and reviews split across
/// directories with records reversed.
pub fn load_corpus(split: bool) -> CorpusGraph {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let patch = patch_record();
    let digest = patch.reviewed_content_digest.as_str().to_string();
    let review_1 = review_toml(REV_1, "REV-001", "alice@example.com", &digest);
    let review_2 = review_toml(REV_2, "REV-002", "bob@example.com", &digest);
    if split {
        write(&root.join("sources/nested/records.toml"), &source_toml());
        let nodes = base_nodes();
        write(&root.join("graphs/z.toml"), &graphs_toml(&nodes[..2]));
        write(&root.join("graphs/a.toml"), &graphs_toml(&nodes[2..]));
        write(&root.join("patches/nested/p.toml"), &patch_toml(&patch));
        write(
            &root.join("reviews/second.toml"),
            &format!("schema_version = 2\n{review_2}"),
        );
        write(
            &root.join("reviews/first.toml"),
            &format!("schema_version = 2\n{review_1}"),
        );
        write(
            &root.join("corpus.toml"),
            "schema_version = 1\nsources = [\"sources/**/*.toml\"]\n\
             source_graphs = [\"graphs/**/*.toml\"]\n\
             source_patches = [\"patches/**/*.toml\"]\nreviews = [\"reviews/**/*.toml\"]\n",
        );
    } else {
        write(&root.join("sources.toml"), &source_toml());
        write(&root.join("graphs.toml"), &graphs_toml(&base_nodes()));
        write(&root.join("patches.toml"), &patch_toml(&patch));
        write(
            &root.join("reviews.toml"),
            &format!("schema_version = 2\n{review_1}{review_2}"),
        );
        write(
            &root.join("corpus.toml"),
            "schema_version = 1\nsources = [\"sources.toml\"]\n\
             source_graphs = [\"graphs.toml\"]\nsource_patches = [\"patches.toml\"]\n\
             reviews = [\"reviews.toml\"]\n",
        );
    }
    CorpusIndex::load_graph(&root.join("corpus.toml")).expect("fixture corpus loads")
}

pub fn committed_patches(corpus: &CorpusGraph) -> Vec<SourcePatchRecord> {
    corpus.source_patches().values().cloned().collect()
}

pub fn baseline<'a>(
    corpus: &'a CorpusGraph,
    evaluations: &'a BTreeMap<String, PatchLifecycleEvaluation>,
) -> DriftBaseline<'a> {
    DriftBaseline {
        corpus,
        source_revision_uid: REVISION,
        recipe_digest: fixture_recipe().digest(),
        input_digest: structural(INPUT_HEX),
        patch_evaluations: evaluations,
    }
}

pub fn candidate<'a>(
    recipe: &'a IngesterRecipe,
    graph: &'a SourceGraph,
    patches: &'a [SourcePatchRecord],
    evaluations: &'a BTreeMap<String, PatchLifecycleEvaluation>,
) -> ReingestionCandidate<'a> {
    ReingestionCandidate {
        source_document: "docs/doc.md",
        recipe: Some(recipe),
        verified_input_digest: Some(structural(INPUT_HEX)),
        parser_graph: graph,
        patches,
        patch_evaluations: evaluations,
    }
}

pub fn categories(findings: &[DriftFinding]) -> BTreeSet<DriftCategory> {
    findings.iter().map(|finding| finding.category).collect()
}
