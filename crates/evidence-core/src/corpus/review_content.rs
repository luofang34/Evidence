//! Versioned canonical projection of the normative requirement
//! content a review approves, plus its digest (LLR-111).
//!
//! Approval binds [`RequirementReviewContentV1`] — never the short
//! title alone while normative prose or assurance-relevant metadata
//! changes underneath. The requirement uid is bound separately by
//! the review record and is **not** part of the projection.
//!
//! # v1 projection membership
//!
//! Included: title; decomposition layer; description; rationale;
//! scope; category; source reference; verification methods; the
//! canonical `derives_from` target uids; and a derived requirement's
//! `safety_impact` — normative assurance content, so changing it
//! must stale an approval. Excluded: the uid (bound separately),
//! the human id and namespace (renameable under DD-4), owner,
//! `sort_key`, implementation modules, governed surfaces, emitted
//! diagnostic codes, file path, TOML layout, file order, record
//! order, and which loader (native records or the legacy adapter)
//! produced the node — an approved requirement can acquire
//! implementation and test mappings without invalidating approval,
//! and equivalent records from either source digest identically.
//! `Verifies` edges are test mappings and never enter the
//! projection.
//!
//! # Canonical byte format (v1)
//!
//! [`canonical_bytes_v1`] encodes the projection as:
//!
//! ```text
//! b"evidence/requirement-review-content/v1" || 0x00
//! || str(title) || str(layer)
//! || opt(description) || opt(rationale) || opt(scope)
//! || opt(category) || opt(source)
//! || u64_be(verification_methods.len()) || str(each method, sorted)
//! || u64_be(derives_from.len())         || str(each target, sorted)
//! || opt(safety_impact)
//! ```
//!
//! where `str(s)` is `u64_be(byte length of s) || s`'s exact UTF-8
//! bytes as parsed — no whitespace or Unicode normalization — and
//! `opt(o)` is the all-ones sentinel `0xFFFFFFFFFFFFFFFF`
//! (`u64::MAX`, unreachable as a real byte length) for `None`, or
//! `str(v)` for `Some(v)`. `layer` encodes as its serde snake_case
//! wire string ([`RequirementLayer::as_str`]). `safety_impact` is
//! the last field: populated for derived requirements, the `None`
//! sentinel for other layers. All lengths and counts are unsigned
//! 64-bit big-endian; the length-prefix framing (the
//! `envelope_bytes` precedent) prevents field-boundary collisions,
//! and the domain/version tag keeps v1 bytes disjoint from every
//! other encoding. Changing this contract requires a new projection
//! version, never a silent change of existing digests.
//!
//! Both set-like lists are sorted and duplicate-free before
//! encoding — enforced here, so no construction path can affect the
//! digest. (Duplicate *edges* remain a hard error at graph
//! insertion; that contract is unchanged.)

use super::digest::ReviewContentDigest;
use super::graph::{CorpusGraph, EdgeKind, Node, RequirementLayer, RequirementNode};

/// Domain/version tag prefixing the v1 canonical encoding.
const DOMAIN_TAG_V1: &[u8] = b"evidence/requirement-review-content/v1";

/// `None` sentinel for optional fields: the all-ones `u64`, which no
/// real byte length can reach.
const NONE_SENTINEL: u64 = u64::MAX;

/// The v1 review-content projection (LLR-111).
///
/// Field membership is specified by the module docs. The two
/// set-like lists are sorted and duplicate-free after construction
/// via [`RequirementReviewContentV1::from_node`] or
/// [`RequirementReviewContentV1::canonicalize`], and
/// [`canonical_bytes_v1`] re-canonicalizes before encoding so the
/// digest never depends on the construction path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementReviewContentV1 {
    /// One-line requirement title.
    pub title: String,
    /// Decomposition layer.
    pub layer: RequirementLayer,
    /// Normative requirement description.
    pub description: Option<String>,
    /// Normative rationale.
    pub rationale: Option<String>,
    /// Requirement scope.
    pub scope: Option<String>,
    /// Requirement category.
    pub category: Option<String>,
    /// Source reference.
    pub source: Option<String>,
    /// Verification methods — sorted and duplicate-free.
    pub verification_methods: Vec<String>,
    /// Canonical `derives_from` target uids — sorted and
    /// duplicate-free.
    pub derives_from: Vec<String>,
    /// Derived requirement's safety impact — normative assurance
    /// content, bound so changing it stales an approval. `None` for
    /// non-derived layers.
    pub safety_impact: Option<String>,
}

