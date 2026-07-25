//! The pulldown-cmark adapter: parser events become candidate
//! structural source nodes (LLR-162).
//!
//! The adapter parses CommonMark with exactly `ENABLE_TABLES` and
//! `ENABLE_FOOTNOTES` and walks the parser's offset iterator,
//! mapping events into the canonical candidate builder. It never
//! constructs graph records independently: candidates reconcile
//! through the structural identity service, and an empty committed
//! graph mints every identity fresh. The adapter performs no I/O —
//! no fetch, no filesystem, no environment.
//!
//! # Structure mapping
//!
//! | Construct | Kind | Parent | Canonical text | Anchor |
//! |---|---|---|---|---|
//! | Heading H1–H6 | Section | nearest open section of strictly lower level, else root | heading text (explicit `{#id}` stripped) | explicit `{#id}` or generated slug |
//! | Paragraph | Paragraph | enclosing section, else root | inline content, prose contract | none |
//! | List item (ordered, unordered, nested) | ListItem | enclosing section | item's own inline content | none |
//! | GFM table | Table | enclosing section | empty | none |
//! | Table head / row | TableRow | the table | empty | none |
//! | Table cell | TableCell | the row | cell content, prose contract | none |
//! | Fenced / indented code | CodeBlock | enclosing section | literal, code contract | none |
//! | Block quote | Note | enclosing section | quoted prose, alert marker stripped | none |
//! | Footnote definition | Note, label `footnote:<id>` | enclosing section | definition body | none |
//!
//! Mapping rules the table abbreviates:
//!
//! - **Sections** nest by heading level: a heading closes every open
//!   section of equal or higher level and opens under the nearest
//!   lower-level one; a document opening with a deep heading roots
//!   it. A section's label is its normalized heading text (an
//!   explicit-numbering-shaped leading token feeds the structural
//!   key's first tier); its heading path is the ancestor section
//!   labels plus its own label. Content nodes carry the enclosing
//!   section trail.
//! - **Paragraphs** inside a list item, block quote, footnote
//!   definition, or table cell are absorbed: their text folds into
//!   the enclosing node's text, paragraphs joined with newlines.
//! - **Lists** project flat: every item attaches to the enclosing
//!   section in document order, so a nested list's items follow
//!   their parent item as siblings — the closed parent/child kind
//!   table makes list items leaves, and this is the canonical
//!   nested-list projection. The ordered-list start number is not
//!   projected; ordinals capture order.
//! - **Tables**: the header row maps to the first `TableRow`; column
//!   alignment is not projected.
//! - **Code**: fenced and indented blocks are verbatim under the
//!   code normalization contract (NFC, line endings to LF,
//!   significant whitespace preserved); the fence info string is
//!   metadata and is not projected.
//! - **Block quotes**: a leading GFM alert marker `[!NOTE]`,
//!   `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, or `[!CAUTION]`
//!   (case-insensitive) is stripped; the alert kind is not
//!   separately projected — the Note kind denotes advisory content.
//!   Blocks nested inside a quote (lists, code) project as their own
//!   sibling nodes.
//! - **Footnotes**: the first definition of an id becomes a Note
//!   labeled `footnote:<id>`; a repeated definition produces a
//!   `footnote-definition` lossy diagnostic and no node. Reference
//!   markers are not projected.
//! - **Definition kinds** (`DefinitionTerm`, `DefinitionBody`) and
//!   `FigureCaption` are never emitted: the enabled syntax has no
//!   explicit representation for them and nothing is inferred.
//!
//! # Anchors
//!
//! A section's anchor is its explicit `{#id}` suffix — non-empty,
//! ASCII alphanumeric first, then alphanumerics, `-`, or `_`; the
//! last trailing suffix wins — or a generated slug (`anchors`
//! module: NFC, lowercase, non-alphanumeric runs to one hyphen,
//! trimmed, empty becomes `section`, `-2`/`-3` deduplication against
//! every anchor claimed so far in document order). A trailing
//! `{#...}` that fails the id rule is a malformed explicit id
//! diagnostic, and the heading is slugged from its full text. An
//! explicit id claiming an already-claimed anchor value is a
//! duplicate anchor diagnostic on the second and later claims; the
//! value is kept as the document author's claim, and the structural
//! key pools equal explicit keys deterministically.
//!
//! # Diagnostics and locators
//!
//! Raw HTML (block or inline) produces an unsupported-raw-HTML
//! diagnostic per event; images and thematic breaks produce
//! lossy-construct diagnostics (an image's alt text remains in the
//! enclosing text; its target does not project). Inline link targets
//! and emphasis markers are inline formatting and are not diagnosed.
//! Diagnostics sort by byte range, kind, and detail. Every node
//! records a Markdown locator: the canonical path and optional git
//! blob from the input, the anchor (sections only), the heading
//! path, and a byte range from the parser's offset iterator through
//! a checked constructor that rejects unaligned or out-of-bounds
//! ranges. Byte ranges are diagnostic, never identity.
//!
//! # Assembly
//!
//! After the parse, candidates reconcile through the identity
//! service, content digests and fingerprints come from the canonical
//! normalization module, and the assembled set is inserted into a
//! fresh source graph (enforcing uid and human-identity uniqueness —
//! a duplicated section label fails closed) and validated against
//! the standalone forest invariants before the result returns.

