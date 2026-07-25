//! Shared fixture helpers for the PDF ingestion acceptance
//! tests: fixture loading, the recipe, golden comparison, and the
//! on-disk PICS corpus (raw parser graph, curated patch, optional
//! approval review).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use evidence_core::corpus::{
    CorpusGraph, CorpusIndex, IngestPdfInput, InsertedNodeSpec, PatchOperation, PdfIngestion,
    PdfIngestionRecipe, PdfLayoutRules, PdfToolLock, SourceGraph, SourceNodeKind,
    SourcePatchRecord, StructuralContentDigest, ingest_pdf, reviewed_content_digest,
    source_graph_digest,
};
use evidence_core::hash::sha256;

pub const REV: &str = "src_00000000-0000-4000-8000-0000000000d2";
pub const MEDIA: &str = "application/pdf";
pub const CANONICAL: &str = "vendor/spec/pics-like.pdf";
pub const PATCH_UID: &str = "patch_00000000-0000-4000-8000-0000000000d1";
pub const REVIEW_UID: &str = "rev_00000000-0000-4000-8000-0000000000d1";
pub const TBL: &str = "snode_00000000-0000-4000-8000-0000000000e1";
pub const ROW1: &str = "snode_00000000-0000-4000-8000-0000000000e2";
pub const ROW2: &str = "snode_00000000-0000-4000-8000-0000000000e3";

pub fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus")
}

pub fn fixture_bytes(name: &str) -> Vec<u8> {
    fs::read(fixture_dir().join(name)).expect("read fixture")
}

pub fn recipe() -> PdfIngestionRecipe {
    let raw =
        fs::read_to_string(fixture_dir().join("pdf_tool_lock_v1.toml")).expect("read tool lock");
    PdfIngestionRecipe {
        tool_lock: PdfToolLock::from_toml(&raw).expect("tool lock fixture validates"),
        rules: PdfLayoutRules {
            header_bottom: 50.0,
            footer_top: 740.0,
            column_split_x: Some(306.0),
            max_heading_depth: 2,
            note_prefixes: BTreeSet::from(["NOTE".to_string()]),
            caption_prefixes: BTreeSet::from(["Figure".to_string()]),
            page_label_prefix: Some("Page ".to_string()),
        },
    }
}

pub fn input<'a>(pdf: &'a [u8], xhtml: &'a [u8]) -> IngestPdfInput<'a> {
    IngestPdfInput {
        bytes: pdf,
        media_type: MEDIA,
        source_revision_uid: REV,
        canonical_path: CANONICAL,
        input_digest: StructuralContentDigest::from_hex(&sha256(pdf)).expect("sha256 hex"),
        extractor_output: xhtml,
        recipe: recipe(),
    }
}

pub fn ingest(name_pdf: &str, name_xhtml: &str) -> PdfIngestion {
    let pdf = fixture_bytes(name_pdf);
    let xhtml = fixture_bytes(name_xhtml);
    ingest_pdf(&input(&pdf, &xhtml)).expect("fixture ingestion succeeds")
}

/// Compare bytes against a golden, regenerating under
/// `EVIDENCE_UPDATE_FIXTURES=1`.
pub fn assert_golden(name: &str, bytes: &[u8]) {
    let path = fixture_dir().join(name);
    if std::env::var_os("EVIDENCE_UPDATE_FIXTURES").is_some() {
        fs::write(&path, bytes).expect("write golden");
    }
    let expected = fs::read(&path).unwrap_or_else(|_| {
        panic!("golden {name} missing; run with EVIDENCE_UPDATE_FIXTURES=1 to write it")
    });
    assert_eq!(bytes, expected.as_slice(), "golden {name} drifted");
}

