//! The event-walking adapter state (LLR-162).
//!
//! [`Adapter`] consumes balanced parser events through a frame stack
//! and produces candidate nodes plus typed diagnostics. Frames are
//! pushed for block tags only; inline tags are transparent (their
//! text events land in the innermost open block frame). Node
//! creation happens at frame close, when the frame's text is
//! assembled (`close` module).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use pulldown_cmark::{Event, Tag};

use super::super::{IngestDiagnostic, IngestDiagnosticKind, IngestError, IngestMarkdownInput};
use crate::corpus::{CandidateNode, SafeRelPath};

/// Which block a frame represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FrameKind {
    Section,
    Paragraph,
    ListItem,
    Table,
    TableRow,
    TableCell,
    CodeBlock,
    Note,
    FootnoteNote,
    /// A block that produces no node (`List`, `HtmlBlock`).
    Transparent,
}

impl FrameKind {
    /// Frames that absorb an enclosed paragraph's text instead of
    /// letting it become its own node.
    pub(super) fn absorbs_paragraph(self) -> bool {
        matches!(
            self,
            FrameKind::ListItem | FrameKind::Note | FrameKind::FootnoteNote | FrameKind::TableCell
        )
    }
}

/// One open block.
#[derive(Debug)]
pub(super) struct Frame {
    pub(super) kind: FrameKind,
    /// Assembled raw inline text (adjacent text concatenated, breaks
    /// as `\n`, paragraphs joined with `\n`).
    pub(super) text: String,
    /// Byte offset of the opening event.
    pub(super) start: usize,
    /// Candidate-local identity; assigned at open.
    pub(super) provisional: String,
    /// Resolved parent provisional id; `None` for a root or for a
    /// paragraph whose fate is decided at close.
    pub(super) parent: Option<String>,
    /// Position within the parent's sibling set; assigned at open
    /// except for paragraphs (see `parent`).
    pub(super) ordinal: u32,
    /// Heading rank 1–6 (Section frames).
    pub(super) heading_level: usize,
    /// Footnote identifier (FootnoteNote frames).
    pub(super) footnote_id: String,
    /// A duplicate footnote definition: diagnosed at close, no node.
    pub(super) skip: bool,
}

impl Frame {
    fn bare(kind: FrameKind, provisional: String, start: usize) -> Self {
        Self {
            kind,
            text: String::new(),
            start,
            provisional,
            parent: None,
            ordinal: 0,
            heading_level: 0,
            footnote_id: String::new(),
            skip: false,
        }
    }
}

/// One logical section in the heading trail.
#[derive(Debug)]
pub(super) struct SectionEntry {
    pub(super) level: usize,
    pub(super) provisional: String,
    /// Ancestor section labels plus this section's own label.
    pub(super) path: Vec<String>,
}

/// The adapter state for one parse.
#[derive(Debug)]
pub(super) struct Adapter<'i> {
    pub(super) input: &'i IngestMarkdownInput<'i>,
    pub(super) path: SafeRelPath,
    pub(super) frames: Vec<Frame>,
    pub(super) sections: Vec<SectionEntry>,
    pub(super) candidates: Vec<CandidateNode>,
    pub(super) diagnostics: Vec<IngestDiagnostic>,
    pub(super) ordinals: BTreeMap<Option<String>, u32>,
    pub(super) used_anchors: BTreeSet<String>,
    pub(super) footnote_ids: BTreeSet<String>,
    pub(super) next_provisional: u64,
}

impl<'i> Adapter<'i> {
    pub(super) fn new(input: &'i IngestMarkdownInput<'i>, path: SafeRelPath) -> Self {
        Self {
            input,
            path,
            frames: Vec::new(),
            sections: Vec::new(),
            candidates: Vec::new(),
            diagnostics: Vec::new(),
            ordinals: BTreeMap::new(),
            used_anchors: BTreeSet::new(),
            footnote_ids: BTreeSet::new(),
            next_provisional: 0,
        }
    }

