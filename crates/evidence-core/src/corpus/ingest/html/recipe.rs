//! The HTML ingestion recipe identity: parser, encoding,
//! selectors, and compatibility modes under one canonical encoding
//! (LLR-163).
//!
//! An [`HtmlIngestionRecipe`] is caller-supplied identity metadata
//! recording exactly which HTML ingestion configuration produced a
//! candidate node set: the parser crate name and version, the
//! adapter version, the normalization contract version, the
//! explicitly declared encoding, the optional inclusion root
//! selector, the exclusion selector set, the note and
//! figure-caption classification selector sets, and the enabled
//! compatibility mode set. The recipe is one of three independent
//! identity planes of an ingestion — recipe, input, output — and
//! is bound into every result through its digest.
//!
//! # Canonical byte format
//!
//! [`HtmlIngestionRecipe::canonical_bytes`] encodes under the
//! domain-tagged, length-prefixed framing of the Markdown ingester
//! recipe, with a distinct domain tag so the two recipe families
//! are disjoint and the Markdown encoding —
//! `evidence/ingester-recipe/v1` — is untouched:
//!
//! ```text
//! b"evidence/html-ingester-recipe/v1" || 0x00
//! || str(parser) || str(parser_version)
//! || str(adapter_version) || str(normalization_contract)
//! || str(encoding)
//! || byte(has_inclusion_root) || [str(inclusion_root)]
//! || u64_be(exclusion_selectors.len())
//! || for each selector in sorted order: str(selector)
//! || u64_be(note_selectors.len()) || sorted strs
//! || u64_be(figure_caption_selectors.len()) || sorted strs
//! || u64_be(compatibility_modes.len()) || sorted strs
//! ```
//!
//! where `str(s)` is `u64_be(byte length of s) || s`'s exact UTF-8
//! bytes and `byte(b)` is `0x00` or `0x01`. Every set is a
//! `BTreeSet`, so iteration order — and therefore the encoding —
//! is sorted and insertion-order independent.
//! [`HtmlIngestionRecipe::digest`] is the validated structural
//! digest over those bytes: changing any field — including any
//! recipe selector — moves the recipe identity plane and every
//! comparison output bound to it, while changing only the
//! insertion order of a set does not. The struct is pure data: it
//! performs no validation, because every field value — including
//! an empty one — is encodable and binds a distinct identity.

use std::collections::BTreeSet;

use super::super::super::digest::StructuralContentDigest;

/// Domain/version tag prefixing the recipe encoding.
const RECIPE_DOMAIN_TAG: &[u8] = b"evidence/html-ingester-recipe/v1";

/// The explicit identity of one HTML ingestion configuration
/// (LLR-163).
///
/// All ten fields bind into [`Self::canonical_bytes`] and
/// therefore into [`Self::digest`]. The recipe makes a claim about
/// how ingestion ran; the claim is identity metadata, bound
/// verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlIngestionRecipe {
    /// Parser crate name (e.g. `scraper`).
    pub parser: String,
    /// Parser crate version (e.g. `0.27.0`).
    pub parser_version: String,
    /// Adapter version mapping the DOM to candidate nodes.
    pub adapter_version: String,
    /// Normalization contract version the canonical text follows.
    pub normalization_contract: String,
    /// The explicitly declared input encoding. Only UTF-8 is
    /// supported; declaration is required, and the adapter fails
    /// closed on any other label.
    pub encoding: String,
    /// Optional CSS selector restricting the walk to the first
    /// matching element's subtree.
    pub inclusion_root: Option<String>,
    /// CSS selectors whose matching subtrees are pruned with one
    /// typed structural-loss diagnostic each; iterated in sorted
    /// order, so insertion order is non-semantic.
    pub exclusion_selectors: BTreeSet<String>,
    /// CSS selectors classifying an element without a native
    /// structural mapping as a note/example node; sorted.
    pub note_selectors: BTreeSet<String>,
    /// CSS selectors classifying an element without a native
    /// structural mapping as a figure-caption node; sorted.
    pub figure_caption_selectors: BTreeSet<String>,
    /// Enabled compatibility modes; sorted.
    pub compatibility_modes: BTreeSet<String>,
}

impl HtmlIngestionRecipe {
    /// The canonical byte encoding pinned by the module docs. Pure
    /// and host-independent.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(RECIPE_DOMAIN_TAG);
        bytes.push(0);
        push_str(&mut bytes, &self.parser);
        push_str(&mut bytes, &self.parser_version);
        push_str(&mut bytes, &self.adapter_version);
        push_str(&mut bytes, &self.normalization_contract);
        push_str(&mut bytes, &self.encoding);
        match &self.inclusion_root {
            Some(root) => {
                bytes.push(1);
                push_str(&mut bytes, root);
            }
            None => bytes.push(0),
        }
        push_set(&mut bytes, &self.exclusion_selectors);
        push_set(&mut bytes, &self.note_selectors);
        push_set(&mut bytes, &self.figure_caption_selectors);
        push_set(&mut bytes, &self.compatibility_modes);
        bytes
    }

    /// The recipe identity plane: SHA-256 over
    /// [`Self::canonical_bytes`], as the validated structural
    /// digest domain.
    pub fn digest(&self) -> StructuralContentDigest {
        StructuralContentDigest::from_hasher_output(crate::hash::sha256(&self.canonical_bytes()))
    }
}

/// `str(s)` framing: `u64_be` byte length, then the exact UTF-8
/// bytes.
fn push_str(out: &mut Vec<u8>, value: &str) {
    push_count(out, value.len());
    out.extend_from_slice(value.as_bytes());
}

fn push_count(out: &mut Vec<u8>, count: usize) {
    out.extend_from_slice(&(count as u64).to_be_bytes());
}

/// A set encodes as its count, then each member in sorted
/// (`BTreeSet`) order.
fn push_set(out: &mut Vec<u8>, set: &BTreeSet<String>) {
    push_count(out, set.len());
    for member in set {
        push_str(out, member);
    }
}
