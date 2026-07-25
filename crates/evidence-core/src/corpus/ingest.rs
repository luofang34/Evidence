//! Structure-preserving ingestion of verified frozen
//! Markdown/CommonMark-GFM bytes into candidate structural source
//! nodes (LLR-161).
//!
//! [`ingest_markdown`] is a pure core API over verified bytes plus
//! explicit metadata: it performs no fetch, no filesystem access, no
//! environment reads, no workspace mutation, and no baseline
//! replacement. The caller selects the effective source revision and
//! completes material verification first; the adapter then requires
//! the frozen bytes, the Markdown media type assertion, the
//! source-revision uid, the canonical source path, the verified
//! input digest, and an explicit [`IngesterRecipe`]. Local-file
//! metadata may carry a git blob SHA; the frozen source digest
//! remains authoritative.
//!
//! # Input contract
//!
//! The contract validates before parsing, in a documented fail-fast
//! order so the reported error is deterministic:
//!
//! 1. **Media type** — must be `text/markdown`
//!    ([`MARKDOWN_MEDIA_TYPE`], ASCII case-insensitive).
//! 2. **Input digest** — the bytes must re-digest to the declared
//!    digest; a mismatch means the bytes are not the verified
//!    material.
//! 3. **Revision uid** — must satisfy the corpus-native `src_`
//!    UUIDv4 contract.
//! 4. **Canonical path** — must satisfy the vendored wire-path
//!    rules ([`SafeRelPath`]).
//! 5. **Git blob** — when present, must be 40- or 64-character
//!    lowercase hexadecimal.
//! 6. **UTF-8** — the bytes must decode; the error carries the byte
//!    offset of the first invalid sequence, and decoding is never
//!    lossy.
//!
//! Every failure is a flat typed [`IngestError`] variant carrying
//! the conflicting values. The family is deliberately uncoded (no
//! [`crate::diagnostic::DiagnosticCode`] impl), following the
//! [`super::CorpusError`] precedent: ingestion runs inside flows
//! that already own the diagnostic surface, so a second code family
//! would double-report.
//!
//! # Output
//!
//! [`MarkdownIngestion`] carries the candidate nodes (identities
//! minted through the structural identity service, in document
//! order), the sorted typed [`IngestDiagnostic`] list, and the
//! `output_digest` over the canonical node projection (`projection`
//! module). Recipe, input, and output digests are three independent
//! identity planes: each covers only its own projection, so a
//! targeted mutation of one plane moves exactly one digest.
//!
//! diagnostics are typed — duplicate anchor, malformed explicit id,
//! unsupported raw HTML, lossy construct — each carrying a byte
//! range into the frozen source, sorted deterministically by range,
//! kind, and detail. Silent dropping is forbidden: every construct
//! the projection cannot represent produces a diagnostic.
//!
//! The `html` submodule applies the same contract, identity planes,
//! and canonical node builder to verified frozen HTML bytes: same
//! reconciliation, normalization, and assembly, a parallel typed
//! error and diagnostic taxonomy with DOM-path locators, and its own
//! recipe and projection domain tags so the two format families
//! stay disjoint (LLR-163..LLR-165).
//!
//! Module map:
//!
//! - `recipe` — the [`IngesterRecipe`] identity with canonical byte
//!   encoding and digest (LLR-160)
//! - `markdown` — the parser adapter mapping events into candidate
//!   nodes (LLR-162)
//! - `html` — offline structure-preserving HTML ingestion
//!   (LLR-163..LLR-165)
//! - `projection` — the canonical uid-free node projection and the
//!   output digest (LLR-161)

use thiserror::Error;

use super::digest::StructuralContentDigest;
use super::records::validate_native_uid;
use super::source::SOURCE_UID_PREFIX;
use super::{SafeRelPath, SourceGraphError, SourceNode, VendoredPathRule};

pub(super) mod html;
pub(super) mod markdown;
pub mod pdf;
pub(super) mod projection;
pub(super) mod recipe;

