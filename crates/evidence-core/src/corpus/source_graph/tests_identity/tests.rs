//! Tests for uid minting and structural-key reconciliation
//! (TEST-174).

use super::identity::{CandidateNode, StructuralKey, mint_node_uid, reconcile, structural_key};
use super::normalization::{content_digest, fingerprint};
use super::records::SNODE_UID_PREFIX;
use super::{SourceGraph, SourceNode, SourceNodeKind};
use crate::corpus::SourceLocator;

const REV_A: &str = "src_00000000-0000-4000-8000-0000000000a1";
const NODE_A: &str = "snode_00000000-0000-4000-8000-0000000000b1";
const NODE_B: &str = "snode_00000000-0000-4000-8000-0000000000b2";
const NODE_C: &str = "snode_00000000-0000-4000-8000-0000000000b3";

fn md_locator(anchor: Option<&str>) -> SourceLocator {
    SourceLocator::Markdown {
        path: crate::corpus::SafeRelPath::new("docs/spec.md").expect("safe path"),
        git_blob: None,
        anchor: anchor.map(str::to_string),
        heading_path: Vec::new(),
        byte_range: (0, 10),
    }
}

/// Build a committed node with digests computed against the
/// already-committed graph's ancestry.
#[allow(
    clippy::too_many_arguments,
    reason = "the builder mirrors the node schema field for field"
)]
fn make_node(
    graph: &SourceGraph,
    uid: &str,
    parent: Option<&str>,
    kind: SourceNodeKind,
    ordinal: u32,
    label: Option<&str>,
    text: &str,
    anchor: Option<&str>,
) -> SourceNode {
    let mut ancestry = Vec::new();
    let mut current = parent;
    while let Some(uid) = current {
        let node = graph.get(uid).expect("parent committed first");
        ancestry.push((node.kind, node.label.clone()));
        current = node.parent_uid.as_deref();
    }
    ancestry.reverse();
    let ancestry_refs: Vec<(SourceNodeKind, Option<&str>)> = ancestry
        .iter()
        .map(|(kind, label)| (*kind, label.as_deref()))
        .collect();
    SourceNode {
        uid: uid.to_string(),
        source_revision_uid: REV_A.to_string(),
        parent_uid: parent.map(str::to_string),
        kind,
        ordinal,
        label: label.map(str::to_string),
        canonical_text: text.to_string(),
        content_sha256: content_digest(kind, text),
        fingerprint: fingerprint(kind, label, &ancestry_refs),
        locator: md_locator(anchor),
    }
}

fn candidate(
    id: &str,
    parent: Option<&str>,
    kind: SourceNodeKind,
    ordinal: u32,
    label: Option<&str>,
    text: &str,
    anchor: Option<&str>,
) -> CandidateNode {
    CandidateNode {
        provisional_id: id.to_string(),
        parent_id: parent.map(str::to_string),
        kind,
        ordinal,
        label: label.map(str::to_string),
        canonical_text: text.to_string(),
        locator: md_locator(anchor),
    }
}

/// A committed forest: an anchored section with a positional
/// paragraph child, plus an unanchored, unlabeled root section
/// that only a fingerprint key can match.
fn committed_graph() -> SourceGraph {
    let mut graph = SourceGraph::new();
    let section = make_node(
        &graph,
        NODE_A,
        None,
        SourceNodeKind::Section,
        0,
        None,
        "",
        Some("sec-1"),
    );
    graph.insert(section).expect("insert section");
    let paragraph = make_node(
        &graph,
        NODE_B,
        Some(NODE_A),
        SourceNodeKind::Paragraph,
        0,
        None,
        "First prose.",
        None,
    );
    graph.insert(paragraph).expect("insert paragraph");
    let plain = make_node(
        &graph,
        NODE_C,
        None,
        SourceNodeKind::Section,
        1,
        None,
        "",
        None,
    );
    graph.insert(plain).expect("insert plain section");
    graph
}

/// Initial ingestion: every candidate mints a fresh, unique,
/// valid `snode_<UUIDv4>` uid (TEST-174).
#[test]
fn initial_ingestion_mints_valid_unique_snode_uids() {
    let mut uids = std::collections::BTreeSet::new();
    for _ in 0..100 {
        let uid = mint_node_uid();
        let suffix = uid.strip_prefix(SNODE_UID_PREFIX).expect("snode_ prefix");
        let parsed = uuid::Uuid::parse_str(suffix).expect("UUID parses");
        assert_eq!(
            parsed.get_version(),
            Some(uuid::Version::Random),
            "identity mints UUIDv4"
        );
        assert!(uids.insert(uid), "minted uids are unique");
    }

    // Reconciliation against an empty committed graph mints for
    // every candidate, preserving input order.
    let candidates = vec![
        candidate(
            "c0",
            None,
            SourceNodeKind::Section,
            0,
            None,
            "",
            Some("sec-1"),
        ),
        candidate(
            "c1",
            Some("c0"),
            SourceNodeKind::Paragraph,
            0,
            None,
            "text",
            None,
        ),
    ];
    let reconciled = reconcile(&SourceGraph::new(), candidates);
    assert_eq!(reconciled.len(), 2);
    assert_eq!(reconciled[0].candidate.provisional_id, "c0");
    assert_eq!(reconciled[1].candidate.provisional_id, "c1");
    assert_ne!(reconciled[0].uid, reconciled[1].uid);
    for node in &reconciled {
        assert!(node.uid.starts_with(SNODE_UID_PREFIX));
    }
}

