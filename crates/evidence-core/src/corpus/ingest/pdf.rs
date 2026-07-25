//! Pinned offline ingestion of verified frozen PDF bytes through
//! the Poppler `pdftotext -bbox-layout` adapter (LLR-182).
//!
//! The adapter is split into a blocking extraction stage and a
//! pure projection stage:
//!
//! - [`run_pdftotext_blocking`] (`runner` module, LLR-180)
//!   executes the locked extractor — explicit executable path,
//!   pinned argv, isolated temporary directory, bounded time and
//!   output — and returns the raw extractor output bytes and their
//!   digest. The [`PdfToolLock`] (`lock` module, LLR-179) pins and
//!   verifies the tool identity before any document parse.
//! - [`ingest_pdf`] is a pure core API over the frozen PDF bytes
//!   (digest-verified only — parsing PDF internals is the
//!   extractor's job) plus the raw extractor output: it validates
//!   the input contract, parses the bbox-layout XHTML through the
//!   bounded fail-closed parser (`bbox` module, LLR-181), and
//!   projects the layout model into candidate structural source
//!   nodes under the explicit [`PdfIngestionRecipe`] (`recipe`
//!   module).
//!
//! # Input contract
//!
//! The contract validates before parsing, in a documented
//! fail-fast order so the reported error is deterministic:
//!
//! 1. **Media type** — must be `application/pdf`
//!    ([`PDF_MEDIA_TYPE`], ASCII case-insensitive).
//! 2. **Input digest** — the PDF bytes must re-digest to the
//!    declared digest.
//! 3. **Revision uid** — must satisfy the corpus-native `src_`
//!    UUIDv4 contract.
//! 4. **Canonical path** — must satisfy the vendored wire-path
//!    rules ([`SafeRelPath`]).
//! 5. **Recipe** — the tool lock must validate and the layout
//!    rules must be sane.
//! 6. **Extractor output** — must parse under the bounded
//!    fail-closed bbox parser.
//!
//! # Output
//!
//! [`PdfIngestion`] carries the candidate nodes (identities
//! minted through the structural identity service, in reading
//! order), the sorted typed [`PdfIngestDiagnostic`] list, the raw
//! extractor-output digest (an output-identity component beside
//! the recipe, input, and output digests), and the
//! `output_digest` over the canonical node projection
//! (`evidence/pdf-ingest-output/v1` in the shared `projection`
//! module).
//!
//! Structural-loss diagnostics are typed — header/footer rule
//! exclusions, unprovable table-shaped blocks, unclassifiable
//! blocks — each carrying the page, block index, and bounding
//! box, sorted deterministically. Silent dropping is forbidden;
//! no table row or cell node is ever claimed without a proving
//! rule: parser-hostile tables recover through approved curated
//! patches (#219/#220), never through inference.
//!
//! Module map:
//!
//! - `lock` — the [`PdfToolLock`] schema and executable
//!   verification (LLR-179)
//! - `runner` — the bounded offline blocking runner (LLR-180)
//! - `bbox` — the bounded fail-closed bbox-layout parser
//!   (LLR-181)
//! - `recipe` — the [`PdfIngestionRecipe`] identity (LLR-182)
//! - `project` — the layout projection and structural-loss
//!   diagnostics (LLR-182)

use std::collections::BTreeMap;

use thiserror::Error;

use super::super::digest::StructuralContentDigest;
use super::super::records::validate_native_uid;
use super::super::source::SOURCE_UID_PREFIX;
use super::super::source_graph::validate::validate_graph_standalone;
use super::super::{
    CandidateNode, SafeRelPath, SourceGraph, SourceGraphError, SourceNode, SourceNodeKind,
    VendoredPathRule, content_digest, fingerprint, reconcile,
};

pub mod bbox;
pub mod lock;
mod project;
pub mod recipe;
pub mod runner;

pub use recipe::{PdfIngestionRecipe, PdfLayoutRules};

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "pdf_tests.rs"]
mod tests;

/// The RFC 6838 media type the PDF ingestion contract requires
/// (LLR-182). Compared ASCII case-insensitively.
pub const PDF_MEDIA_TYPE: &str = "application/pdf";

/// The verified input of one PDF ingestion (LLR-182): the frozen
/// PDF bytes plus the raw extractor output and the explicit
/// metadata the contract binds.
#[derive(Debug, Clone)]
pub struct IngestPdfInput<'a> {
    /// The frozen PDF bytes, already material-verified by the
    /// caller against `input_digest`. Re-digested for comparison
    /// only; the adapter never parses PDF internals.
    pub bytes: &'a [u8],
    /// The media type assertion; must be [`PDF_MEDIA_TYPE`].
    pub media_type: &'a str,
    /// The `src_<UUIDv4>` revision the bytes were frozen as.
    pub source_revision_uid: &'a str,
    /// Canonical relative path of the source document.
    pub canonical_path: &'a str,
    /// The verified digest of `bytes`; re-computed and compared
    /// before parsing.
    pub input_digest: StructuralContentDigest,
    /// The raw extractor output bytes (the bbox-layout XHTML) the
    /// blocking runner produced over `bytes`.
    pub extractor_output: &'a [u8],
    /// The explicit PDF ingestion recipe identity.
    pub recipe: PdfIngestionRecipe,
}