    /// Consume one parser event with its byte range.
    pub(super) fn event(
        &mut self,
        text: &str,
        event: Event,
        range: Range<usize>,
    ) -> Result<(), IngestError> {
        match event {
            Event::Start(tag) => self.open(&tag, range),
            Event::End(tag_end) => self.close(text, &tag_end, range)?,
            Event::Text(t) | Event::Code(t) | Event::InlineMath(t) | Event::DisplayMath(t) => {
                if let Some(top) = self.frames.last_mut() {
                    top.text.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(top) = self.frames.last_mut() {
                    top.text.push('\n');
                }
            }
            Event::FootnoteReference(_) | Event::TaskListMarker(_) => {}
            Event::Html(_) | Event::InlineHtml(_) => {
                self.diagnose(
                    IngestDiagnosticKind::UnsupportedRawHtml,
                    &range,
                    "raw HTML is not represented in the structural projection".to_string(),
                );
            }
            Event::Rule => {
                self.diagnose(
                    IngestDiagnosticKind::LossyConstruct {
                        construct: "thematic-break",
                    },
                    &range,
                    "a thematic break is not represented in the structural projection".to_string(),
                );
            }
        }
        Ok(())
    }

    /// The next candidate-local identity.
    pub(super) fn provisional(&mut self) -> String {
        let id = format!("cand-{}", self.next_provisional);
        self.next_provisional += 1;
        id
    }

    /// The next sibling ordinal under `parent`.
    pub(super) fn next_ordinal(&mut self, parent: &Option<String>) -> u32 {
        let ordinal = self.ordinals.entry(parent.clone()).or_insert(0);
        let current = *ordinal;
        *ordinal += 1;
        current
    }

    /// The innermost open section's provisional id, if any.
    pub(super) fn section_parent(&self) -> Option<String> {
        self.sections.last().map(|s| s.provisional.clone())
    }

    /// The innermost open section's heading path, or empty at the
    /// document root.
    pub(super) fn section_path(&self) -> Vec<String> {
        self.sections
            .last()
            .map(|s| s.path.clone())
            .unwrap_or_default()
    }

    /// The provisional id of the innermost open frame of `kind`,
    /// falling back to the enclosing section (a parser-balanced
    /// document always finds the frame; the fallback keeps the
    /// adapter total).
    fn frame_parent(&self, kind: FrameKind) -> Option<String> {
        self.frames
            .iter()
            .rev()
            .find(|frame| frame.kind == kind)
            .map(|frame| frame.provisional.clone())
            .or_else(|| self.section_parent())
    }

    /// Push a node-producing frame whose parent and ordinal resolve
    /// at open.
    fn push_node_frame(&mut self, kind: FrameKind, parent: Option<String>, start: usize) {
        let ordinal = self.next_ordinal(&parent);
        let provisional = self.provisional();
        let mut frame = Frame::bare(kind, provisional, start);
        frame.parent = parent;
        frame.ordinal = ordinal;
        self.frames.push(frame);
    }

    /// Record one typed diagnostic.
    pub(super) fn diagnose(
        &mut self,
        kind: IngestDiagnosticKind,
        range: &Range<usize>,
        detail: String,
    ) {
        self.diagnostics.push(IngestDiagnostic {
            kind,
            byte_range: (range.start as u64, range.end as u64),
            detail,
        });
    }

    /// Open one block tag.
    fn open(&mut self, tag: &Tag, range: Range<usize>) {
        match tag {
            Tag::Heading { level, .. } => {
                let level = *level as usize;
                while self.sections.last().is_some_and(|s| s.level >= level) {
                    self.sections.pop();
                }
                let parent = self.section_parent();
                self.push_node_frame(FrameKind::Section, parent, range.start);
                if let Some(frame) = self.frames.last_mut() {
                    frame.heading_level = level;
                }
            }
            Tag::Paragraph => {
                let provisional = self.provisional();
                self.frames
                    .push(Frame::bare(FrameKind::Paragraph, provisional, range.start));
            }
            Tag::BlockQuote(_) => {
                let parent = self.section_parent();
                self.push_node_frame(FrameKind::Note, parent, range.start);
            }
            Tag::CodeBlock(_) => {
                let parent = self.section_parent();
                self.push_node_frame(FrameKind::CodeBlock, parent, range.start);
            }
            Tag::List(_) | Tag::HtmlBlock => {
                let provisional = self.provisional();
                self.frames.push(Frame::bare(
                    FrameKind::Transparent,
                    provisional,
                    range.start,
                ));
            }
            Tag::Item => {
                let parent = self.section_parent();
                self.push_node_frame(FrameKind::ListItem, parent, range.start);
            }
            Tag::Table(_) => {
                let parent = self.section_parent();
                self.push_node_frame(FrameKind::Table, parent, range.start);
            }
            Tag::TableHead | Tag::TableRow => {
                let parent = self.frame_parent(FrameKind::Table);
                self.push_node_frame(FrameKind::TableRow, parent, range.start);
            }
            Tag::TableCell => {
                let parent = self.frame_parent(FrameKind::TableRow);
                self.push_node_frame(FrameKind::TableCell, parent, range.start);
            }
            Tag::FootnoteDefinition(name) => {
                let parent = self.section_parent();
                self.push_node_frame(FrameKind::FootnoteNote, parent, range.start);
                let duplicate = !self.footnote_ids.insert(name.to_string());
                if let Some(frame) = self.frames.last_mut() {
                    frame.footnote_id = name.to_string();
                    frame.skip = duplicate;
                }
            }
            Tag::Image { dest_url, .. } => {
                self.diagnose(
                    IngestDiagnosticKind::LossyConstruct { construct: "image" },
                    &range,
                    format!(
                        "image target {dest_url:?} is not represented in the structural projection"
                    ),
                );
            }
            Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Superscript
            | Tag::Subscript
            | Tag::Link { .. }
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::MetadataBlock(_) => {}
        }
    }
}

/// A checked locator byte range: ordered, in bounds, and aligned to
/// UTF-8 boundaries. Parser-produced ranges always satisfy this; the
/// check keeps construction fail-closed.
pub(super) fn checked_byte_range(
    text: &str,
    start: usize,
    end: usize,
) -> Result<(u64, u64), IngestError> {
    let valid = start <= end
        && end <= text.len()
        && text.is_char_boundary(start)
        && text.is_char_boundary(end);
    if valid {
        Ok((start as u64, end as u64))
    } else {
        Err(IngestError::ByteRangeUnaligned {
            start: start as u64,
            end: end as u64,
            len: text.len(),
        })
    }
}

/// Strip a leading GFM alert marker (`[!NOTE]` and friends,
/// case-insensitive) plus any following whitespace; return the text
/// unchanged when no marker leads.
pub(super) fn strip_alert_marker(text: &str) -> &str {
    /// The GFM alert kinds a leading `[!KIND]` marker may name.
    const ALERT_KINDS: [&str; 5] = ["NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION"];
    let trimmed = text.trim_start();
    let Some(rest) = trimmed.strip_prefix("[!") else {
        return text;
    };
    let Some(close) = rest.find(']') else {
        return text;
    };
    if ALERT_KINDS
        .iter()
        .any(|kind| rest[..close].eq_ignore_ascii_case(kind))
    {
        rest[close + 1..].trim_start()
    } else {
        text
    }
}
