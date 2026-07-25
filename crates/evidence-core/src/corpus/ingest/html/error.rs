//! The HTML ingestion error and structural-loss diagnostic
//! taxonomy (LLR-164).
//!
//! Every fail-closed contract failure is a flat typed
//! [`HtmlIngestError`] variant carrying the conflicting values —
//! the offending media type, the declared and recomputed digests,
//! the rejected encoding, the bound and the observed value. The
//! family is deliberately uncoded (no
//! [`crate::diagnostic::DiagnosticCode`] impl), following the
//! [`super::super::IngestError`] precedent: ingestion runs inside
//! flows that already own the diagnostic surface, so a second code
//! family would double-report.
//!
//! Structural loss that does not abort ingestion — a configured
//! exclusion, a closed-rule drop, an unsupported element, a
//! duplicate explicit anchor, a dangling internal link — is a
//! typed [`HtmlIngestDiagnostic`] carrying a DOM-path locator and
//! a typed reason. Silent dropping is forbidden: every element
//! the projection cannot represent produces exactly one
//! diagnostic at the point of loss.

use thiserror::Error;

use super::super::super::SourceGraphError;
use super::super::super::digest::StructuralContentDigest;

/// The typed kind of one HTML structural-loss diagnostic
/// (LLR-164). `Ord` is the derived variant-then-payload order,
/// used only for deterministic sorting.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum HtmlIngestDiagnosticKind {
    /// An element subtree matched a recipe exclusion selector and
    /// was pruned from the walk.
    ExcludedByRecipe {
        /// The first matching exclusion selector in sorted order.
        selector: String,
    },
    /// An element dropped by the closed rule: script, style,
    /// template, and non-content metadata (`head`, `title`,
    /// `meta`, `link`, `noscript`).
    DroppedByClosedRule {
        /// The dropped tag name, lowercase.
        tag: &'static str,
    },
    /// An element the structural projection cannot represent. Its
    /// text content is retained unless the element is foreign
    /// content (`svg`, `math`) or embedded content (`iframe`,
    /// `object`, `embed`, `audio`, `video`, `canvas`), whose
    /// subtree is skipped.
    UnsupportedElement {
        /// The unsupported tag name, lowercase.
        tag: String,
    },
    /// An explicit `id` was claimed a second or later time.
    DuplicateAnchor {
        /// The duplicated anchor value.
        anchor: String,
    },
    /// A pure-fragment internal link whose target is absent from
    /// the retained document's id set.
    DanglingInternalLink {
        /// The unresolved fragment (without the leading `#`).
        fragment: String,
    },
}

/// One typed HTML structural-loss diagnostic (LLR-164): what was
/// found, where in the DOM it sits, and a human-readable detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlIngestDiagnostic {
    /// The typed kind.
    pub kind: HtmlIngestDiagnosticKind,
    /// Element-child indexes from the document root locating the
    /// element the diagnostic names; diagnostic, never identity.
    pub dom_path: Vec<u32>,
    /// Human-readable context (never identity).
    pub detail: String,
}

/// Every fail-closed HTML ingestion contract failure (LLR-164).
/// Deliberately uncoded; see the module docs.
#[derive(Debug, Error)]
pub enum HtmlIngestError {
    /// The media type assertion is not `text/html`.
    #[error("media type {found:?} is not the required \"text/html\"")]
    MediaTypeMismatch {
        /// The asserted media type.
        found: String,
    },
    /// The supplied bytes do not re-digest to the declared input
    /// digest — they are not the verified material.
    #[error(
        "input digest mismatch: declared {declared}, recomputed {recomputed} over the supplied bytes"
    )]
    InputDigestMismatch {
        /// The declared verified digest.
        declared: StructuralContentDigest,
        /// The digest recomputed over the supplied bytes.
        recomputed: StructuralContentDigest,
    },
    /// The source-revision uid is not a corpus-native `src_` UUIDv4.
    #[error("source revision uid {uid:?} is not a corpus-native src_ UUIDv4 uid")]
    InvalidSourceRevisionUid {
        /// The offending uid.
        uid: String,
    },
    /// A URL field lacks the absolute `<scheme>://` lexical shape.
    /// URLs are opaque audit identity: they are never fetched,
    /// resolved, or normalized.
    #[error("{field} {value:?} lacks an absolute <scheme>:// shape")]
    InvalidUrl {
        /// The offending field (`canonical_url` or `final_url`).
        field: &'static str,
        /// The offending value.
        value: String,
    },
    /// The recipe declares no encoding; the contract requires an
    /// explicit declaration rather than platform defaults or
    /// content sniffing.
    #[error("the recipe declares no encoding; an explicit encoding declaration is required")]
    MissingEncoding,
    /// The recipe declares an encoding other than UTF-8.
    #[error("encoding {encoding:?} is not supported; only \"utf-8\" is accepted")]
    UnsupportedEncoding {
        /// The declared encoding label.
        encoding: String,
    },
    /// The input is not valid UTF-8; decoding is never lossy.
    #[error("input is not valid UTF-8; first invalid sequence starts at byte offset {offset}")]
    NonUtf8 {
        /// Byte offset of the first invalid sequence.
        offset: usize,
    },
    /// The raw bytes declare an external entity or another
    /// construct that would require network or file resolution.
    /// The parser never expands entities; the scan rejects the
    /// attempt fail-closed.
    #[error("input declares a construct requiring external resolution: {detail}")]
    ExternalResolution {
        /// What was found.
        detail: String,
    },
    /// The input exceeds the byte-size bound.
    #[error("input is {size} bytes, exceeding the {limit}-byte bound")]
    InputTooLarge {
        /// The input size in bytes.
        size: usize,
        /// The bound.
        limit: usize,
    },
    /// The retained DOM nests deeper than the depth bound.
    #[error("DOM nests {depth} elements deep, exceeding the {limit}-element bound")]
    NestingTooDeep {
        /// The observed element nesting depth below the walk root.
        depth: usize,
        /// The bound.
        limit: usize,
    },
    /// The candidate set exceeds the node-count bound.
    #[error("ingestion produced {count} candidate nodes, exceeding the {limit}-node bound")]
    TooManyNodes {
        /// The observed candidate count.
        count: usize,
        /// The bound.
        limit: usize,
    },
    /// One element carries more attributes than the bound.
    #[error("element <{tag}> carries {count} attributes, exceeding the {limit}-attribute bound")]
    TooManyAttributes {
        /// The element's tag name.
        tag: String,
        /// The observed attribute count.
        count: usize,
        /// The bound.
        limit: usize,
    },
    /// One node's canonical text exceeds the text-size bound.
    #[error("a node's canonical text is {size} bytes, exceeding the {limit}-byte bound")]
    TextTooLarge {
        /// The canonical text size in bytes.
        size: usize,
        /// The bound.
        limit: usize,
    },
    /// A recipe selector fails to parse as a CSS selector.
    #[error("recipe selector {selector:?} is not a valid CSS selector: {detail}")]
    InvalidSelector {
        /// The offending selector.
        selector: String,
        /// The parser's reason.
        detail: String,
    },
    /// The recipe's inclusion root selector matches no element.
    #[error("inclusion root selector {selector:?} matches no element in the document")]
    InclusionRootNotFound {
        /// The unmatched selector.
        selector: String,
    },
    /// The assembled candidate set violates the source-graph forest
    /// invariants (parents, kinds, ordinals, digests, or identity
    /// uniqueness).
    #[error("candidate graph violates the source-graph invariants")]
    CandidateGraph(#[from] SourceGraphError),
}