impl RequirementReviewContentV1 {
    /// Project a requirement node's review content: the node's
    /// normative content fields plus the canonical targets of its
    /// `DerivesFrom` edges. `Verifies` edges are test mappings and
    /// excluded (LLR-111).
    pub fn from_node(node: &RequirementNode) -> Self {
        let mut content = Self {
            title: node.title.clone(),
            layer: node.layer,
            description: node.description.clone(),
            rationale: node.rationale.clone(),
            scope: node.scope.clone(),
            category: node.category.clone(),
            source: node.source.clone(),
            verification_methods: node.verification_methods.clone(),
            derives_from: node
                .edges
                .iter()
                .filter(|(kind, _)| *kind == EdgeKind::DerivesFrom)
                .map(|(_, target)| target.clone())
                .collect(),
            safety_impact: node.safety_impact.clone(),
        };
        content.canonicalize();
        content
    }

    /// Sort and deduplicate both set-like lists, matching the
    /// existing metadata-list contract: set-like duplicates
    /// canonicalize silently while duplicate typed edges stay a hard
    /// error at graph insertion.
    pub fn canonicalize(&mut self) {
        canonical_strings_in_place(&mut self.verification_methods);
        canonical_strings_in_place(&mut self.derives_from);
    }
}

impl CorpusGraph {
    /// The v1 review-content projection for the requirement at
    /// `uid`, or `None` when the uid is absent or names a test node.
    /// Pure: no I/O, no lifecycle state (LLR-111).
    pub fn review_content(&self, uid: &str) -> Option<RequirementReviewContentV1> {
        match self.get(uid) {
            Some(Node::Requirement(node)) => Some(RequirementReviewContentV1::from_node(node)),
            _ => None,
        }
    }
}

/// Encode `content` in the v1 canonical byte format pinned by the
/// module docs. Pure and host-independent (LLR-111).
pub fn canonical_bytes_v1(content: &RequirementReviewContentV1) -> Vec<u8> {
    let mut methods = content.verification_methods.clone();
    canonical_strings_in_place(&mut methods);
    let mut targets = content.derives_from.clone();
    canonical_strings_in_place(&mut targets);

    let mut out = Vec::new();
    out.extend_from_slice(DOMAIN_TAG_V1);
    out.push(0x00);
    push_str(&mut out, &content.title);
    push_str(&mut out, content.layer.as_str());
    push_opt(&mut out, content.description.as_deref());
    push_opt(&mut out, content.rationale.as_deref());
    push_opt(&mut out, content.scope.as_deref());
    push_opt(&mut out, content.category.as_deref());
    push_opt(&mut out, content.source.as_deref());
    push_count(&mut out, methods.len());
    for method in &methods {
        push_str(&mut out, method);
    }
    push_count(&mut out, targets.len());
    for target in &targets {
        push_str(&mut out, target);
    }
    push_opt(&mut out, content.safety_impact.as_deref());
    out
}

/// Lowercase hexadecimal SHA-256 of the v1 canonical bytes, as a
/// typed digest value (LLR-112). Pure: no I/O.
pub fn review_content_digest_v1(content: &RequirementReviewContentV1) -> ReviewContentDigest {
    ReviewContentDigest::from_hasher_output(crate::hash::sha256(&canonical_bytes_v1(content)))
}

/// `str(s)` framing: `u64_be(len) || bytes`, exactly as parsed.
fn push_str(out: &mut Vec<u8>, value: &str) {
    push_count(out, value.len());
    out.extend_from_slice(value.as_bytes());
}

/// `opt(o)` framing: the all-ones sentinel for `None`, `str(v)` for
/// `Some(v)`.
fn push_opt(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => push_str(out, value),
        None => out.extend_from_slice(&NONE_SENTINEL.to_be_bytes()),
    }
}

fn push_count(out: &mut Vec<u8>, count: usize) {
    out.extend_from_slice(&(count as u64).to_be_bytes());
}