/// Serialize the committed parser graph with PDF locators.
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
        if let Some(label) = &node.label {
            out.push_str(&format!("label = \"{}\"\n", toml_escape(label)));
        }
        out.push_str(&format!(
            "canonical_text = \"{}\"\n",
            toml_escape(&node.canonical_text)
        ));
        out.push_str(&format!("content_sha256 = \"{}\"\n", node.content_sha256));
        out.push_str(&format!("fingerprint = \"{}\"\n", node.fingerprint));
        let evidence_core::corpus::SourceLocator::Pdf {
            physical_page,
            printed_label,
            bbox,
        } = &node.locator
        else {
            panic!("pdf locator");
        };
        out.push_str("\n[nodes.locator]\nformat = \"pdf\"\n");
        out.push_str(&format!("physical_page = {physical_page}\n"));
        if let Some(printed_label) = printed_label {
            out.push_str(&format!(
                "printed_label = \"{}\"\n",
                toml_escape(printed_label)
            ));
        }
        out.push_str(&format!("bbox = {bbox:?}\n"));
    }
    out
}

/// TOML basic-string escaping for the fixture serializer.
fn toml_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// The curated patch restoring the PICS table graph: a table with
/// header and data rows under the PICS section.
fn pics_patch(committed: &SourceGraph, section_uid: &str, input_hex: &str) -> SourcePatchRecord {
    let locator = || evidence_core::corpus::SourceLocator::Pdf {
        physical_page: 1,
        printed_label: Some("1".to_string()),
        bbox: [72.0, 172.82, 167.04, 218.07],
    };
    let spec = |uid: &str, kind: SourceNodeKind, ordinal: u32, text: &str| InsertedNodeSpec {
        uid: uid.to_string(),
        kind,
        ordinal,
        label: None,
        canonical_text: text.to_string(),
        locator: locator(),
    };
    let cell = |suffix: &str, ordinal: u32, text: &str| {
        spec(
            &format!("snode_00000000-0000-4000-8000-0000000000f{suffix}"),
            SourceNodeKind::TableCell,
            ordinal,
            text,
        )
    };
    let insert = |ordinal: u32, parent: &str, node: InsertedNodeSpec| PatchOperation::Insert {
        ordinal,
        expected_parent_uid: Some(parent.to_string()),
        node,
    };
    let mut record = SourcePatchRecord {
        uid: PATCH_UID.to_string(),
        human_id: "pics-table-restore".to_string(),
        source_revision_uid: REV.to_string(),
        recipe_digest: recipe().digest(),
        input_digest: StructuralContentDigest::from_hex(input_hex).expect("hex"),
        pre_patch_graph_digest: source_graph_digest(committed),
        reviewed_content_digest: StructuralContentDigest::from_hex(&"0".repeat(64)).expect("hex"),
        author: "curator@example.com".to_string(),
        rationale: "the raw bbox projection cannot prove the PICS table rows and cells".to_string(),
        created_at: "2026-07-25T00:00:00Z".to_string(),
        operations: vec![
            insert(0, section_uid, spec(TBL, SourceNodeKind::Table, 2, "")),
            insert(1, TBL, spec(ROW1, SourceNodeKind::TableRow, 0, "")),
            insert(2, ROW1, cell("1", 0, "Item")),
            insert(3, ROW1, cell("2", 1, "M")),
            insert(4, ROW1, cell("3", 2, "Status")),
            insert(5, TBL, spec(ROW2, SourceNodeKind::TableRow, 1, "")),
            insert(6, ROW2, cell("4", 0, "R1")),
            insert(7, ROW2, cell("5", 1, "M")),
            insert(8, ROW2, cell("6", 2, "Mandatory")),
        ],
    };
    record.reviewed_content_digest = reviewed_content_digest(&record);
    record
}