use std::collections::BTreeMap;

use pulldown_cmark::{Options, Parser};

use super::projection::output_digest;
use super::{IngestError, IngestMarkdownInput, MarkdownIngestion, validate_input};
use crate::corpus::source_graph::validate::validate_graph_standalone;
use crate::corpus::{
    CandidateNode, SourceGraph, SourceNode, SourceNodeKind, content_digest, fingerprint, reconcile,
};

mod adapter;
pub(super) mod anchors;
mod close;

use adapter::Adapter;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "markdown/tests.rs"]
mod tests;

/// Ingest verified frozen Markdown bytes into candidate structural
/// source nodes (LLR-162).
///
/// The input contract validates first (see the `ingest` module
/// docs); parsing and assembly then follow the module docs' mapping.
///
/// # Errors
///
/// Fails closed with [`IngestError`] on any contract violation or
/// when the assembled candidate set violates the source-graph
/// invariants.
pub fn ingest_markdown(input: &IngestMarkdownInput) -> Result<MarkdownIngestion, IngestError> {
    let (text, path) = validate_input(input)?;
    let options = Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES;
    let mut adapter = Adapter::new(input, path);
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        adapter.event(text, event, range)?;
    }
    assemble(input, adapter)
}

/// Assemble the parse outcome: sort diagnostics, reconcile candidate
/// identities, compute digests, and run the final validation.
fn assemble(
    input: &IngestMarkdownInput,
    mut adapter: Adapter,
) -> Result<MarkdownIngestion, IngestError> {
    adapter.diagnostics.sort_by(|a, b| {
        (a.byte_range.0, a.byte_range.1, &a.kind, &a.detail).cmp(&(
            b.byte_range.0,
            b.byte_range.1,
            &b.kind,
            &b.detail,
        ))
    });
    // Candidates close in finish order (children before parents);
    // restoring open order makes the node list document order, which
    // the projection and the module docs promise.
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
    validate_graph_standalone(
        input.source_revision_uid,
        super::MARKDOWN_MEDIA_TYPE,
        &graph,
    )?;
    let output_digest = output_digest(&nodes);
    Ok(MarkdownIngestion {
        nodes,
        diagnostics: adapter.diagnostics,
        output_digest,
    })
}

/// The numeric sequence of a `cand-N` provisional id — the frame
/// open order, which is the document order. Malformed ids (which the
/// adapter never produces) sort last.
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
    input: &IngestMarkdownInput,
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
