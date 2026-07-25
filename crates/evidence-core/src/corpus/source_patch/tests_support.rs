//! Shared fixtures for curated-patch unit tests: a small valid
//! committed graph builder, a patch-record builder whose digests
//! finalize against a graph, and a deterministic TOML writer for
//! patch records.

use std::collections::BTreeSet;

use super::super::digest::StructuralContentDigest;
use super::super::source_graph::locator::{SafeRelPath, SourceLocator};
use super::super::source_graph::normalization::{content_digest, fingerprint};
use super::super::source_graph::{SourceGraph, SourceNode, SourceNodeKind};
use super::digest::{reviewed_content_digest, source_graph_digest};
use super::records::SourcePatchRecord;
use super::{ChildDisposition, InsertedNodeSpec, PatchOperation};

/// The bound source revision of every fixture patch.
pub(crate) const REV: &str = "src_00000000-0000-4000-8000-0000000000d1";
/// The recipe digest every fixture patch binds.
pub(crate) const RECIPE_HEX: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
/// The verified input digest every fixture patch binds.
pub(crate) const INPUT_HEX: &str =
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

pub(crate) fn recipe_digest() -> StructuralContentDigest {
    StructuralContentDigest::from_hex(RECIPE_HEX).unwrap()
}

pub(crate) fn input_digest() -> StructuralContentDigest {
    StructuralContentDigest::from_hex(INPUT_HEX).unwrap()
}

/// One row of the graph builder: parents must precede children.
pub(crate) struct NodeRow {
    pub uid: &'static str,
    pub parent: Option<&'static str>,
    pub kind: SourceNodeKind,
    pub ordinal: u32,
    pub label: Option<&'static str>,
    pub text: &'static str,
}

fn locator() -> SourceLocator {
    SourceLocator::Markdown {
        path: SafeRelPath::new("docs/spec.md").unwrap(),
        git_blob: None,
        anchor: None,
        heading_path: Vec::new(),
        byte_range: (0, 10),
    }
}

/// Build a valid committed graph from `rows`, computing every
/// content digest and fingerprint from the kind, text, label, and
/// ancestry of the nodes inserted so far.
pub(crate) fn build_graph(rows: &[NodeRow]) -> SourceGraph {
    let mut graph = SourceGraph::new();
    for row in rows {
        let mut ancestry = Vec::new();
        let mut visited = BTreeSet::new();
        let mut current = row.parent;
        while let Some(uid) = current {
            let parent = graph.get(uid).unwrap();
            assert!(visited.insert(uid), "fixture ancestry must be acyclic");
            ancestry.push((parent.kind, parent.label.clone()));
            current = parent.parent_uid.as_deref();
        }
        ancestry.reverse();
        let pair_refs: Vec<(SourceNodeKind, Option<&str>)> = ancestry
            .iter()
            .map(|(kind, label)| (*kind, label.as_deref()))
            .collect();
        graph
            .insert(SourceNode {
                uid: row.uid.to_string(),
                source_revision_uid: REV.to_string(),
                parent_uid: row.parent.map(str::to_string),
                kind: row.kind,
                ordinal: row.ordinal,
                label: row.label.map(str::to_string),
                canonical_text: row.text.to_string(),
                content_sha256: content_digest(row.kind, row.text),
                fingerprint: fingerprint(row.kind, row.label, &pair_refs),
                locator: locator(),
            })
            .unwrap();
    }
    graph
}

/// A patch record over `operations` with placeholder digests;
/// [`finalize`] binds it to a graph.
pub(crate) fn patch_record(
    uid: &str,
    human_id: &str,
    operations: Vec<PatchOperation>,
) -> SourcePatchRecord {
    SourcePatchRecord {
        uid: uid.to_string(),
        human_id: human_id.to_string(),
        source_revision_uid: REV.to_string(),
        recipe_digest: recipe_digest(),
        input_digest: input_digest(),
        pre_patch_graph_digest: recipe_digest(),
        reviewed_content_digest: recipe_digest(),
        author: "curator@example.com".to_string(),
        rationale: "restore the intended structure".to_string(),
        created_at: "2026-07-25T00:00:00Z".to_string(),
        operations,
    }
}

