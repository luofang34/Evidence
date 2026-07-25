//! Structure-preserving offline ingestion of verified frozen HTML
//! bytes into candidate structural source nodes (LLR-164).
//!
//! [`ingest_html`] is a pure core API over verified bytes plus
//! explicit metadata: it performs no fetch, no DNS lookup, no
//! stylesheet load, no script execution, no browser rendering, no
//! filesystem access, no environment reads, no workspace mutation,
//! and no baseline replacement. The caller selects the effective
//! source revision and completes material verification first; the
//! adapter then requires the frozen bytes, the HTML media type
//! assertion, the source-revision uid, the canonical URL and the
//! final captured URL when distinct, the verified input digest,
//! and an explicit [`HtmlIngestionRecipe`] declaring the parser,
//! encoding, inclusion root, exclusion selectors, classification
//! rules, and compatibility modes. URLs are opaque audit identity:
//! their lexical shape is checked, and they are never fetched,
//! resolved, or normalized. It uses the same canonical node
//! builder and identity planes as Markdown ingestion, so
//! format-specific parsing cannot create a second graph model.
//!
//! # Input contract
//!
//! The contract validates before parsing, in a documented
//! fail-fast order so the reported error is deterministic:
//!
//! 1. **Media type** — must be `text/html`
//!    ([`HTML_MEDIA_TYPE`], ASCII case-insensitive).
//! 2. **Input digest** — the bytes must re-digest to the declared
//!    digest; a mismatch means the bytes are not the verified
//!    material.
//! 3. **Revision uid** — must satisfy the corpus-native `src_`
//!    UUIDv4 contract.
//! 4. **URLs** — the canonical URL, and the final URL when
//!    present, must carry an absolute `<scheme>://` shape.
//! 5. **Size** — the input must fit the byte bound.
//! 6. **Encoding** — the recipe must declare an encoding, and only
//!    UTF-8 is accepted; missing and unsupported encodings fail
//!    closed rather than relying on platform defaults, content
//!    sniffing, or lossy replacement.
//! 7. **UTF-8** — the bytes must decode strictly; the error
//!    carries the byte offset of the first invalid sequence. One
//!    leading byte-order mark is stripped — the single documented
//!    deterministic BOM rule.
//! 8. **External resolution** — the raw bytes must not declare an
//!    external entity (`<!ENTITY`): the parser never expands
//!    entities or resolves external identifiers by construction,
//!    and the scan rejects the attempt fail-closed.
//!
//! Every failure is a flat typed [`HtmlIngestError`] variant
//! carrying the conflicting values (`error` module). Recipe
//! selector syntax errors fail closed before parsing; an
//! inclusion root matching no element fails closed after parsing.
//!
//! # Output
//!
//! [`HtmlIngestion`] carries the candidate nodes (identities
//! minted through the structural identity service, in document
//! order), the sorted typed [`HtmlIngestDiagnostic`] list, and the
//! `output_digest` over the canonical node projection
//! (`evidence/html-ingest-output/v1` in the shared `projection`
//! module). Recipe, input, and output digests are three
//! independent identity planes: each covers only its own
//! projection, so a targeted mutation of one plane — a
//! recipe-selector change included — moves exactly one digest.
//!
//! Structural-loss diagnostics are typed — excluded by recipe
//! selector, closed-rule drop, unsupported element, duplicate
//! anchor, dangling internal link — each carrying a DOM-path
//! locator and a typed reason, sorted deterministically by DOM
//! path, kind, and detail. Silent dropping is forbidden; every
//! configured exclusion and every element the projection cannot
//! represent produces a diagnostic, while unconfigured
//! potentially normative regions are retained and projected.
//!
//! Module map:
//!
//! - `recipe` — the [`HtmlIngestionRecipe`] identity with
//!   canonical byte encoding and digest (LLR-163)
//! - `error` — the [`HtmlIngestError`] taxonomy and the typed
//!   structural-loss diagnostics (LLR-164)
//! - `adapter` — the adapter state, bounds, and pre-pass (LLR-165)
//! - `walk` — the document-order DOM walk and node handlers
//!   (LLR-165)
//! - `emit` — candidate emission, text collection, and the table
//!   grid projection (LLR-165)

use std::collections::BTreeMap;

use super::projection::html_output_digest;
use crate::corpus::records::validate_native_uid;
use crate::corpus::source::SOURCE_UID_PREFIX;
use crate::corpus::source_graph::validate::validate_graph_standalone;
use crate::corpus::{
    CandidateNode, SourceGraph, SourceNode, SourceNodeKind, content_digest, fingerprint, reconcile,
};