/// Re-ingestion: candidates matching a committed structural key
/// reuse the committed uid; unmatched candidates mint
/// (TEST-174).
#[test]
fn reconciliation_reuses_committed_uids_by_structural_key() {
    let committed = committed_graph();
    let candidates = vec![
        // Tier 1: the anchor matches the committed section.
        candidate(
            "c0",
            None,
            SourceNodeKind::Section,
            0,
            None,
            "",
            Some("sec-1"),
        ),
        // Tier 2: parent path plus ordinal matches the paragraph.
        candidate(
            "c1",
            Some("c0"),
            SourceNodeKind::Paragraph,
            0,
            None,
            "First prose.",
            None,
        ),
        // Tier 3: no anchor, no label, root — fingerprint matches
        // the plain section.
        candidate("c2", None, SourceNodeKind::Section, 1, None, "", None),
        // No match: a new note mints.
        candidate("c3", Some("c0"), SourceNodeKind::Note, 1, None, "new", None),
    ];
    let reconciled = reconcile(&committed, candidates);
    assert_eq!(
        reconciled[0].uid, NODE_A,
        "anchor key reuses the committed uid"
    );
    assert_eq!(
        reconciled[1].uid, NODE_B,
        "position key reuses the committed uid"
    );
    assert_eq!(
        reconciled[2].uid, NODE_C,
        "fingerprint key reuses the committed uid"
    );
    assert!(reconciled[3].uid.starts_with(SNODE_UID_PREFIX));
    assert!(
        committed.get(&reconciled[3].uid).is_none(),
        "an unmatched candidate never inherits an identity"
    );
}

/// The precedence chain is deterministic: explicit numbering or
/// anchor beats parent path plus ordinal, which beats the
/// fingerprint; numbering beats the locator anchor; and equal
/// structures key equally across the committed and candidate
/// representations (TEST-174).
#[test]
fn structural_key_precedence_is_deterministic() {
    let committed = committed_graph();

    // Tier 1 beats tier 2: the anchored section has a parent-free
    // explicit key.
    let section = committed.get(NODE_A).expect("section");
    assert!(
        matches!(
            structural_key(section, &committed),
            StructuralKey::Explicit {
                kind: SourceNodeKind::Section,
                ref anchor,
            } if anchor == "sec-1"
        ),
        "an anchored node keys explicitly"
    );

    // Numbering beats the locator anchor within tier 1.
    let mut numbered = section.clone();
    numbered.label = Some("1.2 Overview".to_string());
    assert!(
        matches!(
            structural_key(&numbered, &committed),
            StructuralKey::Explicit {
                kind: SourceNodeKind::Section,
                ref anchor,
            } if anchor == "1.2"
        ),
        "explicit numbering beats the locator anchor"
    );

    // Tier 2: the paragraph chains off its parent's explicit key.
    let paragraph = committed.get(NODE_B).expect("paragraph");
    let paragraph_key = structural_key(paragraph, &committed);
    assert!(
        matches!(
            paragraph_key,
            StructuralKey::Position {
                kind: SourceNodeKind::Paragraph,
                ordinal: 0,
                ..
            }
        ),
        "a parented node keys by parent path plus ordinal"
    );

    // Tier 3: the plain root section keys by fingerprint.
    let plain = committed.get(NODE_C).expect("plain section");
    assert!(
        matches!(
            structural_key(plain, &committed),
            StructuralKey::Fingerprint(_)
        ),
        "an unanchored, unlabeled root keys by fingerprint"
    );

    // Cross-representation determinism: an equivalent candidate
    // subtree keys exactly like the committed nodes.
    let candidates = vec![
        candidate(
            "c0",
            None,
            SourceNodeKind::Section,
            0,
            None,
            "",
            Some("sec-1"),
        ),
        candidate(
            "c1",
            Some("c0"),
            SourceNodeKind::Paragraph,
            0,
            None,
            "First prose.",
            None,
        ),
    ];
    let reconciled = reconcile(&committed, candidates);
    assert_eq!(reconciled[0].uid, NODE_A);
    assert_eq!(reconciled[1].uid, NODE_B);

    // Repeated computation is stable.
    assert_eq!(
        structural_key(paragraph, &committed),
        structural_key(paragraph, &committed)
    );
}

/// A content change moves the content digest while the structural
/// identity persists (TEST-174).
#[test]
fn content_change_moves_digest_not_identity() {
    let committed = committed_graph();
    let before = committed.get(NODE_B).expect("paragraph");

    let changed = candidate(
        "c1",
        Some("c0"),
        SourceNodeKind::Paragraph,
        0,
        None,
        "First prose, revised.",
        None,
    );
    let section = candidate(
        "c0",
        None,
        SourceNodeKind::Section,
        0,
        None,
        "",
        Some("sec-1"),
    );
    let reconciled = reconcile(&committed, vec![section, changed]);
    let paragraph = &reconciled[1];
    assert_eq!(
        paragraph.uid, NODE_B,
        "a content change never replaces identity"
    );
    let drifted = content_digest(
        SourceNodeKind::Paragraph,
        &paragraph.candidate.canonical_text,
    );
    assert_ne!(
        drifted, before.content_sha256,
        "the content change moves the content digest: drift is observable"
    );
    assert_eq!(
        paragraph.candidate.canonical_text, "First prose, revised.",
        "the candidate carries the changed content"
    );
}
