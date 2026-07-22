//! Frame closing: node emission and heading processing (LLR-162).
//!
//! Node creation happens at frame close, when the frame's inline
//! text is assembled. The mapping is pinned by the `markdown`
//! module docs; this module implements it.

use std::ops::Range;

use pulldown_cmark::TagEnd;

use super::super::{IngestDiagnosticKind, IngestError};
use super::adapter::{Adapter, Frame, SectionEntry, checked_byte_range, strip_alert_marker};
use super::anchors::{self, ExplicitId};
use crate::corpus::{
    CandidateNode, SourceLocator, SourceNodeKind, normalize_code, normalize_prose,
};

/// The content one closed frame emits as a candidate node.
struct NodeContent {
    kind: SourceNodeKind,
    label: Option<String>,
    canonical_text: String,
    anchor: Option<String>,
    heading_path: Vec<String>,
}

impl Adapter<'_> {
    /// Close one block tag: pop its frame and emit its node, or fold
    /// its text into an absorbing ancestor.
    pub(super) fn close(
        &mut self,
        text: &str,
        end: &TagEnd,
        range: Range<usize>,
    ) -> Result<(), IngestError> {
        match end {
            TagEnd::Heading(_) => {
                let Some(frame) = self.frames.pop() else {
                    return Ok(());
                };
                self.close_section(text, frame, &range)
            }
            TagEnd::Paragraph => {
                let Some(frame) = self.frames.pop() else {
                    return Ok(());
                };
                self.close_paragraph(text, frame, &range)
            }
            TagEnd::BlockQuote(_) => {
                let Some(frame) = self.frames.pop() else {
                    return Ok(());
                };
                let stripped = strip_alert_marker(&frame.text).to_string();
                let canonical = normalize_prose(&stripped);
                self.emit_simple(
                    text,
                    frame,
                    SourceNodeKind::Note,
                    None,
                    canonical,
                    range.end,
                )
            }
            TagEnd::CodeBlock => {
                let Some(frame) = self.frames.pop() else {
                    return Ok(());
                };
                let canonical = normalize_code(&frame.text);
                self.emit_simple(
                    text,
                    frame,
                    SourceNodeKind::CodeBlock,
                    None,
                    canonical,
                    range.end,
                )
            }
            TagEnd::List(_) | TagEnd::HtmlBlock => {
                self.frames.pop();
                Ok(())
            }
            TagEnd::Item => {
                let Some(frame) = self.frames.pop() else {
                    return Ok(());
                };
                let canonical = normalize_prose(&frame.text);
                self.emit_simple(
                    text,
                    frame,
                    SourceNodeKind::ListItem,
                    None,
                    canonical,
                    range.end,
                )
            }
            TagEnd::FootnoteDefinition => {
                let Some(frame) = self.frames.pop() else {
                    return Ok(());
                };
                self.close_footnote(text, frame, &range)
            }
            TagEnd::Table => {
                let Some(frame) = self.frames.pop() else {
                    return Ok(());
                };
                self.emit_simple(
                    text,
                    frame,
                    SourceNodeKind::Table,
                    None,
                    String::new(),
                    range.end,
                )
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                let Some(frame) = self.frames.pop() else {
                    return Ok(());
                };
                self.emit_simple(
                    text,
                    frame,
                    SourceNodeKind::TableRow,
                    None,
                    String::new(),
                    range.end,
                )
            }
            TagEnd::TableCell => {
                let Some(frame) = self.frames.pop() else {
                    return Ok(());
                };
                let canonical = normalize_prose(&frame.text);
                self.emit_simple(
                    text,
                    frame,
                    SourceNodeKind::TableCell,
                    None,
                    canonical,
                    range.end,
                )
            }
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::Link
            | TagEnd::Image
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::MetadataBlock(_) => Ok(()),
        }
    }

    /// Emit one candidate node from a closed frame. Non-section
    /// nodes carry no anchor and the enclosing section trail as
    /// their heading path.
    fn emit_simple(
        &mut self,
        text: &str,
        frame: Frame,
        kind: SourceNodeKind,
        label: Option<String>,
        canonical_text: String,
        end: usize,
    ) -> Result<(), IngestError> {
        let content = NodeContent {
            kind,
            label,
            canonical_text,
            anchor: None,
            heading_path: self.section_path(),
        };
        self.emit(text, frame, content, end)
    }

    /// Emit one candidate node from a closed frame.
    fn emit(
        &mut self,
        text: &str,
        frame: Frame,
        content: NodeContent,
        end: usize,
    ) -> Result<(), IngestError> {
        let byte_range = checked_byte_range(text, frame.start, end)?;
        self.candidates.push(CandidateNode {
            provisional_id: frame.provisional,
            parent_id: frame.parent,
            kind: content.kind,
            ordinal: frame.ordinal,
            label: content.label,
            canonical_text: content.canonical_text,
            locator: SourceLocator::Markdown {
                path: self.path.clone(),
                git_blob: self.input.git_blob.clone(),
                anchor: content.anchor,
                heading_path: content.heading_path,
                byte_range,
            },
        });
        Ok(())
    }

    /// Close a heading: extract its anchor, build its label and
    /// heading path, emit the section, and push the section trail
    /// entry.
    fn close_section(
        &mut self,
        text: &str,
        frame: Frame,
        range: &Range<usize>,
    ) -> Result<(), IngestError> {
        let (label_text, anchor) = self.process_heading(&frame.text, range);
        let label = if label_text.is_empty() {
            None
        } else {
            Some(label_text)
        };
        let canonical = label.clone().unwrap_or_default();
        let mut heading_path = self.section_path();
        if let Some(label) = &label {
            heading_path.push(label.clone());
        }
        let level = frame.heading_level;
        let provisional = frame.provisional.clone();
        let end = range.end;
        let content = NodeContent {
            kind: SourceNodeKind::Section,
            label,
            canonical_text: canonical,
            anchor: Some(anchor),
            heading_path: heading_path.clone(),
        };
        self.emit(text, frame, content, end)?;
        self.sections.push(SectionEntry {
            level,
            provisional,
            path: heading_path,
        });
        Ok(())
    }

    /// Resolve a heading's label text and anchor: the explicit
    /// `{#id}` when present and valid, else the generated slug.
    /// Malformed ids and repeated explicit anchors diagnose.
    fn process_heading(&mut self, raw: &str, range: &Range<usize>) -> (String, String) {
        match anchors::extract_explicit_id(raw) {
            ExplicitId::None => {
                let label = normalize_prose(raw);
                let anchor = anchors::dedup(&mut self.used_anchors, anchors::slugify(&label));
                (label, anchor)
            }
            ExplicitId::Valid { label_prefix, id } => {
                if !self.used_anchors.insert(id.clone()) {
                    self.diagnose(
                        IngestDiagnosticKind::DuplicateAnchor { anchor: id.clone() },
                        range,
                        format!("explicit heading anchor {id:?} is claimed more than once"),
                    );
                }
                (normalize_prose(&label_prefix), id)
            }
            ExplicitId::Malformed { raw: bad } => {
                self.diagnose(
                    IngestDiagnosticKind::MalformedExplicitId { raw: bad.clone() },
                    range,
                    format!("trailing heading id {{#{bad}}} is not a valid explicit id"),
                );
                let label = normalize_prose(raw);
                let anchor = anchors::dedup(&mut self.used_anchors, anchors::slugify(&label));
                (label, anchor)
            }
        }
    }

    /// Close a paragraph: fold its text into the nearest absorbing
    /// ancestor (list item, block quote, footnote definition, table
    /// cell), or emit it as its own node at the enclosing section.
    fn close_paragraph(
        &mut self,
        text: &str,
        mut frame: Frame,
        range: &Range<usize>,
    ) -> Result<(), IngestError> {
        let absorber = self
            .frames
            .iter_mut()
            .rev()
            .find(|frame| frame.kind.absorbs_paragraph());
        if let Some(absorber) = absorber {
            if !absorber.text.is_empty() && !frame.text.is_empty() {
                absorber.text.push('\n');
            }
            absorber.text.push_str(&frame.text);
            return Ok(());
        }
        frame.parent = self.section_parent();
        frame.ordinal = self.next_ordinal(&frame.parent);
        let canonical = normalize_prose(&frame.text);
        self.emit_simple(
            text,
            frame,
            SourceNodeKind::Paragraph,
            None,
            canonical,
            range.end,
        )
    }

    /// Close a footnote definition: a duplicate id produces a
    /// lossy-construct diagnostic and no node; the first definition
    /// emits a note labeled `footnote:<id>`.
    fn close_footnote(
        &mut self,
        text: &str,
        frame: Frame,
        range: &Range<usize>,
    ) -> Result<(), IngestError> {
        if frame.skip {
            self.diagnose(
                IngestDiagnosticKind::LossyConstruct {
                    construct: "footnote-definition",
                },
                range,
                format!(
                    "footnote definition {:?} duplicates an earlier definition",
                    frame.footnote_id
                ),
            );
            return Ok(());
        }
        let canonical = normalize_prose(&frame.text);
        let label = Some(format!("footnote:{}", frame.footnote_id));
        self.emit_simple(
            text,
            frame,
            SourceNodeKind::Note,
            label,
            canonical,
            range.end,
        )
    }
}
