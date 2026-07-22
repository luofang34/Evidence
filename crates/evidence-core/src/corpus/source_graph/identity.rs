//! Structural source-node identity: uid minting and
//! deterministic structural-key reconciliation (LLR-158).
//!
//! Initial ingestion mints a fresh `snode_<UUIDv4>` uid per node
//! through [`mint_node_uid`] — the only identity-minting path, so
//! no identity is ever derived from content. Re-ingestion
//! reconciles a candidate forest against the committed graph:
//! each candidate's [`StructuralKey`] is computed, a key match
//! against a committed node reuses that node's committed uid,
//! and an unmatched candidate mints a fresh UUIDv4 uid. A content
//! change moves only the candidate's content digest; the
//! structural key — and therefore the identity — persists.
//!
//! # Structural-key precedence
//!
//! The key is the first computable tier of three:
//!
//! 1. **Explicit document numbering or anchor** — the label's
//!    leading dotted-decimal token (`"4.1.2"` in
//!    `"4.1.2 Introduction"`: ASCII digits and dots, at least one
//!    digit, starting and ending with a digit) or, failing that,
//!    the locator's document-author anchor (Markdown `anchor`,
//!    HTML `fragment`), keyed with the kind. Numbering and anchor
//!    share one key space: both are document-author explicit
//!    identities. A label whose leading token is numbering-shaped
//!    IS explicit numbering by contract — ingesters assign labels
//!    accordingly.
//! 2. **Parent path plus sibling ordinal** — the parent's
//!    structural key chained with the node's kind and ordinal.
//!    Applies whenever the node has a parent.
//! 3. **Structural fingerprint** — the stable kind/label/ancestry
//!    encoding. Applies to roots, which have no parent path.
//!
//! Diagnostic positions — page, DOM, byte, line — never enter any
//! tier. [`reconcile`] assumes a validated committed graph; on an
//! unvalidated graph with a dangling parent or a cycle, the
//! walk stops at the break and the key falls back to the deepest
//! resolvable tier, deterministically and never panicking.

use std::collections::{BTreeMap, BTreeSet};

use super::super::digest::StructuralContentDigest;
use super::SourceNodeKind;
use super::locator::SourceLocator;
use super::normalization::fingerprint;
use super::records::SNODE_UID_PREFIX;
use super::{SourceGraph, SourceNode};

/// Mint a fresh structural source-node uid: `snode_` plus a
/// UUIDv4. The only identity-minting path; identities are never
/// derived from content.
pub fn mint_node_uid() -> String {
    format!("{}{}", SNODE_UID_PREFIX, uuid::Uuid::new_v4())
}

/// The deterministic structural key of a committed node or
/// ingestion candidate (LLR-158). Tier ordering is the precedence
/// chain pinned by the module docs; `Ord` is the derived lexical
/// order used only for deterministic map keying, never semantics.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum StructuralKey {
    /// Tier 1: explicit document numbering or anchor, with the
    /// kind.
    Explicit {
        /// The node's kind.
        kind: SourceNodeKind,
        /// The numbering token or document-author anchor.
        anchor: String,
    },
    /// Tier 2: parent path chained with kind and sibling ordinal.
    Position {
        /// The parent's structural key.
        parent: Box<StructuralKey>,
        /// The node's kind.
        kind: SourceNodeKind,
        /// The node's ordinal within its sibling set.
        ordinal: u32,
    },
    /// Tier 3: the structural fingerprint.
    Fingerprint(StructuralContentDigest),
}

/// One structural node as an ingester parsed it, before identity
/// reconciliation (LLR-158).
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateNode {
    /// Candidate-local identity for parent linkage; unique within
    /// one candidate set (on duplicates the last candidate wins
    /// parent resolution, deterministically).
    pub provisional_id: String,
    /// The parent's provisional id; `None` for a root.
    pub parent_id: Option<String>,
    /// Closed structural kind.
    pub kind: SourceNodeKind,
    /// Position within the parent's sibling set.
    pub ordinal: u32,
    /// Optional human identity; an explicit-numbering-shaped
    /// leading token feeds the key's first tier.
    pub label: Option<String>,
    /// Canonical text under the kind's normalization contract.
    pub canonical_text: String,
    /// The one typed diagnostic locator.
    pub locator: SourceLocator,
}

/// A candidate whose permanent uid reconciliation decided
/// (LLR-158): the reused committed uid on a structural-key match,
/// or a freshly minted UUIDv4 uid otherwise.
#[derive(Debug, Clone, PartialEq)]
pub struct ReconciledNode {
    /// The permanent committed identity.
    pub uid: String,
    /// The candidate as parsed.
    pub candidate: CandidateNode,
}

/// Compute a committed node's structural key under the precedence
/// chain pinned by the module docs.
pub fn structural_key(node: &SourceNode, graph: &SourceGraph) -> StructuralKey {
    if let Some(anchor) = explicit_identity(node.label.as_deref(), &node.locator) {
        return StructuralKey::Explicit {
            kind: node.kind,
            anchor,
        };
    }
    let mut chain: Vec<&SourceNode> = Vec::new();
    let mut visited = BTreeSet::from([node.uid.as_str()]);
    let mut current = node.parent_uid.as_deref();
    while let Some(uid) = current {
        let Some(parent) = graph.get(uid) else {
            break;
        };
        if !visited.insert(parent.uid.as_str()) {
            break;
        }
        chain.push(parent);
        current = parent.parent_uid.as_deref();
    }
    chain.reverse();
    let mut key: Option<StructuralKey> = None;
    for ancestor in chain {
        key = Some(fold_tier(
            ancestor.kind,
            ancestor.label.as_deref(),
            &ancestor.locator,
            ancestor.ordinal,
            key,
        ));
    }
    match key {
        Some(parent) => StructuralKey::Position {
            parent: Box::new(parent),
            kind: node.kind,
            ordinal: node.ordinal,
        },
        None => StructuralKey::Fingerprint(fingerprint(node.kind, node.label.as_deref(), &[])),
    }
}

