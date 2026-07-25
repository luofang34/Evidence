//! Candidate emission, inline text collection, and the table
//! grid projection (LLR-165).
//!
//! [`Adapter::emit`] is the single candidate-construction point:
//! it enforces the node-count and text-size bounds, assigns the
//! sibling ordinal and provisional identity, and stamps the HTML
//! locator — canonical URL, final URL when distinct, fragment,
//! heading path, and DOM path. [`Adapter::collect_text`] folds a
//! subtree's inline content: text nodes concatenate, `br` breaks
//! lines, inline formatting is transparent, internal links are
//! recorded for the resolution check, closed-rule drops diagnose,
//! structural blocks skip (they project as their own nodes) or
//! fold (in captions and stray runs), and unknown elements
//! diagnose while their text is retained. [`Adapter::table`]
//! projects the table grid: header rows carry unique `header N`
//! labels in document order, and row/column spans project as one
//! cell per covered grid slot — the origin slot carries the
//! text, continuation slots are empty — so the table's full
//! row/column shape is recoverable from the graph.

use std::collections::BTreeMap;

use ego_tree::NodeRef;
use scraper::node::Node;

use super::adapter::{AbsorbFrame, Adapter, MAX_NODES, MAX_TEXT_BYTES, is_dropped};
use super::error::{HtmlIngestDiagnosticKind, HtmlIngestError};
use crate::corpus::{CandidateNode, SourceLocator, SourceNodeKind, normalize_prose};

/// The HTML-spec maximum column span; larger values clamp.
const MAX_COLSPAN: u32 = 1000;
/// The HTML-spec maximum row span; larger values clamp.
const MAX_ROWSPAN: u32 = 65534;

/// The content of one candidate node emission: everything but the
/// parent link, the DOM path, and the assigned ordinal/identity.
pub(super) struct EmitContent {
    pub(super) kind: SourceNodeKind,
    pub(super) label: Option<String>,
    pub(super) canonical_text: String,
    pub(super) fragment: Option<String>,
    pub(super) heading_path: Vec<String>,
}

/// One collected table row: the row element, its DOM path, and
/// whether it originates from `thead`.
type TableRowRef<'d> = (NodeRef<'d, Node>, Vec<u32>, bool);

