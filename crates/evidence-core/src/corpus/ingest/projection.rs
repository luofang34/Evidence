//! The canonical node projection of one ingestion result and its
//! output digest (LLR-161, LLR-164).
//!
//! [`render_candidate_projection`] renders the candidate node set
//! into one deterministic byte form. The projection is uid-free —
//! minted node identities are random, so including them would make
//! repeated ingestion of identical bytes digest-differently. Instead
//! the parent link renders as the parent's index in the same
//! document-order node list (or `root`), which is deterministic for
//! fixed input bytes and recipe. Every other field renders exactly:
//! kind, ordinal, optional label, the locator, canonical text, and
//! the node's content digest.
//!
//! The form is line-based and TOML-shaped with the minimal escaping
//! of the canonical source-graph rendering. Two domain-tagged
//! headers keep the format families disjoint:
//!
//! - `evidence/markdown-ingest-output/v1`
//!   ([`render_candidate_projection`]) renders Markdown locators:
//!   path, optional git blob, optional anchor, heading path, byte
//!   range.
//! - `evidence/html-ingest-output/v1`
//!   ([`render_html_projection`], LLR-164) renders HTML locators:
//!   canonical URL, optional final URL, optional fragment, heading
//!   path, DOM path.
//!
//! ```text
//! evidence/markdown-ingest-output/v1
//! nodes = 2
//!
//! [[node]]
//! parent = root
//! kind = "section"
//! ordinal = 0
//! label = "1 Intro"
//! anchor = "1-intro"
//! heading_path = ["1 Intro"]
//! byte_range = [0, 10]
//! text = "1 Intro"
//! digest = "<content_sha256 hex>"
//! ```
//!
//! Optional fields render only when present. The output digest of an
//! ingestion is SHA-256 over these bytes: the output identity plane,
//! covering the canonical node projection and never parser-internal
//! objects. The recipe and input digests do not enter the
//! projection, so the three identity planes change independently.

use super::super::digest::StructuralContentDigest;
use super::super::{SourceLocator, SourceNode};

/// Domain/version tag opening the Markdown projection.
const PROJECTION_HEADER: &str = "evidence/markdown-ingest-output/v1";

/// Domain/version tag opening the HTML projection (LLR-164).
const HTML_PROJECTION_HEADER: &str = "evidence/html-ingest-output/v1";

/// Render `nodes` (in document order) into the canonical Markdown
/// projection pinned by the module docs. Pure and host-independent.
pub fn render_candidate_projection(nodes: &[SourceNode]) -> Vec<u8> {
    render_projection(PROJECTION_HEADER, nodes)
}

/// Render `nodes` (in document order) into the canonical HTML
/// projection (LLR-164). Pure and host-independent.
pub fn render_html_projection(nodes: &[SourceNode]) -> Vec<u8> {
    render_projection(HTML_PROJECTION_HEADER, nodes)
}

/// The output identity plane: SHA-256 over the canonical Markdown
/// projection of `nodes`, as the validated structural digest domain.
pub fn output_digest(nodes: &[SourceNode]) -> StructuralContentDigest {
    StructuralContentDigest::from_hasher_output(crate::hash::sha256(&render_candidate_projection(
        nodes,
    )))
}

/// The HTML output identity plane (LLR-164): SHA-256 over the
/// canonical HTML projection of `nodes`.
pub fn html_output_digest(nodes: &[SourceNode]) -> StructuralContentDigest {
    StructuralContentDigest::from_hasher_output(crate::hash::sha256(&render_html_projection(nodes)))
}

/// Render `nodes` under `header`. Every locator renders its own
/// variant's fields; a locator of a different format than the
/// header's family — which the ingesters never produce — renders
/// only its `format` line, keeping the function total over
/// fabricated node sets.
fn render_projection(header: &str, nodes: &[SourceNode]) -> Vec<u8> {
    let mut out = String::new();
    out.push_str(header);
    out.push('\n');
    out.push_str(&format!("nodes = {}\n", nodes.len()));
    for node in nodes {
        out.push_str("\n[[node]]\n");
        match &node.parent_uid {
            Some(parent) => {
                let parent_index = nodes.iter().position(|n| &n.uid == parent);
                match parent_index {
                    Some(found) => out.push_str(&format!("parent = {found}\n")),
                    None => out.push_str("parent = dangling\n"),
                }
            }
            None => out.push_str("parent = root\n"),
        }
        push_field(&mut out, "kind", node.kind.as_str());
        out.push_str(&format!("ordinal = {}\n", node.ordinal));
        if let Some(label) = &node.label {
            push_field(&mut out, "label", label);
        }
        push_locator(&mut out, &node.locator);
        push_field(&mut out, "text", &node.canonical_text);
        push_field(&mut out, "digest", node.content_sha256.as_str());
    }
    out.into_bytes()
}

/// Render one locator. Markdown and HTML locators render every
/// field in schema order; other formats render only the
/// discriminator.
fn push_locator(out: &mut String, locator: &SourceLocator) {
    match locator {
        SourceLocator::Markdown {
            path,
            git_blob,
            anchor,
            heading_path,
            byte_range,
        } => {
            push_field(out, "path", path.as_str());
            if let Some(blob) = git_blob {
                push_field(out, "git_blob", blob);
            }
            if let Some(anchor) = anchor {
                push_field(out, "anchor", anchor);
            }
            push_heading_path(out, heading_path);
            out.push_str(&format!(
                "byte_range = [{}, {}]\n",
                byte_range.0, byte_range.1
            ));
        }
        SourceLocator::Html {
            canonical_url,
            final_url,
            fragment,
            heading_path,
            dom_path,
        } => {
            push_field(out, "canonical_url", canonical_url);
            if let Some(final_url) = final_url {
                push_field(out, "final_url", final_url);
            }
            if let Some(fragment) = fragment {
                push_field(out, "fragment", fragment);
            }
            push_heading_path(out, heading_path);
            out.push_str("dom_path = [");
            for (index, component) in dom_path.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("{component}"));
            }
            out.push_str("]\n");
        }
        other => push_field(out, "format", other.format_str()),
    }
}

/// One `heading_path = [...]` line in canonical escaping.
fn push_heading_path(out: &mut String, heading_path: &[String]) {
    out.push_str("heading_path = [");
    for (index, component) in heading_path.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        push_basic_string(out, component);
    }
    out.push_str("]\n");
}

/// One `key = "<value>"` line in canonical escaping.
fn push_field(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(" = ");
    push_basic_string(out, value);
    out.push('\n');
}

/// Append `value` as a TOML basic string with deterministic minimal
/// escaping — the same rules the canonical source-graph rendering
/// pins.
fn push_basic_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{0C}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7F => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