/// Bind `record` to `graph`: the pre-patch graph digest recomputes
/// from the graph, then the reviewed-content digest recomputes
/// over the finalized record.
pub(crate) fn finalize(mut record: SourcePatchRecord, graph: &SourceGraph) -> SourcePatchRecord {
    record.pre_patch_graph_digest = source_graph_digest(graph);
    record.reviewed_content_digest = reviewed_content_digest(&record);
    record
}

/// Render a patch operation as TOML lines (deterministic, minimal).
fn push_operation_toml(out: &mut String, operation: &PatchOperation) {
    out.push_str("\n[[patch.operations]]\n");
    match operation {
        PatchOperation::ReplaceContent {
            ordinal,
            target_uid,
            expected_content_sha256,
            new_canonical_text,
            new_label,
        } => {
            out.push_str("op = \"replace_content\"\n");
            out.push_str(&format!("ordinal = {ordinal}\n"));
            out.push_str(&format!("target_uid = \"{target_uid}\"\n"));
            out.push_str(&format!(
                "expected_content_sha256 = \"{expected_content_sha256}\"\n"
            ));
            if let Some(text) = new_canonical_text {
                out.push_str(&format!("new_canonical_text = \"{text}\"\n"));
            }
            if let Some(label) = new_label {
                out.push_str(&format!("new_label = \"{label}\"\n"));
            }
        }
        PatchOperation::Reclassify {
            ordinal,
            target_uid,
            expected_kind,
            new_kind,
        } => {
            out.push_str("op = \"reclassify\"\n");
            out.push_str(&format!("ordinal = {ordinal}\n"));
            out.push_str(&format!("target_uid = \"{target_uid}\"\n"));
            out.push_str(&format!("expected_kind = \"{}\"\n", expected_kind.as_str()));
            out.push_str(&format!("new_kind = \"{}\"\n", new_kind.as_str()));
        }
        PatchOperation::Reparent {
            ordinal,
            target_uid,
            expected_parent_uid,
            expected_ordinal,
            new_parent_uid,
            new_ordinal,
        } => {
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
        PatchOperation::Insert {
            ordinal,
            expected_parent_uid,
            node,
        } => {
            out.push_str("op = \"insert\"\n");
            out.push_str(&format!("ordinal = {ordinal}\n"));
            if let Some(parent) = expected_parent_uid {
                out.push_str(&format!("expected_parent_uid = \"{parent}\"\n"));
            }
            out.push_str(&format!(
                "node = {{ uid = \"{}\", kind = \"{}\", ordinal = {}, canonical_text = \"{}\", \
                 locator = {{ format = \"markdown\", path = \"docs/spec.md\", byte_range = [0, 10] }} }}\n",
                node.uid,
                node.kind.as_str(),
                node.ordinal,
                node.canonical_text
            ));
        }
        PatchOperation::Remove {
            ordinal,
            target_uid,
            expected_digest,
            child_disposition,
        } => {
            out.push_str("op = \"remove\"\n");
            out.push_str(&format!("ordinal = {ordinal}\n"));
            out.push_str(&format!("target_uid = \"{target_uid}\"\n"));
            out.push_str(&format!("expected_digest = \"{expected_digest}\"\n"));
            let disposition = match child_disposition {
                ChildDisposition::ReparentChildren => "reparent_children",
                ChildDisposition::RemoveSubtree => "remove_subtree",
            };
            out.push_str(&format!("child_disposition = \"{disposition}\"\n"));
        }
    }
}

/// Render a patch record as a deterministic TOML document that the
/// strict loader accepts.
pub(crate) fn patch_toml(record: &SourcePatchRecord) -> String {
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
        push_operation_toml(&mut out, operation);
    }
    out
}

/// An insert operation fixture with a markdown locator.
pub(crate) fn insert_op(
    ordinal: u32,
    parent: Option<&str>,
    uid: &str,
    kind: SourceNodeKind,
    node_ordinal: u32,
    text: &str,
) -> PatchOperation {
    PatchOperation::Insert {
        ordinal,
        expected_parent_uid: parent.map(str::to_string),
        node: InsertedNodeSpec {
            uid: uid.to_string(),
            kind,
            ordinal: node_ordinal,
            label: None,
            canonical_text: text.to_string(),
            locator: locator(),
        },
    }
}