impl Adapter<'_> {
    /// Emit one candidate node: the single construction point
    /// pinned by the module docs. Returns the provisional id.
    pub(super) fn emit(
        &mut self,
        parent: Option<String>,
        path: &[u32],
        content: EmitContent,
    ) -> Result<String, HtmlIngestError> {
        let ordinal = self.next_ordinal(&parent);
        let provisional = self.provisional();
        self.push_candidate(CandidateNode {
            provisional_id: provisional.clone(),
            parent_id: parent,
            kind: content.kind,
            ordinal,
            label: content.label,
            canonical_text: content.canonical_text,
            locator: self.locator(path, content.fragment, content.heading_path),
        })?;
        Ok(provisional)
    }

    /// Emit the note node of a closed absorber frame, whose
    /// provisional identity, parent, and ordinal were assigned at
    /// open so nested sibling blocks order after it.
    pub(super) fn emit_closed_note(
        &mut self,
        frame: AbsorbFrame,
        canonical_text: String,
    ) -> Result<(), HtmlIngestError> {
        self.push_candidate(CandidateNode {
            provisional_id: frame.provisional,
            parent_id: frame.parent,
            kind: SourceNodeKind::Note,
            ordinal: frame.ordinal,
            label: None,
            canonical_text,
            locator: self.locator(&frame.path, None, frame.heading_path),
        })
    }

    /// The HTML locator of one candidate.
    fn locator(
        &self,
        path: &[u32],
        fragment: Option<String>,
        heading_path: Vec<String>,
    ) -> SourceLocator {
        SourceLocator::Html {
            canonical_url: self.input.canonical_url.to_string(),
            final_url: self.input.final_url.clone(),
            fragment,
            heading_path,
            dom_path: path.to_vec(),
        }
    }

    /// Push one candidate, enforcing the node-count and text-size
    /// bounds.
    fn push_candidate(&mut self, candidate: CandidateNode) -> Result<(), HtmlIngestError> {
        if candidate.canonical_text.len() > MAX_TEXT_BYTES {
            return Err(HtmlIngestError::TextTooLarge {
                size: candidate.canonical_text.len(),
                limit: MAX_TEXT_BYTES,
            });
        }
        if self.candidates.len() >= MAX_NODES {
            return Err(HtmlIngestError::TooManyNodes {
                count: self.candidates.len() + 1,
                limit: MAX_NODES,
            });
        }
        self.candidates.push(candidate);
        Ok(())
    }

    /// Fold one subtree's inline content into `out`, following the
    /// rules in the module docs. `fold_blocks` includes structural
    /// blocks' text (captions, stray runs); without it they skip —
    /// they project as their own nodes.
    pub(super) fn collect_text(
        &mut self,
        node: NodeRef<Node>,
        path: &[u32],
        out: &mut String,
        fold_blocks: bool,
    ) -> Result<(), HtmlIngestError> {
        let mut element_index = 0u32;
        for child in node.children() {
            let Some(el) = child.value().as_element() else {
                if let Some(text) = child.value().as_text() {
                    out.push_str(&text.text);
                }
                continue;
            };
            let mut child_path = path.to_vec();
            child_path.push(element_index);
            element_index += 1;
            let name = el.name();
            if is_dropped(name) {
                if super::adapter::carries_content(child) {
                    self.diagnose(
                        HtmlIngestDiagnosticKind::DroppedByClosedRule {
                            tag: super::walk::static_tag(name),
                        },
                        &child_path,
                        format!("element <{name}> drops by the closed rule"),
                    );
                }
                continue;
            }
            match name {
                "br" | "wbr" => out.push('\n'),
                "a" => {
                    self.record_link(el, &child_path);
                    self.collect_text(child, &child_path, out, fold_blocks)?;
                }
                "ul" | "ol" | "dl" | "table" | "figure" | "blockquote" | "pre" | "h1" | "h2"
                | "h3" | "h4" | "h5" | "h6"
                    if !fold_blocks => {}
                "p" => {
                    if !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    self.collect_text(child, &child_path, out, fold_blocks)?;
                }
                _ if fold_blocks || super::adapter::is_inline(name) => {
                    self.collect_text(child, &child_path, out, fold_blocks)?;
                }
                _ => {
                    self.diagnose(
                        HtmlIngestDiagnosticKind::UnsupportedElement {
                            tag: name.to_string(),
                        },
                        &child_path,
                        format!(
                            "element <{name}> is not represented; its text is retained in the enclosing block"
                        ),
                    );
                    self.collect_text(child, &child_path, out, fold_blocks)?;
                }
            }
        }
        Ok(())
    }

    /// Project one table: a table node, one row node per `tr` in
    /// document order (header rows labeled `header N`), and one
    /// cell node per covered grid slot under the grid algorithm in
    /// the module docs.
    pub(super) fn table(
        &mut self,
        node: NodeRef<Node>,
        path: &[u32],
    ) -> Result<(), HtmlIngestError> {
        let parent = self.section_parent();
        let heading_path = self.section_path();
        let table = self.emit(
            parent,
            path,
            EmitContent {
                kind: SourceNodeKind::Table,
                label: None,
                canonical_text: String::new(),
                fragment: None,
                heading_path,
            },
        )?;
        let rows = collect_rows(self, node, path)?;
        // Columns an earlier row's rowspan still covers, mapped to
        // the number of further rows covered.
        let mut pending: BTreeMap<u32, u32> = BTreeMap::new();
        for (row, row_path, is_header) in rows {
            let label = if is_header {
                self.header_rows += 1;
                Some(format!("header {}", self.header_rows))
            } else {
                None
            };
            let row_id = self.emit(
                Some(table.clone()),
                &row_path,
                EmitContent {
                    kind: SourceNodeKind::TableRow,
                    label,
                    canonical_text: String::new(),
                    fragment: None,
                    heading_path: self.section_path(),
                },
            )?;
            self.table_row(row, &row_path, &row_id, &mut pending)?;
        }
        Ok(())
    }

    /// Project one row's cells under the span grid algorithm:
    /// pending rowspan columns emit empty placeholder cells, each
    /// `th`/`td` emits an origin cell plus `colspan - 1` empty
    /// continuation cells, and `rowspan > 1` marks its columns
    /// pending for the following rows.
    fn table_row(
        &mut self,
        row: NodeRef<Node>,
        row_path: &[u32],
        row_id: &str,
        pending: &mut BTreeMap<u32, u32>,
    ) -> Result<(), HtmlIngestError> {
        let mut carry = std::mem::take(pending);
        let mut col = 0u32;
        let mut cell_index = 0u32;
        let mut new_pending: Vec<(u32, u32)> = Vec::new();
        for cell in row.children() {
            let Some(el) = cell.value().as_element() else {
                continue;
            };
            if !matches!(el.name(), "th" | "td") {
                continue;
            }
            let mut cell_path = row_path.to_vec();
            cell_path.push(cell_index);
            cell_index += 1;
            loop {
                let covered = carry.range(col..).next().map(|(&c, &r)| (c, r));
                let Some((covered, remaining)) = covered else {
                    break;
                };
                if covered != col {
                    break;
                }
                self.emit_cell(row_id, String::new(), row_path)?;
                if remaining > 1 {
                    pending.insert(covered, remaining - 1);
                }
                carry.remove(&covered);
                col += 1;
            }
            let colspan = span(el, "colspan", MAX_COLSPAN);
            let rowspan = span(el, "rowspan", MAX_ROWSPAN);
            let mut raw = String::new();
            self.collect_text(cell, &cell_path, &mut raw, false)?;
            self.emit_cell(row_id, normalize_prose(&raw), &cell_path)?;
            for _ in 1..colspan {
                self.emit_cell(row_id, String::new(), &cell_path)?;
            }
            if rowspan > 1 {
                for covered in col..col + colspan {
                    new_pending.push((covered, rowspan - 1));
                }
            }
            col += colspan;
            self.walk_nested_blocks(cell, &cell_path)?;
        }
        for (&covered, &remaining) in carry.range(col..) {
            self.emit_cell(row_id, String::new(), row_path)?;
            if remaining > 1 {
                pending.insert(covered, remaining - 1);
            }
        }
        for (covered, remaining) in new_pending {
            pending.insert(covered, remaining);
        }
        Ok(())
    }

    /// Emit one table-cell node under `row_id`.
    fn emit_cell(
        &mut self,
        row_id: &str,
        canonical_text: String,
        path: &[u32],
    ) -> Result<(), HtmlIngestError> {
        let heading_path = self.section_path();
        self.emit(
            Some(row_id.to_string()),
            path,
            EmitContent {
                kind: SourceNodeKind::TableCell,
                label: None,
                canonical_text,
                fragment: None,
                heading_path,
            },
        )?;
        Ok(())
    }
}

