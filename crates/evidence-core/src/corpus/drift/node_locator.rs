//! Locator comparison for the node plane (LLR-177): the semantic
//! fields (variant, path or canonical URL, anchor or fragment,
//! heading path) report one finding per field, and the
//! diagnostic-only positions (byte range, DOM path, page, bounding
//! box, printed label, final URL, git blob) report one finding per
//! node under the distinct
//! [`DriftCategory::DiagnosticLocatorMoved`] category — diagnostic
//! movement never counts as semantic drift.

use super::super::source_graph::SourceNode;
use super::super::source_graph::locator::SourceLocator;
use super::findings::{DriftCategory, DriftDetail};

/// Compare the typed locators, splitting semantic fields from
/// diagnostic-only positions. A variant change is one semantic
/// `format` finding; positions across formats are incomparable and
/// produce no diagnostic finding.
pub(super) fn compare_locators(
    committed: &SourceNode,
    candidate: &SourceNode,
    push: &mut impl FnMut(DriftCategory, DriftDetail),
) {
    if committed.locator.format_str() != candidate.locator.format_str() {
        push(
            DriftCategory::NodeSemanticLocatorChanged,
            change(
                "format",
                committed.locator.format_str(),
                candidate.locator.format_str(),
            ),
        );
        return;
    }
    let (semantic, diagnostic) = locator_changes(&committed.locator, &candidate.locator);
    for (field, before, after) in semantic {
        push(
            DriftCategory::NodeSemanticLocatorChanged,
            change(field, &before, &after),
        );
    }
    if !diagnostic.is_empty() {
        let render = |fields: &[(&'static str, String)]| {
            fields
                .iter()
                .map(|(field, value)| format!("{field} = {value}"))
                .collect::<Vec<_>>()
                .join("; ")
        };
        let committed_fields: Vec<(&'static str, String)> = diagnostic
            .iter()
            .map(|(field, before, _)| (*field, before.clone()))
            .collect();
        let candidate_fields: Vec<(&'static str, String)> = diagnostic
            .iter()
            .map(|(field, _, after)| (*field, after.clone()))
            .collect();
        push(
            DriftCategory::DiagnosticLocatorMoved,
            change(
                "diagnostic_fields",
                &render(&committed_fields),
                &render(&candidate_fields),
            ),
        );
    }
}

/// One changed locator field: `(field, committed, candidate)` in
/// schema order.
type FieldChange3 = (&'static str, String, String);

/// The changed semantic and diagnostic locator fields between two
/// same-variant locators, in schema order.
fn locator_changes(
    committed: &SourceLocator,
    candidate: &SourceLocator,
) -> (Vec<FieldChange3>, Vec<FieldChange3>) {
    let mut semantic = Vec::new();
    let mut diagnostic = Vec::new();
    match (committed, candidate) {
        (
            SourceLocator::Markdown {
                path,
                git_blob,
                anchor,
                heading_path,
                byte_range,
            },
            SourceLocator::Markdown {
                path: c_path,
                git_blob: c_blob,
                anchor: c_anchor,
                heading_path: c_headings,
                byte_range: c_range,
            },
        ) => {
            if path != c_path {
                semantic.push((
                    "path",
                    path.as_str().to_string(),
                    c_path.as_str().to_string(),
                ));
            }
            if anchor != c_anchor {
                semantic.push((
                    "anchor",
                    render_option(anchor.as_deref()),
                    render_option(c_anchor.as_deref()),
                ));
            }
            if heading_path != c_headings {
                semantic.push((
                    "heading_path",
                    render_list(heading_path),
                    render_list(c_headings),
                ));
            }
            if git_blob != c_blob {
                diagnostic.push((
                    "git_blob",
                    render_option(git_blob.as_deref()),
                    render_option(c_blob.as_deref()),
                ));
            }
            if byte_range != c_range {
                diagnostic.push((
                    "byte_range",
                    render_range(*byte_range),
                    render_range(*c_range),
                ));
            }
        }
        (
            SourceLocator::Html {
                canonical_url,
                final_url,
                fragment,
                heading_path,
                dom_path,
            },
            SourceLocator::Html {
                canonical_url: c_url,
                final_url: c_final,
                fragment: c_fragment,
                heading_path: c_headings,
                dom_path: c_dom,
            },
        ) => {
            if canonical_url != c_url {
                semantic.push(("canonical_url", canonical_url.clone(), c_url.clone()));
            }
            if fragment != c_fragment {
                semantic.push((
                    "fragment",
                    render_option(fragment.as_deref()),
                    render_option(c_fragment.as_deref()),
                ));
            }
            if heading_path != c_headings {
                semantic.push((
                    "heading_path",
                    render_list(heading_path),
                    render_list(c_headings),
                ));
            }
            if final_url != c_final {
                diagnostic.push((
                    "final_url",
                    render_option(final_url.as_deref()),
                    render_option(c_final.as_deref()),
                ));
            }
            if dom_path != c_dom {
                diagnostic.push((
                    "dom_path",
                    render_u32_list(dom_path),
                    render_u32_list(c_dom),
                ));
            }
        }
        (
            SourceLocator::Pdf {
                physical_page,
                printed_label,
                bbox,
            },
            SourceLocator::Pdf {
                physical_page: c_page,
                printed_label: c_label,
                bbox: c_bbox,
            },
        ) => {
            if physical_page != c_page {
                diagnostic.push((
                    "physical_page",
                    physical_page.to_string(),
                    c_page.to_string(),
                ));
            }
            if printed_label != c_label {
                diagnostic.push((
                    "printed_label",
                    render_option(printed_label.as_deref()),
                    render_option(c_label.as_deref()),
                ));
            }
            if bbox != c_bbox {
                diagnostic.push(("bbox", render_bbox(*bbox), render_bbox(*c_bbox)));
            }
        }
        // Different variants: the caller already reported the
        // semantic `format` change; cross-format positions are
        // incomparable.
        _ => {}
    }
    (semantic, diagnostic)
}

/// The deterministic structural path of one node: label or
/// `kind[ordinal]` segments, root first, joined by `/`. A
/// validated graph always resolves to a root; the visited guard
/// keeps a broken graph total.
pub(super) fn render_option(value: Option<&str>) -> String {
    value.unwrap_or("<none>").to_string()
}
fn render_list(values: &[String]) -> String {
    format!("[{}]", values.join(", "))
}

fn render_u32_list(values: &[u32]) -> String {
    let rendered: Vec<String> = values.iter().map(u32::to_string).collect();
    format!("[{}]", rendered.join(", "))
}

fn render_range(range: (u64, u64)) -> String {
    format!("[{}, {}]", range.0, range.1)
}

fn render_bbox(bbox: [f64; 4]) -> String {
    format!("[{}, {}, {}, {}]", bbox[0], bbox[1], bbox[2], bbox[3])
}

/// One field-change detail triple.
pub(super) fn change(field: &'static str, committed: &str, candidate: &str) -> DriftDetail {
    DriftDetail::FieldChange {
        field,
        committed: committed.to_string(),
        candidate: candidate.to_string(),
    }
}
