//! Curated source-graph patch acceptance tests (TEST-185,
//! TEST-187): the committed PICS-shaped fixture byte-locks its
//! golden canonical patch bytes and reviewed-content digest, the
//! correction restores the intended table ancestry and canonical
//! digests, and the parser, patch, and candidate planes stay
//! separately inspectable.
//!
//! The fixture models a parser-hostile PICS table: the raw parser
//! graph loses one table relationship — a data cell lands as a
//! direct child of the section instead of its row — and the
//! committed curated patch restores the intended row/cell
//! structure with one reparent operation. It demonstrates the
//! patch contract without claiming PDF ingestion. The golden
//! `pics_patch_v1.golden` byte-locks the canonical reviewed-content
//! bytes plus the pre/post canonical graph digests; regenerate
//! with `EVIDENCE_UPDATE_FIXTURES=1`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use evidence_core::corpus::{
    CorpusIndex, PatchApplication, PatchBindings, PatchOperation, SafeRelPath, SourceGraph,
    SourceLocator, SourceNode, SourceNodeKind, SourcePatchRecord, StructuralContentDigest,
    apply_patch, content_digest, fingerprint, parse_source_patch, reviewed_content_bytes,
    reviewed_content_digest, source_graph_digest,
};
use evidence_core::hash::sha256;

const REV: &str = "src_00000000-0000-4000-8000-0000000000a1";
const PATCH_UID: &str = "patch_00000000-0000-4000-8000-0000000000a1";
const SEC: &str = "snode_00000000-0000-4000-8000-0000000000a1";
const TBL: &str = "snode_00000000-0000-4000-8000-0000000000a2";
const ROW1: &str = "snode_00000000-0000-4000-8000-0000000000a3";
const C11: &str = "snode_00000000-0000-4000-8000-0000000000a4";
const C12: &str = "snode_00000000-0000-4000-8000-0000000000a5";
const ROW2: &str = "snode_00000000-0000-4000-8000-0000000000a6";
const C21: &str = "snode_00000000-0000-4000-8000-0000000000a7";
const RECIPE_HEX: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const INPUT_HEX: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const MEDIA: &str = "text/markdown";

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus")
}

fn locator(byte_range: (u64, u64)) -> SourceLocator {
    SourceLocator::Markdown {
        path: SafeRelPath::new("docs/pics.md").expect("safe path"),
        git_blob: None,
        anchor: None,
        heading_path: Vec::new(),
        byte_range,
    }
}

fn node(
    graph: &SourceGraph,
    uid: &str,
    parent: Option<&str>,
    kind: SourceNodeKind,
    ordinal: u32,
    text: &str,
) -> SourceNode {
    let mut ancestry = Vec::new();
    let mut visited = BTreeSet::new();
    let mut current = parent;
    while let Some(parent_uid) = current {
        let ancestor = graph.get(parent_uid).expect("fixture parents resolve");
        assert!(visited.insert(parent_uid), "fixture ancestry is acyclic");
        ancestry.push((ancestor.kind, ancestor.label.clone()));
        current = ancestor.parent_uid.as_deref();
    }
    ancestry.reverse();
    let pair_refs: Vec<(SourceNodeKind, Option<&str>)> = ancestry
        .iter()
        .map(|(kind, label)| (*kind, label.as_deref()))
        .collect();
    SourceNode {
        uid: uid.to_string(),
        source_revision_uid: REV.to_string(),
        parent_uid: parent.map(str::to_string),
        kind,
        ordinal,
        label: None,
        canonical_text: text.to_string(),
        content_sha256: content_digest(kind, text),
        fingerprint: fingerprint(kind, None, &pair_refs),
        locator: locator((0, 10)),
    }
}

/// The raw parser graph of the PICS-shaped fixture: the parser
/// lost the `C21` row/cell relationship — the data cell landed as
/// a direct child of the section, a sibling of the table.
fn raw_pics_graph() -> SourceGraph {
    let mut graph = SourceGraph::new();
    let rows = [
        (SEC, None, SourceNodeKind::Section, 0, "PICS"),
        (TBL, Some(SEC), SourceNodeKind::Table, 0, "capabilities"),
        (ROW1, Some(TBL), SourceNodeKind::TableRow, 0, "header"),
        (C11, Some(ROW1), SourceNodeKind::TableCell, 0, "Requirement"),
        (C12, Some(ROW1), SourceNodeKind::TableCell, 1, "Support"),
        (ROW2, Some(TBL), SourceNodeKind::TableRow, 1, "data"),
        (
            C21,
            Some(SEC),
            SourceNodeKind::TableCell,
            1,
            "SYS-054 curated patches",
        ),
    ];
    for (uid, parent, kind, ordinal, text) in rows {
        graph
            .insert(node(&graph, uid, parent, kind, ordinal, text))
            .expect("fixture graph inserts");
    }
    graph
}

