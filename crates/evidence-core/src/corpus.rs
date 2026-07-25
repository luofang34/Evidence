//! Typed corpus graph over linked TOML files — the v0.2 data model.
//!
//! Certification artifacts load from files indexed by `cert/corpus.toml`
//! into one uid-keyed, typed graph; traceability reports are derived
//! views of that graph, and file layout carries no semantic meaning
//! (SYS-035). Legacy `cert/trace` documents load into the same graph
//! through [`graph_from_trace_files`] with the same identities and
//! edge sets.
//!
//! Module map:
//!
//! - `index` — `corpus.toml` parsing + per-kind file resolution
//! - `graph` — node/edge types and the uid-keyed graph
//! - `records` — corpus-native record file schemas
//! - `review_records` — corpus-native human review decision records
//!   (LLR-114)
//! - `source` — corpus-native frozen source-revision records
//!   (LLR-125)
//! - `source::lock` — the canonical derived `sources.lock`
//!   inventory of effective source heads (LLR-133)
//! - `source::verify` — offline verification of the material
//!   behind each effective source head (LLR-136)
//! - `source_graph` — the parser-independent committed
//!   structural source graph: `snode_` identity, canonical text
//!   and digests, closed locators, forest invariants, and
//!   structural-key reconciliation (LLR-156)
//! - `ingest` — structure-preserving ingestion of verified frozen
//!   Markdown bytes into candidate structural source nodes, with
//!   the ingester recipe identity and typed diagnostics (LLR-161)
//! - `source_patch` — digest-bound curated patch records correcting
//!   parser output without source mutation: `patch_` identity, the
//!   closed operation enum, the reviewed-content digest, and atomic
//!   candidate application on a separately inspectable plane
//!   (LLR-166)
//! - `patch_lifecycle` — deterministic per-patch lifecycle
//!   evaluation over digest-bound review heads, under the
//!   requirement truth table (LLR-173)
//! - `effective_graph` — the approval-gated effective structural
//!   graph of one source revision: only currently approved patches
//!   contribute (LLR-174)
//! - `drift` — deterministic read-only re-ingestion drift
//!   comparison over the recipe, input, parser, patch, review, and
//!   effective planes (LLR-176)
//! - `legacy` — four-file `cert/trace` → graph adapter
//! - `review_content` — versioned canonical projection of the
//!   normative content a review approves (LLR-111)
//! - `digest` — typed lowercase SHA-256 digest domains (LLR-112,
//!   LLR-125, LLR-155)
//! - `lifecycle` — deterministic per-requirement lifecycle
//!   evaluation over digest-bound review heads (LLR-117)
//! - `approval_boundary` — strict validation that implementation and
//!   verification evidence claims only approved requirements under
//!   explicit lifecycle enforcement (LLR-119)
//! - `proposal` — append-only candidate-proposal store; the one
//!   agent-facing write capability (LLR-122)
//!
//! Design record:
//! `docs/superpowers/specs/2026-07-18-corpus-model-v0.2-design.md`.

mod approval_boundary;
mod digest;
mod drift;
mod effective_graph;
mod error;
mod graph;
mod index;
mod ingest;
mod legacy;
mod lifecycle;
mod patch_lifecycle;
#[cfg(test)]
pub(crate) mod patch_testkit;
mod proposal;
mod records;
mod review_content;
mod review_records;
mod source;
mod source_graph;
mod source_patch;