/// Serialize one patch record with only insert operations.
fn patch_toml(record: &SourcePatchRecord) -> String {
    let mut out = format!(
        "schema_version = 1\n\n[patch]\nuid = \"{}\"\nhuman_id = \"{}\"\n\
         source_revision_uid = \"{}\"\nrecipe_digest = \"{}\"\ninput_digest = \"{}\"\n\
         pre_patch_graph_digest = \"{}\"\nreviewed_content_digest = \"{}\"\n\
         author = \"{}\"\nrationale = \"{}\"\ncreated_at = \"{}\"\n",
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
    );
    for operation in &record.operations {
        let PatchOperation::Insert {
            ordinal,
            expected_parent_uid,
            node,
        } = operation
        else {
            panic!("insert-only fixture");
        };
        out.push_str(&format!(
            "\n[[patch.operations]]\nop = \"insert\"\nordinal = {ordinal}\n"
        ));
        if let Some(parent) = expected_parent_uid {
            out.push_str(&format!("expected_parent_uid = \"{parent}\"\n"));
        }
        let evidence_core::corpus::SourceLocator::Pdf {
            physical_page,
            printed_label,
            bbox,
        } = &node.locator
        else {
            panic!("pdf locator");
        };
        out.push_str(&format!(
            "node = {{ uid = \"{}\", kind = \"{}\", ordinal = {}, canonical_text = \"{}\", \
             locator = {{ format = \"pdf\", physical_page = {physical_page}, \
             printed_label = \"{}\", bbox = {bbox:?} }} }}\n",
            node.uid,
            node.kind.as_str(),
            node.ordinal,
            toml_escape(&node.canonical_text),
            printed_label.clone().unwrap_or_default(),
        ));
    }
    out
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, content).expect("write");
}

/// The loaded PICS corpus plus the identities the tests bind.
pub struct PicsCorpus {
    /// The validated corpus graph.
    pub corpus: CorpusGraph,
    /// The verified input digest of the PICS PDF bytes.
    pub input_hex: String,
}

/// Load the PICS corpus on disk: the raw parser graph, the
/// curated patch, and — when `approve` — the approval review of
/// the patch's current reviewed-content digest.
pub fn load_pics_corpus(approve: bool) -> PicsCorpus {
    let pdf = fixture_bytes("pdf_pics_acceptance_v1.pdf");
    let input_hex = sha256(&pdf);
    let ingestion = ingest("pdf_pics_acceptance_v1.pdf", "pdf_pics_bbox_v1.xhtml");
    let mut committed = SourceGraph::new();
    for node in &ingestion.nodes {
        committed.insert(node.clone()).expect("committed graph");
    }
    let section_uid = ingestion
        .nodes
        .iter()
        .find(|node| node.kind == SourceNodeKind::Section)
        .expect("section")
        .uid
        .clone();
    let patch = pics_patch(&committed, &section_uid, &input_hex);
    let reviewed_hex = patch.reviewed_content_digest.as_str().to_string();
    let reviews = if approve {
        format!(
            "\n[[reviews]]\nuid = \"{REVIEW_UID}\"\nid = \"REV-001\"\n\
             target = {{ kind = \"curated_patch\", uid = \"{PATCH_UID}\" }}\n\
             content_schema = 1\nreviewed_content_sha256 = \"{reviewed_hex}\"\n\
             decision = \"approve\"\nreviewer = \"alice@example.com\"\n\
             reviewed_at = \"2026-07-25T00:00:00Z\"\n"
        )
    } else {
        String::new()
    };
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        &dir.path().join("sources/records.toml"),
        &format!(
            "schema_version = 1\n\n[[sources]]\nuid = \"{REV}\"\nid = \"PICS-PDF\"\n\
             document_key = \"pics-pdf\"\ntitle = \"PICS PDF fixture\"\nmedia_type = \"{MEDIA}\"\n\
             canonical_location = \"{CANONICAL}\"\n\n\
             [sources.material]\nstate = \"unavailable\"\nreason = \"fixture\"\n"
        ),
    );
    write(
        &dir.path().join("graphs/records.toml"),
        &graph_file_toml(&committed),
    );
    write(&dir.path().join("patches/pics.toml"), &patch_toml(&patch));
    write(
        &dir.path().join("reviews/records.toml"),
        &format!("schema_version = 2\n{reviews}"),
    );
    write(
        &dir.path().join("corpus.toml"),
        "schema_version = 1\nsources = [\"sources/**/*.toml\"]\n\
         source_graphs = [\"graphs/**/*.toml\"]\nsource_patches = [\"patches/**/*.toml\"]\n\
         reviews = [\"reviews/**/*.toml\"]\n",
    );
    let corpus = CorpusIndex::load_graph(&dir.path().join("corpus.toml")).expect("corpus loads");
    PicsCorpus { corpus, input_hex }
}