/// Collect a table's rows in document order as
/// `(row, row_path, is_header)`: `thead` rows are header rows;
/// `tbody`, `tfoot`, and stray `tr` children are body rows. Any
/// other child diagnoses per its rule.
fn collect_rows<'d>(
    adapter: &mut Adapter<'_>,
    table: NodeRef<'d, Node>,
    path: &[u32],
) -> Result<Vec<TableRowRef<'d>>, HtmlIngestError> {
    let mut rows = Vec::new();
    let mut element_index = 0u32;
    for child in table.children() {
        let Some(el) = child.value().as_element() else {
            continue;
        };
        let mut child_path = path.to_vec();
        child_path.push(element_index);
        element_index += 1;
        let name = el.name();
        match name {
            "thead" | "tbody" | "tfoot" => {
                let is_header = name == "thead";
                let mut group_index = 0u32;
                for row in child.children() {
                    let Some(row_el) = row.value().as_element() else {
                        continue;
                    };
                    let mut row_path = child_path.clone();
                    row_path.push(group_index);
                    group_index += 1;
                    if row_el.name() == "tr" {
                        rows.push((row, row_path, is_header));
                    } else {
                        diagnose_table_child(adapter, row, &row_path);
                    }
                }
            }
            "tr" => rows.push((child, child_path, false)),
            _ => diagnose_table_child(adapter, child, &child_path),
        }
    }
    Ok(rows)
}

/// Diagnose a non-row child of a table or row group: closed-rule
/// drops and column metadata drop, diagnosed when they carry
/// content; anything else is unsupported.
fn diagnose_table_child(adapter: &mut Adapter<'_>, child: NodeRef<'_, Node>, path: &[u32]) {
    let Some(el) = child.value().as_element() else {
        return;
    };
    let name = el.name();
    if is_dropped(name) {
        if super::adapter::carries_content(child) {
            adapter.diagnose(
                HtmlIngestDiagnosticKind::DroppedByClosedRule {
                    tag: super::walk::static_tag(name),
                },
                path,
                format!("element <{name}> drops by the closed rule"),
            );
        }
        return;
    }
    adapter.diagnose(
        HtmlIngestDiagnosticKind::UnsupportedElement {
            tag: name.to_string(),
        },
        path,
        format!("element <{name}> is not represented in the table projection"),
    );
}

/// A cell's span attribute: a positive integer clamped to `max`;
/// absent, non-numeric, and zero values mean 1, following the
/// HTML spec's error recovery.
fn span(el: &scraper::node::Element, attribute: &str, max: u32) -> u32 {
    el.attr(attribute)
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(1)
        .min(max)
}
