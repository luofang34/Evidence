//! Canonical text normalization, the source-node content digest,
//! and the structural fingerprint (LLR-154).
//!
//! Two disjoint normalization contracts, dispatched by node kind:
//!
//! - [`normalize_prose`] — Unicode NFC, then every whitespace run
//!   folds to one ASCII space, then the ends trim. Applies to
//!   every kind except [`SourceNodeKind::CodeBlock`].
//! - [`normalize_code`] — Unicode NFC, then CRLF and lone CR line
//!   endings map to LF. Significant spaces and line boundaries are
//!   preserved exactly; prose folding never touches code.
//!
//! [`content_digest`] binds a node's kind and its exact canonical
//! text bytes; [`fingerprint`] binds a node's kind, optional
//! label, and ordered root-to-parent ancestry of `(kind, label)`
//! pairs. Both encode under length-prefixed framing (the
//! `envelope_bytes` precedent) with a domain/version tag, so no
//! field-boundary collision is possible and the two encodings are
//! disjoint from every other corpus encoding.
//!
//! # Canonical byte formats
//!
//! `content_digest` covers:
//!
//! ```text
//! b"evidence/source-node-content/v1" || 0x00
//! || str(kind wire name) || str(canonical text)
//! ```
//!
//! `fingerprint` covers:
//!
//! ```text
//! b"evidence/source-node-fingerprint/v1" || 0x00
//! || str(kind wire name) || opt(label)
//! || u64_be(ancestry.len())
//! || for each ancestor, root to parent: str(kind) || opt(label)
//! ```
//!
//! where `str(s)` is `u64_be(byte length of s) || s`'s exact UTF-8
//! bytes and `opt(o)` is the all-ones sentinel `0xFFFFFFFFFFFFFFFF`
//! for `None`, or `str(v)` for `Some(v)`. All lengths and counts
//! are unsigned 64-bit big-endian.
//!
//! Diagnostic positions — page, DOM, byte, line, and sibling
//! ordinal — never enter either encoding, so a pure re-layout
//! moves no digest and no fingerprint. Changing either contract
//! requires a new encoding version, never a silent change of
//! existing digests.

use unicode_normalization::UnicodeNormalization;

use super::super::digest::StructuralContentDigest;
use super::SourceNodeKind;

/// Domain/version tag prefixing the content-digest encoding.
const CONTENT_DOMAIN_TAG: &[u8] = b"evidence/source-node-content/v1";

/// Domain/version tag prefixing the fingerprint encoding.
const FINGERPRINT_DOMAIN_TAG: &[u8] = b"evidence/source-node-fingerprint/v1";

/// `None` sentinel for optional fields: the all-ones `u64`, which
/// no real byte length can reach.
const NONE_SENTINEL: u64 = u64::MAX;

/// Normalize prose to its canonical form: Unicode NFC, every
/// whitespace run folded to one ASCII space, ends trimmed.
pub fn normalize_prose(text: &str) -> String {
    let nfc: String = text.nfc().collect();
    let mut out = String::with_capacity(nfc.len());
    let mut pending_space = false;
    for ch in nfc.chars() {
        if ch.is_whitespace() {
            pending_space = true;
        } else {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.push(ch);
        }
    }
    out
}

/// Normalize code to its canonical form: Unicode NFC, then CRLF
/// and lone CR line endings mapped to LF. Significant spaces and
/// line boundaries are preserved; no prose folding applies.
pub fn normalize_code(text: &str) -> String {
    let nfc: String = text.nfc().collect();
    nfc.replace("\r\n", "\n").replace('\r', "\n")
}

/// The content digest of one source node: SHA-256 over the
/// domain-tagged framing of the kind's wire name and the exact
/// canonical text bytes (LLR-154). `canonical_text` is digested as
/// given — producing canonical text is the ingester's obligation
/// through [`normalize_prose`] / [`normalize_code`], and the
/// digest binds whatever text the record stores.
pub fn content_digest(kind: SourceNodeKind, canonical_text: &str) -> StructuralContentDigest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(CONTENT_DOMAIN_TAG);
    bytes.push(0);
    push_str(&mut bytes, kind.as_str());
    push_str(&mut bytes, canonical_text);
    StructuralContentDigest::from_hasher_output(crate::hash::sha256(&bytes))
}

/// The structural fingerprint of one source node: SHA-256 over
/// the domain-tagged framing of the kind, the optional label, and
/// the ordered root-to-parent ancestry of `(kind, label)` pairs
/// (LLR-154). Diagnostic positions never enter the encoding, so
/// the fingerprint is stable across pure re-layouts and across
/// revisions of one document.
pub fn fingerprint(
    kind: SourceNodeKind,
    label: Option<&str>,
    ancestry: &[(SourceNodeKind, Option<&str>)],
) -> StructuralContentDigest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(FINGERPRINT_DOMAIN_TAG);
    bytes.push(0);
    push_str(&mut bytes, kind.as_str());
    push_opt(&mut bytes, label);
    push_count(&mut bytes, ancestry.len());
    for (ancestor_kind, ancestor_label) in ancestry {
        push_str(&mut bytes, ancestor_kind.as_str());
        push_opt(&mut bytes, *ancestor_label);
    }
    StructuralContentDigest::from_hasher_output(crate::hash::sha256(&bytes))
}

/// `str(s)` framing: `u64_be` byte length, then the exact UTF-8
/// bytes.
fn push_str(out: &mut Vec<u8>, value: &str) {
    push_count(out, value.len());
    out.extend_from_slice(value.as_bytes());
}

/// `opt(o)` framing: the all-ones sentinel for `None`, `str(v)`
/// for `Some(v)`.
fn push_opt(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => push_str(out, value),
        None => out.extend_from_slice(&NONE_SENTINEL.to_be_bytes()),
    }
}

fn push_count(out: &mut Vec<u8>, count: usize) {
    out.extend_from_slice(&(count as u64).to_be_bytes());
}
