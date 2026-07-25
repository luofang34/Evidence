//! The document-order DOM walk: element dispatch and node
//! handlers (LLR-165).
//!
//! `walk_children` traverses one container's children in document
//! order. Text nodes and inline elements accumulate into a stray
//! buffer that flushes as a paragraph (or folds into an open
//! absorber) when a block element interrupts or the container
//! closes, so unwrapped prose — a potentially normative region —
//! is always retained. Block elements dispatch on their tag:
//! native structural tags map to their node kinds, recipe
//! classification selectors apply to elements without a native
//! mapping, known containers walk through, and anything else
//! produces an unsupported-element diagnostic while its children
//! are retained.

use ego_tree::NodeRef;
use scraper::node::Node;

use super::super::markdown::anchors;
use super::adapter::{
    Adapter, NESTED_BLOCK_TAGS, SectionEntry, is_container, is_dropped, is_inline, skips_subtree,
};
use super::emit::EmitContent;
use super::error::{HtmlIngestDiagnosticKind, HtmlIngestError};
use crate::corpus::{SourceNodeKind, normalize_code, normalize_prose};

impl Adapter<'_> {
    /// Walk one container's children in document order, folding
    /// stray text and inline elements into paragraphs.
    pub(super) fn walk_children(
        &mut self,
        node: NodeRef<Node>,
        path: &[u32],
    ) -> Result<(), HtmlIngestError> {
        let mut stray = String::new();
        let mut element_index = 0u32;
        for child in node.children() {
            let Some(el) = child.value().as_element() else {
                if let Some(text) = child.value().as_text() {
                    stray.push_str(&text.text);
                }
                continue;
            };
            let mut child_path = path.to_vec();
            child_path.push(element_index);
            element_index += 1;
            if is_inline(el.name()) {
                self.collect_text(child, &child_path, &mut stray, true)?;
                continue;
            }
            self.flush_stray(&mut stray, path)?;
            self.walk_element(child, &child_path)?;
        }
        self.flush_stray(&mut stray, path)
    }

    /// Flush the stray-text buffer: fold into the open absorber
    /// when one exists, else emit a paragraph node. Whitespace-only
    /// strays project nothing.
    fn flush_stray(&mut self, stray: &mut String, path: &[u32]) -> Result<(), HtmlIngestError> {
        let text = std::mem::take(stray);
        if text.trim().is_empty() {
            return Ok(());
        }
        if let Some(absorber) = self.absorbers.last_mut() {
            if !absorber.text.is_empty() {
                absorber.text.push('\n');
            }
            absorber.text.push_str(&text);
            return Ok(());
        }
        let canonical = normalize_prose(&text);
        let parent = self.section_parent();
        let heading_path = self.section_path();
        self.emit(
            parent,
            path,
            EmitContent {
                kind: SourceNodeKind::Paragraph,
                label: None,
                canonical_text: canonical,
                fragment: None,
                heading_path,
            },
        )?;
        Ok(())
    }

    /// Dispatch one block element. Exclusions prune first, then
    /// the closed-rule drop set, then the native structural
    /// mapping, then recipe classification selectors, then known
    /// containers, then the unsupported-element fallthrough.
    fn walk_element(&mut self, node: NodeRef<Node>, path: &[u32]) -> Result<(), HtmlIngestError> {
        let Some(el) = node.value().as_element() else {
            return Ok(());
        };
        let name = el.name();
        if let Some(selector) = self.matches_exclusion(node) {
            self.diagnose(
                HtmlIngestDiagnosticKind::ExcludedByRecipe {
                    selector: selector.to_string(),
                },
                path,
                format!("element <{name}> subtree excluded by recipe selector {selector:?}"),
            );
            return Ok(());
        }
        if is_dropped(name) {
            if super::adapter::carries_content(node) {
                self.diagnose(
                    HtmlIngestDiagnosticKind::DroppedByClosedRule {
                        tag: static_tag(name),
                    },
                    path,
                    format!("element <{name}> drops by the closed rule"),
                );
            }
            return Ok(());
        }
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = (name.as_bytes()[1] - b'0') as usize;
                self.heading(node, path, level)
            }
            "p" => self.paragraph(node, path),
            "ul" | "ol" | "dl" | "figure" => self.walk_children(node, path),
            "li" => self.list_item(node, path),
            "dt" | "dd" => self.definition(node, path, name == "dt"),
            "table" => self.table(node, path),
            "pre" => self.code_block(node, path),
            "blockquote" => self.absorbing_note(node, path),
            "figcaption" => self.figure_caption(node, path),
            _ => {
                if self.matches_note(node) {
                    self.absorbing_note(node, path)
                } else if self.matches_figure_caption(node) {
                    self.figure_caption(node, path)
                } else if is_inline(name) || is_container(name) {
                    self.walk_children(node, path)
                } else if skips_subtree(name) {
                    self.diagnose(
                        HtmlIngestDiagnosticKind::UnsupportedElement {
                            tag: name.to_string(),
                        },
                        path,
                        format!("element <{name}> is not represented; its subtree is skipped"),
                    );
                    Ok(())
                } else {
                    self.diagnose(
                        HtmlIngestDiagnosticKind::UnsupportedElement {
                            tag: name.to_string(),
                        },
                        path,
                        format!("element <{name}> is not represented; its children are retained"),
                    );
                    self.walk_children(node, path)
                }
            }
        }
    }

    /// A heading opens a section under the nearest lower-level
    /// section. Its fragment is the explicit `id` attribute when
    /// present (a repeated claim diagnoses) else the shared
    /// deterministic slugger.
    fn heading(
        &mut self,
        node: NodeRef<Node>,
        path: &[u32],
        level: usize,
    ) -> Result<(), HtmlIngestError> {
        while self.sections.last().is_some_and(|s| s.level >= level) {
            self.sections.pop();
        }
        let mut raw = String::new();
        self.collect_text(node, path, &mut raw, false)?;
        let label_text = normalize_prose(&raw);
        let fragment = match node.value().as_element().and_then(|el| el.attr("id")) {
            Some(id) if !id.trim().is_empty() => {
                if !self.used_anchors.insert(id.to_string()) {
                    self.diagnose(
                        HtmlIngestDiagnosticKind::DuplicateAnchor {
                            anchor: id.to_string(),
                        },
                        path,
                        format!("explicit id {id:?} is claimed more than once"),
                    );
                }
                id.to_string()
            }
            _ => anchors::dedup(&mut self.used_anchors, anchors::slugify(&label_text)),
        };
        let label = if label_text.is_empty() {
            None
        } else {
            Some(label_text)
        };
        let mut heading_path = self.section_path();
        if let Some(label) = &label {
            heading_path.push(label.clone());
        }
        let parent = self.section_parent();
        let provisional = self.emit(
            parent,
            path,
            EmitContent {
                kind: SourceNodeKind::Section,
                label: label.clone(),
                canonical_text: label.unwrap_or_default(),
                fragment: Some(fragment),
                heading_path: heading_path.clone(),
            },
        )?;
        self.sections.push(SectionEntry {
            level,
            provisional,
            path: heading_path,
        });
        Ok(())
    }

    /// A paragraph folds into the open absorber when one exists,
    /// else emits its own node at the enclosing section. Empty
    /// paragraphs carry no content and project no node.
    fn paragraph(&mut self, node: NodeRef<Node>, path: &[u32]) -> Result<(), HtmlIngestError> {
        let mut raw = String::new();
        self.collect_text(node, path, &mut raw, false)?;
        if raw.trim().is_empty() {
            return Ok(());
        }
        if let Some(absorber) = self.absorbers.last_mut() {
            if !absorber.text.is_empty() {
                absorber.text.push('\n');
            }
            absorber.text.push_str(&raw);
            return Ok(());
        }
        let canonical = normalize_prose(&raw);
        let parent = self.section_parent();
        let heading_path = self.section_path();
        self.emit(
            parent,
            path,
            EmitContent {
                kind: SourceNodeKind::Paragraph,
                label: None,
                canonical_text: canonical,
                fragment: None,
                heading_path,
            },
        )?;
        Ok(())
    }

    /// A list item attaches to the enclosing section in document
    /// order — the canonical nested-list projection: a nested
    /// list's items follow their parent item as siblings. Nested
    /// structural blocks project as their own sibling nodes.
    fn list_item(&mut self, node: NodeRef<Node>, path: &[u32]) -> Result<(), HtmlIngestError> {
        let mut raw = String::new();
        self.collect_text(node, path, &mut raw, false)?;
        let canonical = normalize_prose(&raw);
        let parent = self.section_parent();
        let heading_path = self.section_path();
        self.emit(
            parent,
            path,
            EmitContent {
                kind: SourceNodeKind::ListItem,
                label: None,
                canonical_text: canonical,
                fragment: None,
                heading_path,
            },
        )?;
        self.walk_nested_blocks(node, path)
    }

    /// A definition-list term or body. The term's normalized text
    /// is its label (the human identity of the term); the body
    /// carries no label. A duplicated term label fails closed at
    /// graph insertion, exactly like a duplicated section label.
    fn definition(
        &mut self,
        node: NodeRef<Node>,
        path: &[u32],
        is_term: bool,
    ) -> Result<(), HtmlIngestError> {
        let mut raw = String::new();
        self.collect_text(node, path, &mut raw, false)?;
        let canonical = normalize_prose(&raw);
        let label = if is_term && !canonical.is_empty() {
            Some(canonical.clone())
        } else {
            None
        };
        let kind = if is_term {
            SourceNodeKind::DefinitionTerm
        } else {
            SourceNodeKind::DefinitionBody
        };
        let parent = self.section_parent();
        let heading_path = self.section_path();
        self.emit(
            parent,
            path,
            EmitContent {
                kind,
                label,
                canonical_text: canonical,
                fragment: None,
                heading_path,
            },
        )?;
        self.walk_nested_blocks(node, path)
    }

    /// A `pre` block is a code-block node under the code
    /// normalization contract (significant whitespace preserved).
    fn code_block(&mut self, node: NodeRef<Node>, path: &[u32]) -> Result<(), HtmlIngestError> {
        let mut raw = String::new();
        self.collect_text(node, path, &mut raw, false)?;
        let canonical = normalize_code(&raw);
        let parent = self.section_parent();
        let heading_path = self.section_path();
        self.emit(
            parent,
            path,
            EmitContent {
                kind: SourceNodeKind::CodeBlock,
                label: None,
                canonical_text: canonical,
                fragment: None,
                heading_path,
            },
        )?;
        Ok(())
    }

    /// An absorbing note container (a `blockquote` or an element
    /// matched by the recipe's note/example classification
    /// selectors): enclosed paragraphs and stray text fold into
    /// the note; enclosed structural blocks project as their own
    /// section-level sibling nodes, exactly as the Markdown
    /// adapter projects blocks nested inside a quote.
    fn absorbing_note(&mut self, node: NodeRef<Node>, path: &[u32]) -> Result<(), HtmlIngestError> {
        let parent = self.section_parent();
        let frame = super::adapter::AbsorbFrame {
            provisional: self.provisional(),
            ordinal: self.next_ordinal(&parent),
            path: path.to_vec(),
            heading_path: self.section_path(),
            text: String::new(),
            parent,
        };
        self.absorbers.push(frame);
        self.walk_children(node, path)?;
        let Some(frame) = self.absorbers.pop() else {
            return Ok(());
        };
        let canonical = normalize_prose(&frame.text);
        self.emit_closed_note(frame, canonical)
    }

    /// A figure caption (native `figcaption` or an element matched
    /// by the recipe's figure-caption classification selectors):
    /// a leaf node folding its full descendant text.
    fn figure_caption(&mut self, node: NodeRef<Node>, path: &[u32]) -> Result<(), HtmlIngestError> {
        let mut raw = String::new();
        self.collect_text(node, path, &mut raw, true)?;
        let canonical = normalize_prose(&raw);
        let parent = self.section_parent();
        let heading_path = self.section_path();
        self.emit(
            parent,
            path,
            EmitContent {
                kind: SourceNodeKind::FigureCaption,
                label: None,
                canonical_text: canonical,
                fragment: None,
                heading_path,
            },
        )?;
        Ok(())
    }

    /// Walk the structural block children of a text-collecting
    /// container (list item, definition, table cell) whose own
    /// text was already collected: nested lists, tables, figures,
    /// code, notes, and headings project as section-level sibling
    /// nodes in document order.
    pub(super) fn walk_nested_blocks(
        &mut self,
        node: NodeRef<Node>,
        path: &[u32],
    ) -> Result<(), HtmlIngestError> {
        let mut element_index = 0u32;
        for child in node.children() {
            if child.value().as_element().is_none() {
                continue;
            }
            let mut child_path = path.to_vec();
            child_path.push(element_index);
            element_index += 1;
            let Some(el) = child.value().as_element() else {
                continue;
            };
            if NESTED_BLOCK_TAGS.contains(&el.name())
                || self.matches_note(child)
                || self.matches_figure_caption(child)
            {
                self.walk_element(child, &child_path)?;
            }
        }
        Ok(())
    }
}

/// The `'static` tag name of a closed-rule drop; every member of
/// the drop set is a string literal already.
pub(super) fn static_tag(name: &str) -> &'static str {
    match name {
        "script" => "script",
        "style" => "style",
        "template" => "template",
        "head" => "head",
        "title" => "title",
        "meta" => "meta",
        "link" => "link",
        "colgroup" => "colgroup",
        "col" => "col",
        _ => "noscript",
    }
}