/// The curated patch correcting the raw parse: one reparent
/// restoring `C21` under its row.
fn pics_patch(graph: &SourceGraph) -> SourcePatchRecord {
    let mut record = SourcePatchRecord {
        uid: PATCH_UID.to_string(),
        human_id: "pics-table-row-restore".to_string(),
        source_revision_uid: REV.to_string(),
        recipe_digest: StructuralContentDigest::from_hex(RECIPE_HEX).expect("hex"),
        input_digest: StructuralContentDigest::from_hex(INPUT_HEX).expect("hex"),
        pre_patch_graph_digest: source_graph_digest(graph),
        reviewed_content_digest: StructuralContentDigest::from_hex(RECIPE_HEX).expect("hex"),
        author: "curator@example.com".to_string(),
        rationale: "the parser lost the row/cell relationship of the PICS table".to_string(),
        created_at: "2026-07-25T00:00:00Z".to_string(),
        operations: vec![PatchOperation::Reparent {
            ordinal: 0,
            target_uid: C21.to_string(),
            expected_parent_uid: Some(SEC.to_string()),
            expected_ordinal: 1,
            new_parent_uid: Some(ROW2.to_string()),
            new_ordinal: 0,
        }],
    };
    record.reviewed_content_digest = reviewed_content_digest(&record);
    record
}

fn bindings() -> PatchBindings {
    PatchBindings {
        recipe_digest: StructuralContentDigest::from_hex(RECIPE_HEX).expect("hex"),
        input_digest: StructuralContentDigest::from_hex(INPUT_HEX).expect("hex"),
    }
}

/// The deterministic TOML form of the fixture patch record: the
/// committed fixture file's exact expected content.
fn patch_toml(record: &SourcePatchRecord) -> String {
    let mut out = String::from("schema_version = 1\n\n[patch]\n");
    out.push_str(&format!("uid = \"{}\"\n", record.uid));
    out.push_str(&format!("human_id = \"{}\"\n", record.human_id));
    out.push_str(&format!(
        "source_revision_uid = \"{}\"\n",
        record.source_revision_uid
    ));
    out.push_str(&format!("recipe_digest = \"{}\"\n", record.recipe_digest));
    out.push_str(&format!("input_digest = \"{}\"\n", record.input_digest));
    out.push_str(&format!(
        "pre_patch_graph_digest = \"{}\"\n",
        record.pre_patch_graph_digest
    ));
    out.push_str(&format!(
        "reviewed_content_digest = \"{}\"\n",
        record.reviewed_content_digest
    ));
    out.push_str(&format!("author = \"{}\"\n", record.author));
    out.push_str(&format!("rationale = \"{}\"\n", record.rationale));
    out.push_str(&format!("created_at = \"{}\"\n", record.created_at));
    for operation in &record.operations {
        let PatchOperation::Reparent {
            ordinal,
            target_uid,
            expected_parent_uid,
            expected_ordinal,
            new_parent_uid,
            new_ordinal,
        } = operation
        else {
            panic!("the PICS fixture patch is one reparent operation");
        };
        out.push_str("\n[[patch.operations]]\n");
        out.push_str("op = \"reparent\"\n");
        out.push_str(&format!("ordinal = {ordinal}\n"));
        out.push_str(&format!("target_uid = \"{target_uid}\"\n"));
        if let Some(parent) = expected_parent_uid {
            out.push_str(&format!("expected_parent_uid = \"{parent}\"\n"));
        }
        out.push_str(&format!("expected_ordinal = {expected_ordinal}\n"));
        if let Some(parent) = new_parent_uid {
            out.push_str(&format!("new_parent_uid = \"{parent}\"\n"));
        }
        out.push_str(&format!("new_ordinal = {new_ordinal}\n"));
    }
    out
}

/// The golden payload: the canonical reviewed-content bytes, then
/// a digest trailer.
fn golden_bytes(record: &SourcePatchRecord, application: &PatchApplication) -> Vec<u8> {
    let mut bytes = reviewed_content_bytes(record);
    bytes.extend_from_slice(
        format!(
            "\n-- digests --\npre_patch_graph_digest = \"{}\"\npost_patch_graph_digest = \"{}\"\n",
            application.pre_patch_digest, application.post_patch_digest
        )
        .as_bytes(),
    );
    bytes
}

fn apply_fixture() -> (SourcePatchRecord, PatchApplication) {
    let graph = raw_pics_graph();
    let record = pics_patch(&graph);
    let application = apply_patch(&graph, &record, &bindings(), MEDIA).expect("patch applies");
    (record, application)
}

/// The committed fixture parses to the expected record, and the
/// canonical reviewed-content bytes, the reviewed-content digest,
/// and the pre/post canonical graph digests are byte-locked
/// (TEST-185).
#[test]
fn golden_canonical_patch_bytes_and_reviewed_content_digest() {
    let graph = raw_pics_graph();
    let record = pics_patch(&graph);
    let application = apply_patch(&graph, &record, &bindings(), MEDIA).expect("patch applies");
    let fixture_path = fixture_dir().join("pics_patch_v1.toml");
    let golden_path = fixture_dir().join("pics_patch_v1.golden");
    if std::env::var_os("EVIDENCE_UPDATE_FIXTURES").is_some() {
        fs::write(&fixture_path, patch_toml(&record)).expect("write fixture");
        fs::write(&golden_path, golden_bytes(&record, &application)).expect("write golden");
    }
    let fixture_raw = fs::read_to_string(&fixture_path).expect("read fixture");
    let parsed = parse_source_patch(&fixture_path, &fixture_raw).expect("fixture parses");
    assert_eq!(
        parsed, record,
        "the committed fixture is the expected patch"
    );

    let golden = fs::read(&golden_path).expect("read golden");
    let expected = golden_bytes(&record, &application);
    assert_eq!(
        golden, expected,
        "canonical patch bytes and graph digests drifted; regenerate with EVIDENCE_UPDATE_FIXTURES=1"
    );
    assert_eq!(
        reviewed_content_digest(&record).as_str(),
        sha256(&reviewed_content_bytes(&record)),
        "the reviewed-content digest covers the canonical bytes"
    );
}