/// The result of one PDF ingestion (LLR-182).
#[derive(Debug, Clone)]
pub struct PdfIngestion {
    /// Candidate nodes with minted identities, in reading order.
    pub nodes: Vec<SourceNode>,
    /// Sorted typed structural-loss diagnostics; empty when the
    /// retained layout projects losslessly.
    pub diagnostics: Vec<PdfIngestDiagnostic>,
    /// The raw extractor-output digest: SHA-256 over the
    /// extractor's bbox-layout bytes; an output-identity
    /// component.
    pub extractor_output_digest: StructuralContentDigest,
    /// The output identity plane: SHA-256 over the canonical node
    /// projection (`projection` module).
    pub output_digest: StructuralContentDigest,
}

impl PdfIngestion {
    /// The canonical uid-free projection of the candidate nodes;
    /// `output_digest` is SHA-256 over these bytes.
    pub fn canonical_projection(&self) -> Vec<u8> {
        super::projection::render_pdf_projection(&self.nodes)
    }
}

/// The band a header/footer rule excluded a line from (LLR-182).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PdfExcludedBand {
    /// The page header band.
    Header,
    /// The page footer band.
    Footer,
}

/// The typed kind of one PDF structural-loss diagnostic
/// (LLR-182). `Ord` is the derived variant-then-payload order,
/// used only for deterministic sorting.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PdfIngestDiagnosticKind {
    /// A line fell inside a configured header or footer band and
    /// was excluded by rule.
    ExcludedByRule {
        /// The excluded band.
        band: PdfExcludedBand,
    },
    /// A block the committed rules cannot classify: it carries no
    /// words.
    UnclassifiableBlock,
    /// A table-shaped block the committed rules cannot prove: no
    /// row or cell structure is claimed; recovery is an approved
    /// curated patch's job.
    StructuralLoss {
        /// The closed construct name: `table`.
        construct: &'static str,
    },
}

/// One typed PDF structural-loss diagnostic (LLR-182): what was
/// found, on which page and block, and the element bounding box.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfIngestDiagnostic {
    /// The typed kind.
    pub kind: PdfIngestDiagnosticKind,
    /// The 1-based physical page index.
    pub page: u32,
    /// The 0-based block index within the page, in extractor
    /// order.
    pub block: u32,
    /// The offending element's bounding box.
    pub bbox: bbox::Bbox,
    /// Human-readable context (never identity).
    pub detail: String,
}

