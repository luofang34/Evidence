//! Canonical byte rendering of the committed source graphs
//! (LLR-156).
//!
//! [`render_source_graph_canonical`] renders every committed
//! source node into one deterministic TOML-shaped byte form.
//! Nodes render in global uid order — independent of input file
//! layout, parser event order, map iteration, and source record
//! order — and every field renders in a fixed order with minimal
//! TOML basic-string escaping (the `sources.lock` precedent), so
//! equivalent linked layouts render byte-identically. Optional
//! fields render only when present. The form is:
//!
//! ```text
//! schema_version = 1
//!
//! [[nodes]]
//! uid = "snode_..."
//! source_revision_uid = "src_..."
//! parent_uid = "snode_..."        (omitted when None)
//! kind = "section"
//! ordinal = 0
//! label = "..."                    (omitted when None)
//! canonical_text = "..."
//! content_sha256 = "..."
//! fingerprint = "..."
//!
//! [nodes.locator]
//! format = "markdown"
//! ... per-variant fields in schema order, optionals omitted ...
//! ```
//!
//! Floating-point bounding-box coordinates render through Rust's
//! shortest round-trip `Display`, which is deterministic for a
//! given value. The rendering is a diagnostic and fixture byte
//! lock; it is not a loadable schema (records load through the
//! strict `SourceGraphFile` schema, and loading never depends on
//! this form).

use super::super::graph::CorpusGraph;
use super::SourceNode;
use super::locator::SourceLocator;

/// Render every committed source node in `graph` in the canonical
/// byte form pinned by the module docs. Pure and host-independent.
pub fn render_source_graph_canonical(graph: &CorpusGraph) -> Vec<u8> {
    let mut nodes: Vec<&SourceNode> = graph
        .source_graphs()
        .values()
        .flat_map(|source_graph| source_graph.nodes())
        .collect();
    nodes.sort_by(|a, b| a.uid.cmp(&b.uid));
    let mut out = String::from("schema_version = 1\n");
    for node in nodes {
        out.push_str("\n[[nodes]]\n");
        push_field(&mut out, "uid", &node.uid);
        push_field(&mut out, "source_revision_uid", &node.source_revision_uid);
        if let Some(parent) = &node.parent_uid {
            push_field(&mut out, "parent_uid", parent);
        }
        push_field(&mut out, "kind", node.kind.as_str());
        out.push_str(&format!("ordinal = {}\n", node.ordinal));
        if let Some(label) = &node.label {
            push_field(&mut out, "label", label);
        }
        push_field(&mut out, "canonical_text", &node.canonical_text);
        push_field(&mut out, "content_sha256", node.content_sha256.as_str());
        push_field(&mut out, "fingerprint", node.fingerprint.as_str());
        out.push_str("\n[nodes.locator]\n");
        push_locator(&mut out, &node.locator);
    }
    out.into_bytes()
}

/// Render one locator's fields in schema order.
fn push_locator(out: &mut String, locator: &SourceLocator) {
    push_field(out, "format", locator.format_str());
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
            push_string_array(out, "heading_path", heading_path);
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
            push_string_array(out, "heading_path", heading_path);
            push_u32_array(out, "dom_path", dom_path);
        }
        SourceLocator::Pdf {
            physical_page,
            printed_label,
            bbox,
        } => {
            out.push_str(&format!("physical_page = {physical_page}\n"));
            if let Some(printed_label) = printed_label {
                push_field(out, "printed_label", printed_label);
            }
            out.push_str(&format!(
                "bbox = [{}, {}, {}, {}]\n",
                bbox[0], bbox[1], bbox[2], bbox[3]
            ));
        }
    }
}

/// One `key = "<value>"` line in canonical escaping.
fn push_field(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(" = ");
    push_basic_string(out, value);
    out.push('\n');
}

/// One `key = ["a", "b"]` line.
fn push_string_array(out: &mut String, key: &str, values: &[String]) {
    out.push_str(key);
    out.push_str(" = [");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        push_basic_string(out, value);
    }
    out.push_str("]\n");
}

/// One `key = [0, 2]` line.
fn push_u32_array(out: &mut String, key: &str, values: &[u32]) {
    out.push_str(key);
    out.push_str(" = [");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&value.to_string());
    }
    out.push_str("]\n");
}

/// Append `value` as a TOML basic string with deterministic
/// minimal escaping — the same rules the canonical `sources.lock`
/// rendering pins.
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