mod adapter;
mod emit;
mod error;
mod recipe;
mod walk;

use adapter::Adapter;

pub use error::{HtmlIngestDiagnostic, HtmlIngestDiagnosticKind, HtmlIngestError};
pub use recipe::HtmlIngestionRecipe;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "html/tests.rs"]
mod tests;

/// The RFC 6838 media type the HTML ingestion contract requires
/// (LLR-164). Compared ASCII case-insensitively.
pub const HTML_MEDIA_TYPE: &str = "text/html";

/// The input byte-size bound (LLR-165).
pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;

/// The verified input of one HTML ingestion (LLR-164): frozen
/// bytes plus the explicit metadata the contract binds.
#[derive(Debug, Clone)]
pub struct IngestHtmlInput<'a> {
    /// The frozen source bytes, already material-verified by the
    /// caller against `input_digest`.
    pub bytes: &'a [u8],
    /// The media type assertion; must be [`HTML_MEDIA_TYPE`].
    pub media_type: &'a str,
    /// The `src_<UUIDv4>` revision the bytes were frozen as.
    pub source_revision_uid: &'a str,
    /// Canonical URL of the document; absolute shape, opaque —
    /// never fetched or resolved.
    pub canonical_url: &'a str,
    /// Optional post-redirect captured URL; absolute shape,
    /// opaque.
    pub final_url: Option<String>,
    /// The verified digest of `bytes`; re-computed and compared
    /// before parsing.
    pub input_digest: crate::corpus::StructuralContentDigest,
    /// The explicit HTML ingestion recipe identity.
    pub recipe: HtmlIngestionRecipe,
}

/// The result of one HTML ingestion (LLR-164).
#[derive(Debug, Clone)]
pub struct HtmlIngestion {
    /// Candidate nodes with minted identities, in document order.
    pub nodes: Vec<SourceNode>,
    /// Sorted typed structural-loss diagnostics; empty when the
    /// retained document projects losslessly.
    pub diagnostics: Vec<HtmlIngestDiagnostic>,
    /// The output identity plane: SHA-256 over the canonical node
    /// projection (`projection` module).
    pub output_digest: crate::corpus::StructuralContentDigest,
}

impl HtmlIngestion {
    /// The canonical uid-free projection of the candidate nodes;
    /// `output_digest` is SHA-256 over these bytes.
    pub fn canonical_projection(&self) -> Vec<u8> {
        super::projection::render_html_projection(&self.nodes)
    }
}

/// Ingest verified frozen HTML bytes into candidate structural
/// source nodes (LLR-164).
///
/// The input contract validates first (see the module docs);
/// parsing, the bounded walk, and assembly then follow the
/// `adapter`/`walk`/`emit` module docs.
///
/// # Errors
///
/// Fails closed with [`HtmlIngestError`] on any contract
/// violation, any bound violation, or when the assembled candidate
/// set violates the source-graph invariants.
pub fn ingest_html(input: &IngestHtmlInput) -> Result<HtmlIngestion, HtmlIngestError> {
    let text = validate_html_input(input)?;
    let mut adapter = Adapter::new(input)?;
    let document = scraper::Html::parse_document(text);
    let root = adapter.find_root(&document)?;
    let root_path = Adapter::dom_path_of(root);
    adapter.prepare(root)?;
    adapter.walk_children(root, &root_path)?;
    adapter.resolve_links();
    assemble(input, adapter)
}

