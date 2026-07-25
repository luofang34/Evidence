//! The layout projection: the bounded bbox model becomes
//! candidate structural source nodes under the committed recipe
//! rules (LLR-182).
//!
//! Reading order sorts each page's retained blocks by column band
//! (left of the configured split before right), then `yMin`, then
//! `xMin`. Header and footer band lines drop with typed
//! diagnostics; a footer line matching the configured page-label
//! prefix yields the page's printed label. A single-line block
//! whose first word is a dotted numbering token at or below the
//! configured heading depth opens a `section` carrying the
//! numbering as its label; configured note and caption prefixes
//! project as `note` and `figure_caption`; every other block
//! projects as one `paragraph` whose text joins the block's words
//! with spaces and its lines with newlines — verbatim, never
//! dehyphenated.
//!
//! No table row or cell node is ever claimed: a table-shaped
//! block (multi-line, multi-word, column-aligned) the configured
//! rules cannot prove produces a deterministic
//! [`PdfIngestDiagnosticKind::StructuralLoss`] diagnostic and
//! projects as a plain paragraph; recovery is an approved curated
//! patch's job. Every locator is [`SourceLocator::Pdf`] with the
//! 1-based physical page, the optional printed label, and the
//! element bounding box; positions are diagnostic, never
//! permanent identity.

use std::collections::BTreeMap;

use super::super::super::{CandidateNode, SourceLocator, SourceNodeKind};
use super::bbox::{Bbox, BboxBlock, BboxDocument};
use super::recipe::PdfLayoutRules;
use super::{PdfExcludedBand, PdfIngestDiagnostic, PdfIngestDiagnosticKind};

/// The x-alignment tolerance (points) of the table-shape
/// heuristic. Detection only drives the structural-loss
/// diagnostic; it never proves structure.
const COLUMN_TOLERANCE: f64 = 2.0;

/// The projection outcome: candidate nodes in reading order and
/// the unsorted diagnostics.
pub(crate) struct Projection {
    /// The candidate nodes, in emission (reading) order.
    pub candidates: Vec<CandidateNode>,
    /// The structural-loss diagnostics; the caller sorts them.
    pub diagnostics: Vec<PdfIngestDiagnostic>,
}

/// Project the parsed bbox document under `rules` (LLR-182).
/// Pure and deterministic.
pub(crate) fn project(document: &BboxDocument, rules: &PdfLayoutRules) -> Projection {
    let mut state = ProjectionState::default();
    for (page_index, page) in document.pages.iter().enumerate() {
        let page_number = page_index as u32 + 1;
        let printed_label = page_label(page, rules);
        let mut order: Vec<usize> = (0..page.blocks.len()).collect();
        order.sort_by(|a, b| {
            reading_key(&page.blocks[*a], rules).cmp(&reading_key(&page.blocks[*b], rules))
        });
        for (sorted_index, block_index) in order.iter().enumerate() {
            let block = &page.blocks[*block_index];
            project_block(
                &mut state,
                rules,
                block,
                page_number,
                *block_index as u32,
                sorted_index as u32,
                printed_label.as_deref(),
            );
        }
    }
    Projection {
        candidates: state.candidates,
        diagnostics: state.diagnostics,
    }
}

/// The reading-order sort key: column band, then vertical, then
/// horizontal position. Float positions sort through their
/// ordered bit patterns.
fn reading_key(block: &BboxBlock, rules: &PdfLayoutRules) -> (u8, u64, u64) {
    let band = match rules.column_split_x {
        Some(split) => u8::from((block.bbox[0] + block.bbox[2]) / 2.0 >= split),
        None => 0,
    };
    (band, block.bbox[1].to_bits(), block.bbox[0].to_bits())
}

/// The running projection state: candidates, diagnostics, the
/// open section stack, and the per-parent ordinal counters.
#[derive(Default)]
struct ProjectionState {
    candidates: Vec<CandidateNode>,
    diagnostics: Vec<PdfIngestDiagnostic>,
    /// Open sections as `(depth, provisional id)`, innermost
    /// last. The stack persists across pages: sections continue.
    sections: Vec<(u32, String)>,
    /// Next sibling ordinal per parent provisional id (`""` keys
    /// the root set).
    ordinals: BTreeMap<String, u32>,
}

impl ProjectionState {
    /// Emit one candidate under `parent`, assigning its
    /// provisional id and sibling ordinal.
    fn emit(
        &mut self,
        parent: Option<String>,
        kind: SourceNodeKind,
        label: Option<String>,
        text: String,
        locator: SourceLocator,
    ) -> String {
        let provisional = format!("cand-{}", self.candidates.len());
        let ordinal = self
            .ordinals
            .entry(parent.clone().unwrap_or_default())
            .or_insert(0);
        let candidate = CandidateNode {
            provisional_id: provisional.clone(),
            parent_id: parent,
            kind,
            ordinal: *ordinal,
            label,
            canonical_text: text,
            locator,
        };
        *ordinal += 1;
        self.candidates.push(candidate);
        provisional
    }

    /// The innermost open section's provisional id, if any.
    fn current_section(&self) -> Option<String> {
        self.sections.last().map(|(_, id)| id.clone())
    }
}

