//! The PDF ingestion recipe identity: the tool lock plus the
//! committed layout rules under one canonical encoding (LLR-182).
//!
//! A [`PdfIngestionRecipe`] records exactly which extraction and
//! projection configuration produced a candidate node set: the
//! full [`PdfToolLock`] (tool name, version output, per-platform
//! executable digests, argv, adapter version, configuration
//! digest) and the [`PdfLayoutRules`] the projection applies
//! (header and footer bands, the multi-column split, the heading
//! numbering depth, the note and caption prefixes, and the
//! printed page-label prefix). The recipe digests under
//! `evidence/pdf-ingestion-recipe/v1`, so any tool, argv, or
//! rule change moves the recipe identity plane.
//!
//! Floating-point rule fields encode as their IEEE-754 bit
//! patterns, so the encoding is exact and host-independent.

use std::collections::BTreeSet;

use super::super::super::digest::StructuralContentDigest;
use super::lock::PdfToolLock;

/// Domain/version tag prefixing the recipe encoding.
const RECIPE_DOMAIN_TAG: &[u8] = b"evidence/pdf-ingestion-recipe/v1";

/// The committed layout rules of one PDF projection (LLR-182).
/// Coordinates use the extractor's top-left-origin point space.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfLayoutRules {
    /// Lines whose `yMax` is at or above this value are page
    /// headers and drop with a typed diagnostic.
    pub header_bottom: f64,
    /// Lines whose `yMin` is at or below this value are page
    /// footers and drop with a typed diagnostic; a footer line
    /// matching `page_label_prefix` also yields the page's
    /// printed label.
    pub footer_top: f64,
    /// The x-coordinate splitting a two-column page: blocks whose
    /// horizontal midpoint falls left of the split read before
    /// blocks right of it. `None` disables column handling.
    pub column_split_x: Option<f64>,
    /// The maximum dotted-numbering depth (`1`, `1.2`,
    /// `1.2.3`, …) that still classifies a single-line block as a
    /// numbered heading.
    pub max_heading_depth: u32,
    /// First-word prefixes classifying a block as a note
    /// (e.g. `NOTE`).
    pub note_prefixes: BTreeSet<String>,
    /// First-word prefixes classifying a block as a figure
    /// caption (e.g. `Figure`).
    pub caption_prefixes: BTreeSet<String>,
    /// The footer-line prefix whose remainder is the page's
    /// printed label (e.g. `Page ` → `3`). `None` disables
    /// printed-label extraction.
    pub page_label_prefix: Option<String>,
}

/// The explicit identity of one PDF ingestion configuration
/// (LLR-182). All fields bind into [`Self::canonical_bytes`] and
/// therefore into [`Self::digest`].
#[derive(Debug, Clone, PartialEq)]
pub struct PdfIngestionRecipe {
    /// The extractor tool lock this ingestion ran under.
    pub tool_lock: PdfToolLock,
    /// The committed layout rules the projection applied.
    pub rules: PdfLayoutRules,
}

impl PdfIngestionRecipe {
    /// The canonical byte encoding pinned by the module docs.
    /// Pure and host-independent.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(RECIPE_DOMAIN_TAG);
        bytes.push(0);
        let lock_bytes = self.tool_lock.canonical_bytes();
        push_count(&mut bytes, lock_bytes.len());
        bytes.extend_from_slice(&lock_bytes);
        push_f64(&mut bytes, self.rules.header_bottom);
        push_f64(&mut bytes, self.rules.footer_top);
        match self.rules.column_split_x {
            Some(split) => {
                bytes.push(1);
                push_f64(&mut bytes, split);
            }
            None => bytes.push(0),
        }
        push_count(&mut bytes, self.rules.max_heading_depth as usize);
        push_count(&mut bytes, self.rules.note_prefixes.len());
        for prefix in &self.rules.note_prefixes {
            push_str(&mut bytes, prefix);
        }
        push_count(&mut bytes, self.rules.caption_prefixes.len());
        for prefix in &self.rules.caption_prefixes {
            push_str(&mut bytes, prefix);
        }
        match &self.rules.page_label_prefix {
            Some(prefix) => {
                bytes.push(1);
                push_str(&mut bytes, prefix);
            }
            None => bytes.push(0),
        }
        bytes
    }

    /// The recipe identity plane: SHA-256 over
    /// [`Self::canonical_bytes`], as the validated structural
    /// digest domain.
    pub fn digest(&self) -> StructuralContentDigest {
        StructuralContentDigest::from_hasher_output(crate::hash::sha256(&self.canonical_bytes()))
    }

    /// The rule sanity invariants the type system cannot express:
    /// bands are finite, non-negative, and ordered, and the
    /// heading depth is nonzero.
    pub(crate) fn validate_rules(&self) -> Result<(), &'static str> {
        let finite = |value: f64| value.is_finite() && value >= 0.0;
        if !finite(self.rules.header_bottom)
            || !finite(self.rules.footer_top)
            || self.rules.header_bottom >= self.rules.footer_top
        {
            return Err("header/footer bands must be finite, non-negative, and ordered");
        }
        if let Some(split) = self.rules.column_split_x
            && !finite(split)
        {
            return Err("the column split must be finite and non-negative");
        }
        if self.rules.max_heading_depth == 0 {
            return Err("the maximum heading depth must be nonzero");
        }
        Ok(())
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

fn push_f64(out: &mut Vec<u8>, value: f64) {
    out.extend_from_slice(&value.to_bits().to_be_bytes());
}