/// Validate the input contract in the module docs' fail-fast
/// order, returning the decoded, BOM-stripped text.
///
/// # Errors
///
/// The first contract violation wins, so error precedence is
/// deterministic.
fn validate_html_input<'i>(input: &IngestHtmlInput<'i>) -> Result<&'i str, HtmlIngestError> {
    if !input.media_type.eq_ignore_ascii_case(HTML_MEDIA_TYPE) {
        return Err(HtmlIngestError::MediaTypeMismatch {
            found: input.media_type.to_string(),
        });
    }
    let recomputed = crate::corpus::StructuralContentDigest::from_hasher_output(
        crate::hash::sha256(input.bytes),
    );
    if recomputed != input.input_digest {
        return Err(HtmlIngestError::InputDigestMismatch {
            declared: input.input_digest.clone(),
            recomputed,
        });
    }
    validate_native_uid(
        input.source_revision_uid,
        SOURCE_UID_PREFIX,
        |uid, _expected| HtmlIngestError::InvalidSourceRevisionUid { uid },
        |uid| HtmlIngestError::InvalidSourceRevisionUid { uid },
    )?;
    check_url("canonical_url", input.canonical_url)?;
    if let Some(final_url) = &input.final_url {
        check_url("final_url", final_url)?;
    }
    if input.bytes.len() > MAX_INPUT_BYTES {
        return Err(HtmlIngestError::InputTooLarge {
            size: input.bytes.len(),
            limit: MAX_INPUT_BYTES,
        });
    }
    if input.recipe.encoding.trim().is_empty() {
        return Err(HtmlIngestError::MissingEncoding);
    }
    let encoding = input.recipe.encoding.trim();
    if !encoding.eq_ignore_ascii_case("utf-8") && !encoding.eq_ignore_ascii_case("utf8") {
        return Err(HtmlIngestError::UnsupportedEncoding {
            encoding: input.recipe.encoding.clone(),
        });
    }
    let text = std::str::from_utf8(input.bytes).map_err(|err| HtmlIngestError::NonUtf8 {
        offset: err.valid_up_to(),
    })?;
    // The documented BOM rule: one leading U+FEFF strips.
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    if text
        .as_bytes()
        .windows(b"<!entity".len())
        .any(|window| window.eq_ignore_ascii_case(b"<!entity"))
    {
        return Err(HtmlIngestError::ExternalResolution {
            detail: "an <!ENTITY declaration would require external resolution".to_string(),
        });
    }
    Ok(text)
}

/// A URL is opaque audit identity with an absolute
/// `<scheme>://<rest>` shape — the locator module's lexical rule,
/// applied to the input contract.
fn check_url(field: &'static str, value: &str) -> Result<(), HtmlIngestError> {
    let valid = match value.split_once("://") {
        Some((scheme, rest)) => {
            !rest.is_empty()
                && scheme
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_alphabetic())
                && scheme
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
                && !value.chars().any(char::is_whitespace)
        }
        None => false,
    };
    if !valid {
        return Err(HtmlIngestError::InvalidUrl {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

/// Assemble the walk outcome: sort diagnostics, reconcile
/// candidate identities, compute digests, and run the final
/// validation.
fn assemble(
    input: &IngestHtmlInput,
    mut adapter: Adapter,
) -> Result<HtmlIngestion, HtmlIngestError> {
    adapter
        .diagnostics
        .sort_by(|a, b| (&a.dom_path, &a.kind, &a.detail).cmp(&(&b.dom_path, &b.kind, &b.detail)));
    // Candidates close in finish order (children before parents);
    // restoring open order makes the node list document order,
    // which the projection and the module docs promise.
    let mut candidates = adapter.candidates;
    candidates.sort_by_key(|candidate| provisional_seq(&candidate.provisional_id));
    let reconciled = reconcile(&SourceGraph::new(), candidates);
    let by_provisional: BTreeMap<&str, &CandidateNode> = reconciled
        .iter()
        .map(|r| (r.candidate.provisional_id.as_str(), &r.candidate))
        .collect();
    let uid_of: BTreeMap<&str, &str> = reconciled
        .iter()
        .map(|r| (r.candidate.provisional_id.as_str(), r.uid.as_str()))
        .collect();
    let mut nodes = Vec::with_capacity(reconciled.len());
    for entry in &reconciled {
        nodes.push(build_node(input, entry, &by_provisional, &uid_of));
    }
    let mut graph = SourceGraph::new();
    for node in &nodes {
        graph.insert(node.clone())?;
    }
    validate_graph_standalone(input.source_revision_uid, HTML_MEDIA_TYPE, &graph)?;
    let output_digest = html_output_digest(&nodes);
    Ok(HtmlIngestion {
        nodes,
        diagnostics: adapter.diagnostics,
        output_digest,
    })
}

/// The numeric sequence of a `cand-N` provisional id — the frame
/// open order, which is the document order. Malformed ids (which
/// the adapter never produces) sort last.
fn provisional_seq(provisional_id: &str) -> u64 {
    provisional_id
        .strip_prefix("cand-")
        .and_then(|seq| seq.parse().ok())
        .unwrap_or(u64::MAX)
}

/// Build one committed-shape node from a reconciled candidate:
/// minted uid, resolved parent uid, content digest, and ancestry
/// fingerprint.
fn build_node(
    input: &IngestHtmlInput,
    entry: &crate::corpus::ReconciledNode,
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