/// The PICS-shaped correction restores the intended table ancestry
/// and the recorded canonical digests (TEST-187).
#[test]
fn pics_shaped_correction_restores_table_ancestry_and_canonical_digest() {
    let graph = raw_pics_graph();
    assert_eq!(
        graph.get(C21).expect("cell exists").parent_uid.as_deref(),
        Some(SEC),
        "the raw parse lost the row/cell relationship"
    );
    let (record, application) = apply_fixture();
    let corrected = application.graph.get(C21).expect("cell survives");
    assert_eq!(
        corrected.parent_uid.as_deref(),
        Some(ROW2),
        "the patch restores the intended row/cell structure"
    );
    assert_eq!(corrected.ordinal, 0);
    let row2 = application.graph.get(ROW2).expect("row survives");
    assert_eq!(row2.parent_uid.as_deref(), Some(TBL));
    assert_eq!(application.pre_patch_digest, source_graph_digest(&graph));
    assert_eq!(application.patch_digest, record.reviewed_content_digest);
    assert_eq!(
        application.post_patch_digest,
        source_graph_digest(&application.graph),
        "the recorded post-patch digest recomputes from the candidate graph"
    );
    assert_ne!(application.pre_patch_digest, application.post_patch_digest);
}

/// The parser graph, the patch plane, and the candidate
/// application result stay separately inspectable through the
/// committed corpus (TEST-187).
#[test]
fn parser_patch_and_candidate_planes_are_separately_inspectable() {
    let graph = raw_pics_graph();
    let record = pics_patch(&graph);
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        &dir.path().join("sources/records.toml"),
        &format!(
            r#"schema_version = 1

[[sources]]
uid = "{REV}"
id = "PICS-DOC"
document_key = "PICS"
title = "PICS fixture"
media_type = "{MEDIA}"
canonical_location = "https://example.org/pics/rev-a"

[sources.material]
state = "unavailable"
reason = "fixture"
"#
        ),
    );
    write(
        &dir.path().join("graphs/records.toml"),
        &graph_file_toml(&graph),
    );
    write(&dir.path().join("patches/pics.toml"), &patch_toml(&record));
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources/**/*.toml\"]\nsource_graphs = [\"graphs/**/*.toml\"]\nsource_patches = [\"patches/**/*.toml\"]\n",
    );
    let corpus = CorpusIndex::load_graph(&dir.path().join("corpus.toml")).expect("corpus loads");

    let parser_plane = corpus.source_graph(REV).expect("parser plane");
    assert_eq!(
        parser_plane.get(C21).expect("cell").parent_uid.as_deref(),
        Some(SEC),
        "the committed parser plane keeps the raw extraction"
    );
    let patch_plane = corpus.source_patch(PATCH_UID).expect("patch plane");
    assert_eq!(
        patch_plane, &record,
        "the patch plane is inspectable as data"
    );
    assert_eq!(
        corpus.source_patches().len(),
        1,
        "the patch plane sits beside the parser graphs"
    );

    let application = apply_patch(parser_plane, patch_plane, &bindings(), MEDIA).expect("apply");
    assert_eq!(
        application
            .graph
            .get(C21)
            .expect("cell")
            .parent_uid
            .as_deref(),
        Some(ROW2),
        "the candidate plane carries the correction"
    );
    assert_eq!(
        corpus
            .source_graph(REV)
            .expect("parser plane")
            .get(C21)
            .expect("cell")
            .parent_uid
            .as_deref(),
        Some(SEC),
        "candidate application never mutates the committed parser plane"
    );
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, content).expect("write file");
}

/// Serialize the raw fixture graph as a strict source-graph record
/// file the corpus loader accepts.
fn graph_file_toml(graph: &SourceGraph) -> String {
    let mut out = String::from("schema_version = 1\n");
    for node in graph.nodes() {
        out.push_str("\n[[nodes]]\n");
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
        out.push_str(&format!("canonical_text = \"{}\"\n", node.canonical_text));
        out.push_str(&format!("content_sha256 = \"{}\"\n", node.content_sha256));
        out.push_str(&format!("fingerprint = \"{}\"\n", node.fingerprint));
        out.push_str("\n[nodes.locator]\n");
        out.push_str("format = \"markdown\"\n");
        out.push_str("path = \"docs/pics.md\"\n");
        out.push_str("byte_range = [0, 10]\n");
    }
    out
}