/// Every fail-closed PDF ingestion contract failure (LLR-182).
/// Deliberately uncoded, following the [`super::IngestError`]
/// precedent.
#[derive(Debug, Error)]
pub enum PdfIngestError {
    /// The media type assertion is not `application/pdf`.
    #[error("media type {found:?} is not the required {PDF_MEDIA_TYPE:?}")]
    MediaTypeMismatch {
        /// The asserted media type.
        found: String,
    },
    /// The supplied PDF bytes do not re-digest to the declared
    /// input digest.
    #[error(
        "input digest mismatch: declared {declared}, recomputed {recomputed} over the supplied bytes"
    )]
    InputDigestMismatch {
        /// The declared verified digest.
        declared: StructuralContentDigest,
        /// The digest recomputed over the supplied bytes.
        recomputed: StructuralContentDigest,
    },
    /// The source-revision uid is not a corpus-native `src_`
    /// UUIDv4.
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
    /// The recipe's tool lock is invalid.
    #[error("the recipe's tool lock is invalid: {0}")]
    ToolLock(#[from] lock::PdfToolLockError),
    /// The recipe's layout rules are insane.
    #[error("the recipe's layout rules are invalid: {detail}")]
    InvalidRules {
        /// What is wrong.
        detail: &'static str,
    },
    /// The extractor output failed the bounded bbox parse.
    #[error("extractor output failed the bbox-layout parse: {0}")]
    Bbox(#[from] bbox::BboxParseError),
    /// The assembled candidate set violates the source-graph
    /// forest invariants.
    #[error("candidate graph violates the source-graph invariants")]
    CandidateGraph(#[from] SourceGraphError),
}

/// Ingest verified frozen PDF bytes plus the raw extractor output
/// into candidate structural source nodes (LLR-182). Pure: no
/// I/O, no fetch, no filesystem access, no workspace mutation.
///
/// # Errors
///
/// Fails closed with [`PdfIngestError`] on any contract
/// violation, any bbox-parse violation, or when the assembled
/// candidate set violates the source-graph invariants.
pub fn ingest_pdf(input: &IngestPdfInput) -> Result<PdfIngestion, PdfIngestError> {
    validate_pdf_input(input)?;
    let document = bbox::parse_bbox_layout(input.extractor_output)?;
    let projection = project::project(&document, &input.recipe.rules);
    assemble(input, projection)
}

/// Validate the input contract in the module docs' fail-fast
/// order.
///
/// # Errors
///
/// The first contract violation wins, so error precedence is
/// deterministic.
fn validate_pdf_input(input: &IngestPdfInput) -> Result<SafeRelPath, PdfIngestError> {
    if !input.media_type.eq_ignore_ascii_case(PDF_MEDIA_TYPE) {
        return Err(PdfIngestError::MediaTypeMismatch {
            found: input.media_type.to_string(),
        });
    }
    let recomputed = StructuralContentDigest::from_hasher_output(crate::hash::sha256(input.bytes));
    if recomputed != input.input_digest {
        return Err(PdfIngestError::InputDigestMismatch {
            declared: input.input_digest.clone(),
            recomputed,
        });
    }
    validate_native_uid(
        input.source_revision_uid,
        SOURCE_UID_PREFIX,
        |uid, _expected| PdfIngestError::InvalidSourceRevisionUid { uid },
        |uid| PdfIngestError::InvalidSourceRevisionUid { uid },
    )?;
    let path = SafeRelPath::new(input.canonical_path).map_err(|rule| {
        PdfIngestError::InvalidCanonicalPath {
            path: input.canonical_path.to_string(),
            rule,
        }
    })?;
    input.recipe.tool_lock.validate()?;
    input
        .recipe
        .validate_rules()
        .map_err(|detail| PdfIngestError::InvalidRules { detail })?;
    Ok(path)
}

/// Assemble the projection outcome: sort diagnostics, reconcile
/// candidate identities, compute digests, and run the final
/// validation.
fn assemble(
    input: &IngestPdfInput,
    projection: project::Projection,
) -> Result<PdfIngestion, PdfIngestError> {
    let mut diagnostics = projection.diagnostics;
    diagnostics.sort_by(|a, b| {
        (a.page, a.block, &a.kind, &a.detail).cmp(&(b.page, b.block, &b.kind, &b.detail))
    });
    let reconciled = reconcile(&SourceGraph::new(), projection.candidates);
    let by_provisional: BTreeMap<&str, &CandidateNode> = reconciled
        .iter()
        .map(|entry| (entry.candidate.provisional_id.as_str(), &entry.candidate))
        .collect();
    let uid_of: BTreeMap<&str, &str> = reconciled
        .iter()
        .map(|entry| (entry.candidate.provisional_id.as_str(), entry.uid.as_str()))
        .collect();
    let mut nodes = Vec::with_capacity(reconciled.len());
    for entry in &reconciled {
        nodes.push(build_node(input, entry, &by_provisional, &uid_of));
    }
    let mut graph = SourceGraph::new();
    for node in &nodes {
        graph.insert(node.clone())?;
    }
    validate_graph_standalone(input.source_revision_uid, PDF_MEDIA_TYPE, &graph)?;
    let extractor_output_digest =
        StructuralContentDigest::from_hasher_output(crate::hash::sha256(input.extractor_output));
    let output_digest = super::projection::pdf_output_digest(&nodes);
    Ok(PdfIngestion {
        nodes,
        diagnostics,
        extractor_output_digest,
        output_digest,
    })
}

/// Build one committed-shape node from a reconciled candidate:
/// minted uid, resolved parent uid, content digest, and ancestry
/// fingerprint.
fn build_node(
    input: &IngestPdfInput,
    entry: &super::super::ReconciledNode,
    by_provisional: &BTreeMap<&str, &CandidateNode>,
    uid_of: &BTreeMap<&str, &str>,
) -> SourceNode {
    let candidate = &entry.candidate;
    let parent_uid = candidate
        .parent_id
        .as_deref()
        .and_then(|parent| uid_of.get(parent))
        .map(|uid| (*uid).to_string());
    let mut ancestry: Vec<(SourceNodeKind, Option<&str>)> = Vec::new();
    let mut current = candidate.parent_id.as_deref();
    while let Some(parent) = current {
        let Some(node) = by_provisional.get(parent) else {
            break;
        };
        ancestry.push((node.kind, node.label.as_deref()));
        current = node.parent_id.as_deref();
    }
    ancestry.reverse();
    SourceNode {
        uid: entry.uid.clone(),
        source_revision_uid: input.source_revision_uid.to_string(),
        parent_uid,
        kind: candidate.kind,
        ordinal: candidate.ordinal,
        label: candidate.label.clone(),
        canonical_text: candidate.canonical_text.clone(),
        content_sha256: content_digest(candidate.kind, &candidate.canonical_text),
        fingerprint: fingerprint(candidate.kind, candidate.label.as_deref(), &ancestry),
        locator: candidate.locator.clone(),
    }
}