pub use approval_boundary::{
    ApprovalBoundaryError, ApprovalBoundaryViolation, LifecycleEnforcement, ReferringArtifact,
    validate_approval_boundary,
};
pub use digest::{ReviewContentDigest, SourceContentDigest, StructuralContentDigest};
pub use drift::{
    DriftBaseline, DriftCategory, DriftDetail, DriftError, DriftFinding, DriftOutcome, DriftReport,
    ReingestionCandidate, compare_reingestion, render_report_canonical,
};
pub use effective_graph::{EffectiveGraphError, EffectiveSourceGraph, effective_source_graph};
pub use error::CorpusError;
pub(crate) use graph::TraceMetadata;
pub use graph::{
    CorpusGraph, EdgeKind, Node, NodeKind, RequirementLayer, RequirementNode, ReviewDecision,
    ReviewNode, ReviewTarget, ReviewTargetKind, SourceCapture, SourceMaterial, SourceRevisionNode,
    TestNode,
};
pub use index::{CorpusIndex, SUPPORTED_INDEX_SCHEMA};
pub use ingest::pdf::bbox::{BboxDocument, BboxParseError, parse_bbox_layout};
pub use ingest::pdf::lock::{
    PDF_TOOL_NAME, PINNED_ARGV, PdfPlatform, PdfToolLock, PdfToolLockError,
};
pub use ingest::pdf::runner::{PdfExtraction, PdfRunBounds, PdfRunError, run_pdftotext_blocking};
pub use ingest::{
    HTML_MEDIA_TYPE, HtmlIngestDiagnostic, HtmlIngestDiagnosticKind, HtmlIngestError,
    HtmlIngestion, HtmlIngestionRecipe, IngestDiagnostic, IngestDiagnosticKind, IngestError,
    IngestHtmlInput, IngestMarkdownInput, IngestPdfInput, IngesterRecipe, MARKDOWN_MEDIA_TYPE,
    MarkdownIngestion, PDF_MEDIA_TYPE, PdfExcludedBand, PdfIngestDiagnostic,
    PdfIngestDiagnosticKind, PdfIngestError, PdfIngestion, PdfIngestionRecipe, PdfLayoutRules,
    ingest_html, ingest_markdown, ingest_pdf,
};
pub use legacy::graph_from_trace_files;
pub(crate) use legacy::graph_from_trace_parts;
pub use lifecycle::{
    LifecycleError, LifecycleEvaluation, RequirementLifecycle, evaluate_all_lifecycles,
    evaluate_lifecycle,
};
pub use patch_lifecycle::{
    PatchLifecycle, PatchLifecycleError, PatchLifecycleEvaluation, evaluate_all_patch_lifecycles,
    evaluate_patch_lifecycle,
};
pub use proposal::{
    AppendOutcome, PROPOSAL_UID_PREFIX, ProposalAction, ProposalError, ProposalFile,
    ProposalFileDigest, ProposalRecord, ProposalStore, ProposedRequirementContent,
    SUPPORTED_PROPOSAL_SCHEMA,
};
pub use review_content::{
    RequirementReviewContentV1, canonical_bytes_v1, review_content_digest_v1,
};
pub use review_records::error::ReviewError;
pub use source::error::{SourceError, VendoredPathRule};
pub use source::lineage::{
    SourceRevisionProjection, effective_source_heads, validate_source_lineage,
    validate_source_transition,
};
pub use source::lock::{
    ExternalControlId, LockCapture, LockMaterial, SUPPORTED_LOCK_SCHEMA, SourceLock,
    SourceLockEntry, SourceLockError, derive_lock, parse_lock, read_lock_blocking,
    render_lock_canonical, validate_committed_lock, validate_lock_file_blocking,
};
pub use source::verify::{
    DigestMismatchDetail, SourcePayloadError, SourceVerification, SourceVerificationState,
    verify_effective_sources,
};
pub use source_graph::error::SourceGraphError;
pub use source_graph::identity::{
    CandidateNode, ReconciledNode, StructuralKey, mint_node_uid, reconcile, structural_key,
};
pub use source_graph::locator::{LocatorRule, SafeRelPath, SourceLocator};
pub use source_graph::normalization::{
    content_digest, fingerprint, normalize_code, normalize_prose,
};
pub use source_graph::records::{
    SNODE_UID_PREFIX, SUPPORTED_SOURCE_GRAPH_SCHEMA, SourceGraphFile, SourceNodeRecord,
};
pub use source_graph::render::render_source_graph_canonical;
pub use source_graph::{SourceGraph, SourceNode, SourceNodeKind};
pub use source_patch::apply::{PatchApplication, PatchBindings, apply_patch};
pub use source_patch::digest::{
    reviewed_content_bytes, reviewed_content_digest, source_graph_digest,
};
pub use source_patch::error::SourcePatchError;
pub use source_patch::records::{
    PATCH_UID_PREFIX, SUPPORTED_SOURCE_PATCH_SCHEMA, SourcePatchFile, SourcePatchRecord,
    parse_source_patch,
};
pub use source_patch::{ChildDisposition, InsertedNodeSpec, PatchOperation};

#[cfg(test)]
mod tests;