pub use html::{
    HTML_MEDIA_TYPE, HtmlIngestDiagnostic, HtmlIngestDiagnosticKind, HtmlIngestError,
    HtmlIngestion, HtmlIngestionRecipe, IngestHtmlInput, ingest_html,
};
pub use markdown::ingest_markdown;
pub use pdf::{
    IngestPdfInput, PDF_MEDIA_TYPE, PdfExcludedBand, PdfIngestDiagnostic, PdfIngestDiagnosticKind,
    PdfIngestError, PdfIngestion, PdfIngestionRecipe, PdfLayoutRules, ingest_pdf,
};
pub use recipe::IngesterRecipe;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "ingest/tests.rs"]
mod tests;

/// The RFC 6838 media type the ingestion contract requires
/// (LLR-161). Compared ASCII case-insensitively.
pub const MARKDOWN_MEDIA_TYPE: &str = "text/markdown";

/// The verified input of one Markdown ingestion (LLR-161): frozen
/// bytes plus the explicit metadata the contract binds.
#[derive(Debug, Clone)]
pub struct IngestMarkdownInput<'a> {
    /// The frozen source bytes, already material-verified by the
    /// caller against `input_digest`.
    pub bytes: &'a [u8],
    /// The media type assertion; must be [`MARKDOWN_MEDIA_TYPE`].
    pub media_type: &'a str,
    /// The `src_<UUIDv4>` revision the bytes were frozen as.
    pub source_revision_uid: &'a str,
    /// Canonical relative path of the source document.
    pub canonical_path: &'a str,
    /// The verified digest of `bytes`; re-computed and compared
    /// before parsing.
    pub input_digest: StructuralContentDigest,
    /// Optional git blob SHA of the file revision; the frozen source
    /// digest remains authoritative.
    pub git_blob: Option<String>,
    /// The explicit ingester recipe identity.
    pub recipe: IngesterRecipe,
}

/// The result of one Markdown ingestion (LLR-161).
#[derive(Debug, Clone)]
pub struct MarkdownIngestion {
    /// Candidate nodes with minted identities, in document order.
    pub nodes: Vec<SourceNode>,
    /// Sorted typed diagnostics; empty when the document projects
    /// losslessly.
    pub diagnostics: Vec<IngestDiagnostic>,
    /// The output identity plane: SHA-256 over the canonical node
    /// projection (`projection` module).
    pub output_digest: StructuralContentDigest,
}

impl MarkdownIngestion {
    /// The canonical uid-free projection of the candidate nodes;
    /// `output_digest` is SHA-256 over these bytes.
    pub fn canonical_projection(&self) -> Vec<u8> {
        projection::render_candidate_projection(&self.nodes)
    }
}

/// The typed kind of one ingestion diagnostic (LLR-161). `Ord` is
/// the derived variant-then-payload order, used only for
/// deterministic sorting.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum IngestDiagnosticKind {
    /// An explicit heading anchor was claimed a second or later
    /// time.
    DuplicateAnchor {
        /// The duplicated anchor value.
        anchor: String,
    },
    /// A trailing `{#...}` heading suffix whose contents are not a
    /// valid explicit id.
    MalformedExplicitId {
        /// The raw contents between the braces.
        raw: String,
    },
    /// Raw HTML (block or inline), which the structural projection
    /// does not represent.
    UnsupportedRawHtml,
    /// A construct the projection cannot represent faithfully.
    LossyConstruct {
        /// Closed construct name: `image`, `thematic-break`, or
        /// `footnote-definition` (a duplicate definition).
        construct: &'static str,
    },
}

/// One typed ingestion diagnostic (LLR-161): what was found, where
/// in the frozen source, and a human-readable detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestDiagnostic {
    /// The typed kind.
    pub kind: IngestDiagnosticKind,
    /// Byte range into the frozen source the diagnostic names.
    pub byte_range: (u64, u64),
    /// Human-readable context (never identity).
    pub detail: String,
}