/// The page's printed label: the first footer-band line matching
/// the configured page-label prefix contributes its trimmed
/// remainder.
fn page_label(page: &super::bbox::BboxPage, rules: &PdfLayoutRules) -> Option<String> {
    let prefix = rules.page_label_prefix.as_ref()?;
    for block in &page.blocks {
        for line in &block.lines {
            if line.bbox[1] < rules.footer_top {
                continue;
            }
            let text = line_text(line);
            if let Some(remainder) = text.strip_prefix(prefix.as_str()) {
                let label = remainder.trim();
                if !label.is_empty() {
                    return Some(label.to_string());
                }
            }
        }
    }
    None
}

/// Project one block: classify, diagnose, and emit.
#[allow(
    clippy::too_many_arguments,
    reason = "the projection threads page context through"
)]
fn project_block(
    state: &mut ProjectionState,
    rules: &PdfLayoutRules,
    block: &BboxBlock,
    page: u32,
    block_index: u32,
    _sorted_index: u32,
    printed_label: Option<&str>,
) {
    let locator = |bbox: Bbox| SourceLocator::Pdf {
        physical_page: page,
        printed_label: printed_label.map(str::to_string),
        bbox,
    };
    let mut retained = Vec::new();
    for line in &block.lines {
        let band = if line.bbox[3] <= rules.header_bottom {
            Some(PdfExcludedBand::Header)
        } else if line.bbox[1] >= rules.footer_top {
            Some(PdfExcludedBand::Footer)
        } else {
            None
        };
        match band {
            Some(band) => state.diagnostics.push(PdfIngestDiagnostic {
                kind: PdfIngestDiagnosticKind::ExcludedByRule { band },
                page,
                block: block_index,
                bbox: line.bbox,
                detail: line_text(line),
            }),
            None => retained.push(line),
        }
    }
    if retained.is_empty() {
        if block.lines.is_empty() || block.lines.iter().all(|line| line.words.is_empty()) {
            state.diagnostics.push(PdfIngestDiagnostic {
                kind: PdfIngestDiagnosticKind::UnclassifiableBlock,
                page,
                block: block_index,
                bbox: block.bbox,
                detail: "the block carries no words".to_string(),
            });
        }
        return;
    }
    let first_word = retained
        .first()
        .and_then(|line| line.words.first())
        .map(|word| word.text.as_str())
        .unwrap_or("");
    let text = block_text(&retained);
    if rules.note_prefixes.contains(first_word) {
        state.emit(
            state.current_section(),
            SourceNodeKind::Note,
            None,
            text,
            locator(block.bbox),
        );
        return;
    }
    if rules.caption_prefixes.contains(first_word) {
        state.emit(
            state.current_section(),
            SourceNodeKind::FigureCaption,
            None,
            text,
            locator(block.bbox),
        );
        return;
    }
    if retained.len() == 1
        && let Some(depth) = numbering_depth(first_word)
        && depth <= rules.max_heading_depth
    {
        while state
            .sections
            .last()
            .is_some_and(|(open_depth, _)| *open_depth >= depth)
        {
            state.sections.pop();
        }
        let parent = state.current_section();
        let line = retained[0];
        let id = state.emit(
            parent,
            SourceNodeKind::Section,
            Some(first_word.to_string()),
            text,
            locator(line.bbox),
        );
        state.sections.push((depth, id));
        return;
    }
    if is_table_shaped(&retained) {
        state.diagnostics.push(PdfIngestDiagnostic {
            kind: PdfIngestDiagnosticKind::StructuralLoss { construct: "table" },
            page,
            block: block_index,
            bbox: block.bbox,
            detail: "column-aligned multi-line block; no rule proves row/cell structure"
                .to_string(),
        });
    }
    state.emit(
        state.current_section(),
        SourceNodeKind::Paragraph,
        None,
        text,
        locator(block.bbox),
    );
}

/// The dotted-numbering depth of a token (`1` → 1, `1.2` → 2),
/// or `None` when the token is not a numbering token.
fn numbering_depth(token: &str) -> Option<u32> {
    if token.is_empty() {
        return None;
    }
    let mut depth = 0_u32;
    for component in token.split('.') {
        if component.is_empty() || !component.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        depth += 1;
    }
    Some(depth)
}

/// A block is table-shaped when at least two of its lines carry
/// at least three words each and two words across two lines align
/// on their left edge within [`COLUMN_TOLERANCE`]. Detection
/// drives the structural-loss diagnostic only.
fn is_table_shaped(lines: &[&super::bbox::BboxLine]) -> bool {
    let wide: Vec<&&super::bbox::BboxLine> =
        lines.iter().filter(|line| line.words.len() >= 3).collect();
    for (index, line) in wide.iter().enumerate() {
        for other in &wide[index + 1..] {
            let aligned = line
                .words
                .iter()
                .filter(|word| {
                    other.words.iter().any(|candidate| {
                        (word.bbox[0] - candidate.bbox[0]).abs() <= COLUMN_TOLERANCE
                    })
                })
                .count();
            if aligned >= 2 {
                return true;
            }
        }
    }
    false
}

/// One line's text: its words joined with spaces, verbatim.
fn line_text(line: &super::bbox::BboxLine) -> String {
    line.words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// One block's canonical text: lines joined with newlines,
/// verbatim — never dehyphenated.
fn block_text(lines: &[&super::bbox::BboxLine]) -> String {
    lines
        .iter()
        .map(|line| line_text(line))
        .collect::<Vec<_>>()
        .join("\n")
}