/// Reconcile a candidate forest against the committed graph
/// (LLR-158). Candidates reconcile in input order: each
/// candidate's structural key is computed, a key match consumes
/// one committed uid from the matching pool (committed nodes pool
/// in uid order, so equal keys consume deterministically), and an
/// unmatched candidate mints a fresh UUIDv4 uid. The returned
/// vector preserves the input candidate order.
pub fn reconcile(committed: &SourceGraph, candidates: Vec<CandidateNode>) -> Vec<ReconciledNode> {
    let mut pool: BTreeMap<StructuralKey, Vec<String>> = BTreeMap::new();
    for node in committed.nodes() {
        pool.entry(structural_key(node, committed))
            .or_default()
            .push(node.uid.clone());
    }
    let by_id: BTreeMap<&str, &CandidateNode> = candidates
        .iter()
        .map(|candidate| (candidate.provisional_id.as_str(), candidate))
        .collect();
    let keys: Vec<StructuralKey> = candidates
        .iter()
        .map(|candidate| candidate_key(candidate, &by_id))
        .collect();
    candidates
        .into_iter()
        .zip(keys)
        .map(|(candidate, key)| {
            let uid = match pool.get_mut(&key) {
                Some(uids) if !uids.is_empty() => uids.remove(0),
                _ => mint_node_uid(),
            };
            ReconciledNode { uid, candidate }
        })
        .collect()
}

/// Compute a candidate's structural key under the same precedence
/// chain as committed nodes, resolving the parent path through
/// the candidate set's provisional ids.
fn candidate_key(
    candidate: &CandidateNode,
    by_id: &BTreeMap<&str, &CandidateNode>,
) -> StructuralKey {
    if let Some(anchor) = explicit_identity(candidate.label.as_deref(), &candidate.locator) {
        return StructuralKey::Explicit {
            kind: candidate.kind,
            anchor,
        };
    }
    let mut chain: Vec<&CandidateNode> = Vec::new();
    let mut visited = BTreeSet::from([candidate.provisional_id.as_str()]);
    let mut current = candidate.parent_id.as_deref();
    while let Some(id) = current {
        let Some(parent) = by_id.get(id).copied() else {
            break;
        };
        if !visited.insert(parent.provisional_id.as_str()) {
            break;
        }
        chain.push(parent);
        current = parent.parent_id.as_deref();
    }
    chain.reverse();
    let mut key: Option<StructuralKey> = None;
    for ancestor in chain {
        key = Some(fold_tier(
            ancestor.kind,
            ancestor.label.as_deref(),
            &ancestor.locator,
            ancestor.ordinal,
            key,
        ));
    }
    match key {
        Some(parent) => StructuralKey::Position {
            parent: Box::new(parent),
            kind: candidate.kind,
            ordinal: candidate.ordinal,
        },
        None => {
            StructuralKey::Fingerprint(fingerprint(candidate.kind, candidate.label.as_deref(), &[]))
        }
    }
}

/// One fold step of the precedence chain for an ancestor: its own
/// explicit identity if it has one, else its position under the
/// key accumulated so far, else — for a root — its fingerprint
/// over kind and label (a root's ancestry is empty).
fn fold_tier(
    kind: SourceNodeKind,
    label: Option<&str>,
    locator: &SourceLocator,
    ordinal: u32,
    parent_key: Option<StructuralKey>,
) -> StructuralKey {
    if let Some(anchor) = explicit_identity(label, locator) {
        return StructuralKey::Explicit { kind, anchor };
    }
    match parent_key {
        Some(parent) => StructuralKey::Position {
            parent: Box::new(parent),
            kind,
            ordinal,
        },
        None => StructuralKey::Fingerprint(fingerprint(kind, label, &[])),
    }
}

/// The tier-1 explicit identity of a node: the label's leading
/// dotted-decimal numbering token, else the locator's
/// document-author anchor.
fn explicit_identity(label: Option<&str>, locator: &SourceLocator) -> Option<String> {
    if let Some(label) = label {
        if let Some(numbering) = leading_numbering_token(label) {
            return Some(numbering.to_string());
        }
    }
    locator.explicit_anchor().map(str::to_string)
}

/// The label's leading token when it is explicit document
/// numbering: ASCII digits and dots, at least one digit, starting
/// and ending with a digit (`"4"`, `"4.1"`, `"4.1.2"`).
fn leading_numbering_token(label: &str) -> Option<&str> {
    let token = label.split_whitespace().next()?;
    let valid = token.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && token.bytes().any(|b| b.is_ascii_digit())
        && token.bytes().next().is_some_and(|b| b.is_ascii_digit())
        && token.bytes().last().is_some_and(|b| b.is_ascii_digit());
    if valid { Some(token) } else { None }
}