/// Every fail-closed ingestion contract failure (LLR-161). Deliberately
/// uncoded; see the module docs.
#[derive(Debug, Error)]
pub enum IngestError {
    /// The media type assertion is not `text/markdown`.
    #[error("media type {found:?} is not the required {MARKDOWN_MEDIA_TYPE:?}")]
    MediaTypeMismatch {
        /// The asserted media type.
        found: String,
    },
    /// The source-revision uid is not a corpus-native `src_` UUIDv4.
    #[error("source revision uid {uid:?} is not a corpus-native src_ UUIDv4 uid")]
    InvalidSourceRevisionUid {
        /// The offending uid.
        uid: String,
    },
    /// The canonical path violates the vendored wire-path rules.
    #[error("canonical path {path:?} violates the wire-path rules: {rule}")]
    InvalidCanonicalPath {
        /// The offending path.
        path: String,
        /// The rule it violated.
        rule: VendoredPathRule,
    },
    /// The git blob identifier is not 40/64-character lowercase hex.
    #[error("git blob {blob:?} is not 40- or 64-character lowercase hexadecimal")]
    InvalidGitBlob {
        /// The offending blob identifier.
        blob: String,
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
    /// The input is not valid UTF-8.
    #[error("input is not valid UTF-8; first invalid sequence starts at byte offset {offset}")]
    NonUtf8 {
        /// Byte offset of the first invalid sequence.
        offset: usize,
    },
    /// A byte range handed to the locator constructor is unaligned
    /// or out of bounds. Parser-produced ranges cannot trip this;
    /// the check keeps construction fail-closed.
    #[error("byte range [{start}, {end}] is not UTF-8-aligned within the {len}-byte input")]
    ByteRangeUnaligned {
        /// Range start.
        start: u64,
        /// Range end.
        end: u64,
        /// Input length in bytes.
        len: usize,
    },
    /// The assembled candidate set violates the source-graph forest
    /// invariants (parents, kinds, ordinals, digests, or identity
    /// uniqueness).
    #[error("candidate graph violates the source-graph invariants")]
    CandidateGraph(#[from] SourceGraphError),
}

/// Validate the input contract in the module docs' fail-fast order,
/// returning the decoded text and the validated path.
///
/// # Errors
///
/// The first contract violation wins, so error precedence is
/// deterministic.
pub(crate) fn validate_input<'i>(
    input: &IngestMarkdownInput<'i>,
) -> Result<(&'i str, SafeRelPath), IngestError> {
    if !input.media_type.eq_ignore_ascii_case(MARKDOWN_MEDIA_TYPE) {
        return Err(IngestError::MediaTypeMismatch {
            found: input.media_type.to_string(),
        });
    }
    let recomputed = StructuralContentDigest::from_hasher_output(crate::hash::sha256(input.bytes));
    if recomputed != input.input_digest {
        return Err(IngestError::InputDigestMismatch {
            declared: input.input_digest.clone(),
            recomputed,
        });
    }
    validate_native_uid(
        input.source_revision_uid,
        SOURCE_UID_PREFIX,
        |uid, _expected| IngestError::InvalidSourceRevisionUid { uid },
        |uid| IngestError::InvalidSourceRevisionUid { uid },
    )?;
    let path = SafeRelPath::new(input.canonical_path).map_err(|rule| {
        IngestError::InvalidCanonicalPath {
            path: input.canonical_path.to_string(),
            rule,
        }
    })?;
    if let Some(blob) = &input.git_blob {
        let valid = matches!(blob.len(), 40 | 64)
            && blob
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !valid {
            return Err(IngestError::InvalidGitBlob { blob: blob.clone() });
        }
    }
    let text = std::str::from_utf8(input.bytes).map_err(|err| IngestError::NonUtf8 {
        offset: err.valid_up_to(),
    })?;
    Ok((text, path))
}
