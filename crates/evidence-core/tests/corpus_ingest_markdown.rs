//! Markdown ingestion acceptance tests (TEST-177): the committed
//! fixture byte-locks its golden candidate projection, repeated
//! ingestion produces equal output with fresh identities, and
//! re-ingestion reuses committed uids through structural-key
//! reconciliation.
//!
//! The fixture `markdown_acceptance_v1.md` is an M4 acceptance
//! source — a small redistributable Markdown specification
//! exercising explicit and generated heading anchors, nested lists,
//! a GFM table, fenced code with significant whitespace, a GFM
//! note admonition, a footnote, Unicode normalization cases, and
//! repeated text under distinct structural parents. It is not a
//! certification claim. The golden
//! `markdown_acceptance_v1.golden` byte-locks the canonical
//! uid-free candidate projection; regenerate with
//! `EVIDENCE_UPDATE_FIXTURES=1`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use evidence_core::corpus::{
    CandidateNode, IngestMarkdownInput, IngesterRecipe, MarkdownIngestion, SourceGraph, SourceNode,
    StructuralContentDigest, ingest_markdown, reconcile,
};
use evidence_core::hash::sha256;

const REV: &str = "src_00000000-0000-4000-8000-0000000000c1";
const GIT_BLOB: &str = "0123456789abcdef0123456789abcdef01234567";

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus")
}

fn recipe() -> IngesterRecipe {
    IngesterRecipe {
        parser: "pulldown-cmark".to_string(),
        parser_version: "0.13.4".to_string(),
        extensions: ["footnotes".to_string(), "tables".to_string()]
            .into_iter()
            .collect::<BTreeSet<_>>(),
        adapter_version: "1".to_string(),
        normalization_contract: "evidence/source-node-normalization/v1".to_string(),
    }
}

fn fixture_input(bytes: &[u8]) -> IngestMarkdownInput<'_> {
    IngestMarkdownInput {
        bytes,
        media_type: "text/markdown",
        source_revision_uid: REV,
        canonical_path: "docs/evidence-spec.md",
        input_digest: StructuralContentDigest::from_hex(&sha256(bytes)).expect("sha256 hex"),
        git_blob: Some(GIT_BLOB.to_string()),
        recipe: recipe(),
    }
}

fn ingest_fixture() -> MarkdownIngestion {
    let bytes = fs::read(fixture_dir().join("markdown_acceptance_v1.md")).expect("read fixture");
    ingest_markdown(&fixture_input(&bytes)).expect("fixture ingestion succeeds")
}

/// The candidates of an ingestion, keyed by their minted uids —
/// the re-ingestion input shape.
fn candidates_of(nodes: &[SourceNode]) -> Vec<CandidateNode> {
    nodes
        .iter()
        .map(|node| CandidateNode {
            provisional_id: node.uid.clone(),
            parent_id: node.parent_uid.clone(),
            kind: node.kind,
            ordinal: node.ordinal,
            label: node.label.clone(),
            canonical_text: node.canonical_text.clone(),
            locator: node.locator.clone(),
        })
        .collect()
}

#[test]
fn golden_candidate_projection_byte_locks_canonical_bytes() {
    let outcome = ingest_fixture();
    assert!(
        outcome.diagnostics.is_empty(),
        "the acceptance fixture projects losslessly: {:?}",
        outcome.diagnostics
    );
    assert!(
        outcome.nodes.len() >= 20,
        "the fixture exercises the full construct set: {} nodes",
        outcome.nodes.len()
    );

    let rendered = outcome.canonical_projection();
    assert_eq!(
        outcome.output_digest,
        StructuralContentDigest::from_hex(&sha256(&rendered)).expect("sha256 hex"),
        "the output digest is sha256 over the canonical projection"
    );

    let path = fixture_dir().join("markdown_acceptance_v1.golden");
    if std::env::var_os("EVIDENCE_UPDATE_FIXTURES").is_some() {
        fs::write(&path, &rendered).expect("write fixture");
        return;
    }
    let committed = fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "missing or unreadable fixture {}: {e}\n\
             hint: run with EVIDENCE_UPDATE_FIXTURES=1 to write it",
            path.display()
        )
    });
    assert_eq!(
        rendered, committed,
        "canonical candidate projection drifted from the committed golden; \
         if the change is intended, regenerate with EVIDENCE_UPDATE_FIXTURES=1"
    );
}

#[test]
fn repeated_ingestion_produces_equal_output_with_fresh_uids() {
    let first = ingest_fixture();
    let second = ingest_fixture();

    assert_eq!(
        first.canonical_projection(),
        second.canonical_projection(),
        "repeated ingestion renders an identical canonical projection"
    );
    assert_eq!(
        first.output_digest, second.output_digest,
        "the output identity plane is deterministic across runs"
    );
    assert_eq!(first.nodes.len(), second.nodes.len());

    // Identities are minted fresh per run — never derived from
    // content — so the two runs share no uid.
    let first_uids: BTreeSet<&str> = first.nodes.iter().map(|n| n.uid.as_str()).collect();
    let second_uids: BTreeSet<&str> = second.nodes.iter().map(|n| n.uid.as_str()).collect();
    assert!(
        first_uids.is_disjoint(&second_uids),
        "minted identities are fresh per run"
    );
    assert_eq!(first_uids.len(), first.nodes.len(), "uids are unique");
}

#[test]
fn reingestion_reuses_committed_uids_through_reconciliation() {
    let committed_run = ingest_fixture();
    let reingested = ingest_fixture();

    // Commit run one's nodes in document order, then in reversed
    // record order: reconciliation must reuse the same committed uid
    // for each structurally matching candidate either way.
    for reversed in [false, true] {
        let mut graph = SourceGraph::new();
        let mut records: Vec<&SourceNode> = committed_run.nodes.iter().collect();
        if reversed {
            records.reverse();
        }
        for node in records {
            graph.insert(node.clone()).expect("committed nodes insert");
        }

        let reconciled = reconcile(&graph, candidates_of(&reingested.nodes));
        assert_eq!(reconciled.len(), committed_run.nodes.len());
        for (index, entry) in reconciled.iter().enumerate() {
            assert_eq!(
                entry.uid, committed_run.nodes[index].uid,
                "node {index} must reuse the committed uid (reversed layout: {reversed})"
            );
        }
    }
}