/// Sort + dedup in place — the canonical form every set-like list
/// takes before encoding.
fn canonical_strings_in_place(values: &mut Vec<String>) {
    values.sort();
    values.dedup();
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;
    use crate::corpus::graph::Node;

    fn base() -> RequirementReviewContentV1 {
        let mut content = RequirementReviewContentV1 {
            title: "Reviewed requirement".to_string(),
            layer: RequirementLayer::Hlr,
            description: Some("Normative prose.".to_string()),
            rationale: Some("Why the requirement exists.".to_string()),
            scope: Some("component".to_string()),
            category: Some("functional".to_string()),
            source: Some("SRS-1".to_string()),
            verification_methods: vec!["review".to_string(), "test".to_string()],
            derives_from: vec!["req_a".to_string(), "req_b".to_string()],
            safety_impact: Some("low".to_string()),
        };
        content.canonicalize();
        content
    }

    /// Every included field must move the digest — approval binds
    /// the full normative statement, not the title alone (LLR-111).
    #[test]
    fn included_field_changes_move_the_digest() {
        type Mutation = (&'static str, fn(&mut RequirementReviewContentV1));
        let base_digest = review_content_digest_v1(&base());
        let cases: [Mutation; 13] = [
            ("title", |c| c.title.push_str(" updated")),
            ("layer", |c| c.layer = RequirementLayer::Llr),
            ("description", |c| {
                c.description = Some("Reworded prose.".to_string());
            }),
            ("description removed", |c| c.description = None),
            ("rationale", |c| {
                c.rationale = Some("Different rationale.".to_string());
            }),
            ("scope", |c| c.scope = None),
            ("category", |c| {
                c.category = Some("interface".to_string());
            }),
            ("source", |c| c.source = None),
            ("verification_methods", |c| {
                c.verification_methods.push("analysis".to_string());
            }),
            ("derives_from added", |c| {
                c.derives_from.push("req_c".to_string());
            }),
            ("derives_from removed", |c| {
                c.derives_from.retain(|target| target != "req_a");
            }),
            ("safety_impact", |c| {
                c.safety_impact = Some("high".to_string());
            }),
            ("safety_impact removed", |c| c.safety_impact = None),
        ];
        for (name, mutate) in cases {
            let mut changed = base();
            mutate(&mut changed);
            assert_ne!(
                review_content_digest_v1(&changed),
                base_digest,
                "changing {name} must change the digest"
            );
        }
    }

    /// Set-like lists canonicalize before encoding: order and
    /// duplicates in the input never reach the digest (LLR-111).
    #[test]
    fn set_like_lists_canonicalize_before_encoding() {
        let canonical = base();
        let mut messy = base();
        messy.verification_methods =
            vec!["test".to_string(), "review".to_string(), "test".to_string()];
        messy.derives_from = vec![
            "req_b".to_string(),
            "req_a".to_string(),
            "req_b".to_string(),
        ];
        assert_eq!(
            review_content_digest_v1(&messy),
            review_content_digest_v1(&canonical),
            "unsorted, duplicated set-like input must digest identically"
        );
        assert_eq!(canonical_bytes_v1(&messy), canonical_bytes_v1(&canonical));

        let via_node = node_with_edges(&[
            (EdgeKind::DerivesFrom, "req_b"),
            (EdgeKind::DerivesFrom, "req_a"),
        ]);
        let projected = RequirementReviewContentV1::from_node(&via_node);
        assert_eq!(
            projected.derives_from,
            vec!["req_a".to_string(), "req_b".to_string()],
            "from_node returns the lists in canonical order"
        );
    }

    /// `Verifies` edges are test mappings: they never enter the
    /// projection, so adding test coverage cannot invalidate an
    /// approval (LLR-111).
    #[test]
    fn from_node_binds_derives_from_only() {
        let without_tests = RequirementReviewContentV1::from_node(&node_with_edges(&[(
            EdgeKind::DerivesFrom,
            "req_a",
        )]));
        let with_tests = RequirementReviewContentV1::from_node(&node_with_edges(&[
            (EdgeKind::DerivesFrom, "req_a"),
            (EdgeKind::Verifies, "test_x"),
        ]));
        assert_eq!(
            review_content_digest_v1(&with_tests),
            review_content_digest_v1(&without_tests),
            "test mappings must not enter the review-content digest"
        );
        assert_eq!(with_tests.derives_from, vec!["req_a".to_string()]);
    }

    fn node_with_edges(edges: &[(EdgeKind, &str)]) -> RequirementNode {
        let base = base();
        RequirementNode {
            uid: "req_node".to_string(),
            id: "HLR-1".to_string(),
            title: base.title,
            layer: base.layer,
            edges: edges
                .iter()
                .map(|(kind, target)| (*kind, (*target).to_string()))
                .collect(),
            description: base.description,
            rationale: base.rationale,
            scope: base.scope,
            category: base.category,
            source: base.source,
            verification_methods: base.verification_methods,
            safety_impact: base.safety_impact,
        }
    }

    #[test]
    fn graph_review_content_projects_requirements_only() {
        let mut graph = CorpusGraph::new();
        graph
            .insert(Node::Requirement(node_with_edges(&[])))
            .expect("insert requirement");
        graph
            .insert(Node::Test(crate::corpus::graph::TestNode {
                uid: "test_x".to_string(),
                id: "TEST-1".to_string(),
                title: "test".to_string(),
                selectors: Vec::new(),
                edges: vec![(EdgeKind::Verifies, "req_node".to_string())],
            }))
            .expect("insert test");

        let content = graph
            .review_content("req_node")
            .expect("requirement projects review content");
        let mut expected = base();
        expected.derives_from = Vec::new();
        assert_eq!(content, expected);
        assert!(
            graph.review_content("test_x").is_none(),
            "test nodes carry no requirement review content"
        );
        assert!(graph.review_content("missing").is_none());
    }
}
