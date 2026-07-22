//! The ingester recipe identity: parser, extensions, adapter, and
//! normalization contract under one canonical encoding (LLR-160).
//!
//! An [`IngesterRecipe`] is caller-supplied identity metadata
//! recording exactly which ingestion configuration produced a
//! candidate node set: the parser crate name, the parser version,
//! the enabled extension set, the adapter version, and the
//! normalization contract version. The recipe is one of three
//! independent identity planes of an ingestion — recipe, input,
//! output — and is bound into every result through its digest.
//!
//! # Canonical byte format
//!
//! [`IngesterRecipe::canonical_bytes`] encodes under the
//! domain-tagged, length-prefixed framing of the source-node
//! normalization encodings, so the recipe encoding is disjoint from
//! every other corpus encoding and no field-boundary collision is
//! possible:
//!
//! ```text
//! b"evidence/ingester-recipe/v1" || 0x00
//! || str(parser) || str(parser_version)
//! || u64_be(extensions.len())
//! || for each extension in sorted order: str(extension)
//! || str(adapter_version) || str(normalization_contract)
//! ```
//!
//! where `str(s)` is `u64_be(byte length of s) || s`'s exact UTF-8
//! bytes. The extension set is a `BTreeSet`, so iteration order —
//! and therefore the encoding — is sorted and insertion-order
//! independent. [`IngesterRecipe::digest`] is the validated
//! structural digest over those bytes; changing any field moves the
//! digest, and changing only the insertion order of extensions does
//! not. The struct is pure data: it performs no validation, because
//! every field value — including an empty one — is encodable and
//! binds a distinct identity.

use std::collections::BTreeSet;

use super::super::digest::StructuralContentDigest;

/// Domain/version tag prefixing the recipe encoding.
const RECIPE_DOMAIN_TAG: &[u8] = b"evidence/ingester-recipe/v1";

/// The explicit identity of one ingester configuration (LLR-160).
///
/// All five fields bind into [`Self::canonical_bytes`] and therefore
/// into [`Self::digest`]. The recipe makes a claim about how
/// ingestion ran; the claim is identity metadata, bound verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngesterRecipe {
    /// Parser crate name (e.g. `pulldown-cmark`).
    pub parser: String,
    /// Parser crate version (e.g. `0.13.4`).
    pub parser_version: String,
    /// Enabled parser extensions (e.g. `tables`, `footnotes`);
    /// iterated in sorted order, so insertion order is non-semantic.
    pub extensions: BTreeSet<String>,
    /// Adapter version mapping parser events to candidate nodes.
    pub adapter_version: String,
    /// Normalization contract version the canonical text follows.
    pub normalization_contract: String,
}

impl IngesterRecipe {
    /// The canonical byte encoding pinned by the module docs. Pure
    /// and host-independent.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(RECIPE_DOMAIN_TAG);
        bytes.push(0);
        push_str(&mut bytes, &self.parser);
        push_str(&mut bytes, &self.parser_version);
        push_count(&mut bytes, self.extensions.len());
        for extension in &self.extensions {
            push_str(&mut bytes, extension);
        }
        push_str(&mut bytes, &self.adapter_version);
        push_str(&mut bytes, &self.normalization_contract);
        bytes
    }

    /// The recipe identity plane: SHA-256 over
    /// [`Self::canonical_bytes`], as the validated structural digest
    /// domain.
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
