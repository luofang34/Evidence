//! The DOM-walking adapter state (LLR-165).
//!
//! [`Adapter`] walks the parsed DOM from the walk root — the
//! document root, or the first element matching the recipe's
//! inclusion root selector — and produces candidate nodes plus
//! typed structural-loss diagnostics. The walk is a depth-first
//! document-order traversal; node creation happens as elements
//! close (sections, notes) or immediately (all other kinds), and
//! the candidate sort in the assembly restores document order by
//! provisional sequence, exactly as the Markdown adapter does.
//!
//! # Bounds and pre-pass
//!
//! Before walking, an iterative pre-pass over the retained DOM
//! (excluded subtrees pruned) enforces the nesting-depth and
//! per-element attribute bounds and collects the document id set —
//! every `id` attribute plus every `<a name>` — that internal
//! fragment links resolve against. The candidate-count and
//! per-node text bounds enforce at emission. Every bound
//! violation fails closed with a typed [`HtmlIngestError`]
//! carrying the observed value and the limit.

use std::collections::{BTreeMap, BTreeSet};

use ego_tree::NodeRef;
use scraper::node::Node;
use scraper::{ElementRef, Selector};

use super::IngestHtmlInput;
use super::error::{HtmlIngestDiagnostic, HtmlIngestDiagnosticKind, HtmlIngestError};
use crate::corpus::CandidateNode;

/// The DOM element-nesting bound below the walk root.
pub(super) const MAX_DEPTH: usize = 256;
/// The candidate node-count bound.
pub(super) const MAX_NODES: usize = 65_536;
/// The per-element attribute-count bound.
pub(super) const MAX_ATTRIBUTES: usize = 128;
/// The per-node canonical-text size bound in bytes.
pub(super) const MAX_TEXT_BYTES: usize = 1024 * 1024;

/// Tags dropped by the closed rule — script, style, template, and
/// non-content metadata — each producing one typed diagnostic when
/// it carries content (element children or non-whitespace text).
/// An empty drop-set element — including the parser-synthesized
/// empty `head` — loses nothing and drops without a diagnostic.
pub(super) const DROP_TAGS: [&str; 10] = [
    "script", "style", "template", "head", "title", "meta", "link", "noscript", "colgroup", "col",
];

/// Inline formatting tags: transparent, their text folding into
/// the enclosing block.
pub(super) const INLINE_TAGS: [&str; 29] = [
    "a", "abbr", "b", "bdi", "bdo", "br", "cite", "code", "data", "dfn", "em", "i", "kbd", "mark",
    "q", "rp", "rt", "ruby", "s", "samp", "small", "span", "strong", "sub", "sup", "time", "u",
    "var", "wbr",
];

/// Known transparent container tags. `nav`, `header`, and `footer`
/// are NOT dropped here: navigation, repeated ToC, header, and
/// footer removal is recipe-selector driven, never heuristic.
const CONTAINER_TAGS: [&str; 11] = [
    "html", "body", "div", "section", "article", "main", "nav", "header", "footer", "aside",
    "hgroup",
];

/// Structural block tags that emit their own nodes when nested
/// inside a text-collecting context (list item, definition, table
/// cell): the text collector skips them and the nested-block walk
/// projects them as section-level siblings in document order.
pub(super) const NESTED_BLOCK_TAGS: [&str; 13] = [
    "ul",
    "ol",
    "dl",
    "table",
    "figure",
    "pre",
    "blockquote",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
];

/// Foreign and embedded content whose subtree the projection
/// skips after one unsupported-element diagnostic.
const SKIP_SUBTREE_TAGS: [&str; 8] = [
    "svg", "math", "iframe", "object", "embed", "audio", "video", "canvas",
];

/// One logical section in the heading trail.
#[derive(Debug)]
pub(super) struct SectionEntry {
    pub(super) level: usize,
    pub(super) provisional: String,
    /// Ancestor section labels plus this section's own label.
    pub(super) path: Vec<String>,
}

/// One open absorbing frame (a note/example container): enclosed
/// paragraphs and stray text fold into `text`; enclosed structural
/// blocks project as their own section-level sibling nodes.
#[derive(Debug)]
pub(super) struct AbsorbFrame {
    pub(super) provisional: String,
    pub(super) parent: Option<String>,
    pub(super) ordinal: u32,
    pub(super) path: Vec<u32>,
    pub(super) heading_path: Vec<String>,
    pub(super) text: String,
}

/// The adapter state for one parse.
#[derive(Debug)]
pub(super) struct Adapter<'i> {
    pub(super) input: &'i IngestHtmlInput<'i>,
    /// Compiled exclusion selectors in sorted (recipe) order.
    pub(super) exclusions: Vec<(String, Selector)>,
    /// Compiled note/example classification selectors, sorted.
    pub(super) notes: Vec<Selector>,
    /// Compiled figure-caption classification selectors, sorted.
    pub(super) figures: Vec<Selector>,
    /// The compiled inclusion root selector, when the recipe
    /// declares one.
    pub(super) inclusion: Option<(String, Selector)>,
    pub(super) sections: Vec<SectionEntry>,
    pub(super) absorbers: Vec<AbsorbFrame>,
    pub(super) candidates: Vec<CandidateNode>,
    pub(super) diagnostics: Vec<HtmlIngestDiagnostic>,
    pub(super) ordinals: BTreeMap<Option<String>, u32>,
    pub(super) used_anchors: BTreeSet<String>,
    /// The retained document's id set (every `id` and `<a name>`).
    pub(super) ids: BTreeSet<String>,
    /// Recorded pure-fragment internal links: `(dom_path, fragment)`.
    pub(super) links: Vec<(Vec<u32>, String)>,
    /// Running count of header rows, feeding their unique labels.
    pub(super) header_rows: u32,
    pub(super) next_provisional: u64,
}

/// Compile one recipe selector, failing closed on a parse error.
fn compile(selector: &str) -> Result<Selector, HtmlIngestError> {
    Selector::parse(selector).map_err(|err| HtmlIngestError::InvalidSelector {
        selector: selector.to_string(),
        detail: format!("{err:?}"),
    })
}

impl<'i> Adapter<'i> {
    /// Build the adapter state, compiling every recipe selector.
    /// Selector syntax errors fail closed before any parsing.
    pub(super) fn new(input: &'i IngestHtmlInput<'i>) -> Result<Self, HtmlIngestError> {
        let recipe = &input.recipe;
        let compile_set = |set: &BTreeSet<String>| -> Result<Vec<Selector>, HtmlIngestError> {
            set.iter().map(|s| compile(s)).collect()
        };
        let exclusions = recipe
            .exclusion_selectors
            .iter()
            .map(|s| compile(s).map(|c| (s.clone(), c)))
            .collect::<Result<Vec<_>, _>>()?;
        let inclusion = recipe
            .inclusion_root
            .as_ref()
            .map(|s| compile(s).map(|c| (s.clone(), c)))
            .transpose()?;
        Ok(Self {
            input,
            exclusions,
            notes: compile_set(&recipe.note_selectors)?,
            figures: compile_set(&recipe.figure_caption_selectors)?,
            inclusion,
            sections: Vec::new(),
            absorbers: Vec::new(),
            candidates: Vec::new(),
            diagnostics: Vec::new(),
            ordinals: BTreeMap::new(),
            used_anchors: BTreeSet::new(),
            ids: BTreeSet::new(),
            links: Vec::new(),
            header_rows: 0,
            next_provisional: 0,
        })
    }

    /// Locate the walk root: the first element matching the
    /// inclusion root selector in document order, or the document
    /// root when the recipe declares none.
    pub(super) fn find_root<'d>(
        &self,
        document: &'d scraper::Html,
    ) -> Result<NodeRef<'d, Node>, HtmlIngestError> {
        if let Some((source, selector)) = &self.inclusion {
            for node in document.tree.root().descendants() {
                if let Some(el) = ElementRef::wrap(node) {
                    if selector.matches(&el) {
                        return Ok(node);
                    }
                }
            }
            return Err(HtmlIngestError::InclusionRootNotFound {
                selector: source.clone(),
            });
        }
        Ok(document.tree.root())
    }

    /// The DOM path of `node`: element-child indexes from the
    /// document root. Pure and stable for a fixed DOM.
    pub(super) fn dom_path_of(node: NodeRef<Node>) -> Vec<u32> {
        let mut path = Vec::new();
        let mut current = node;
        while let Some(parent) = current.parent() {
            path.push(element_index(current));
            current = parent;
        }
        path.reverse();
        path
    }

    /// The iterative pre-pass pinned by the module docs: depth and
    /// attribute bounds plus the retained id set.
    pub(super) fn prepare(&mut self, root: NodeRef<Node>) -> Result<(), HtmlIngestError> {
        let mut stack: Vec<(NodeRef<Node>, usize)> = vec![(root, 0)];
        while let Some((node, depth)) = stack.pop() {
            for child in node.children() {
                let Some(el) = child.value().as_element() else {
                    continue;
                };
                let next = depth + 1;
                if next > MAX_DEPTH {
                    return Err(HtmlIngestError::NestingTooDeep {
                        depth: next,
                        limit: MAX_DEPTH,
                    });
                }
                let attributes = el.attrs().count();
                if attributes > MAX_ATTRIBUTES {
                    return Err(HtmlIngestError::TooManyAttributes {
                        tag: el.name().to_string(),
                        count: attributes,
                        limit: MAX_ATTRIBUTES,
                    });
                }
                if self.matches_exclusion(child).is_some() {
                    continue;
                }
                if let Some(id) = el.attr("id") {
                    if !id.trim().is_empty() {
                        self.ids.insert(id.to_string());
                    }
                }
                if el.name() == "a" {
                    if let Some(name) = el.attr("name") {
                        if !name.trim().is_empty() {
                            self.ids.insert(name.to_string());
                        }
                    }
                }
                stack.push((child, next));
            }
        }
        Ok(())
    }

    /// The first matching exclusion selector in sorted order, when
    /// the element matches any.
    pub(super) fn matches_exclusion(&self, node: NodeRef<Node>) -> Option<&str> {
        let el = ElementRef::wrap(node)?;
        self.exclusions
            .iter()
            .find(|(_, selector)| selector.matches(&el))
            .map(|(source, _)| source.as_str())
    }

    /// Whether the element matches a note/example classification
    /// selector.
    pub(super) fn matches_note(&self, node: NodeRef<Node>) -> bool {
        match ElementRef::wrap(node) {
            Some(el) => self.notes.iter().any(|s| s.matches(&el)),
            None => false,
        }
    }

    /// Whether the element matches a figure-caption classification
    /// selector.
    pub(super) fn matches_figure_caption(&self, node: NodeRef<Node>) -> bool {
        match ElementRef::wrap(node) {
            Some(el) => self.figures.iter().any(|s| s.matches(&el)),
            None => false,
        }
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

    /// Record one typed structural-loss diagnostic.
    pub(super) fn diagnose(
        &mut self,
        kind: HtmlIngestDiagnosticKind,
        path: &[u32],
        detail: String,
    ) {
        self.diagnostics.push(HtmlIngestDiagnostic {
            kind,
            dom_path: path.to_vec(),
            detail,
        });
    }

    /// Record a pure-fragment internal link target for the
    /// post-walk resolution check.
    pub(super) fn record_link(&mut self, el: &scraper::node::Element, path: &[u32]) {
        if let Some(href) = el.attr("href") {
            if let Some(fragment) = href.strip_prefix('#') {
                if !fragment.is_empty() {
                    self.links.push((path.to_vec(), fragment.to_string()));
                }
            }
        }
    }

    /// Check every recorded internal link against the retained id
    /// set, diagnosing danglers. Runs after the walk.
    pub(super) fn resolve_links(&mut self) {
        let links = std::mem::take(&mut self.links);
        for (path, fragment) in links {
            if !self.ids.contains(&fragment) {
                self.diagnose(
                    HtmlIngestDiagnosticKind::DanglingInternalLink {
                        fragment: fragment.clone(),
                    },
                    &path,
                    format!(
                        "internal link target #{fragment} is absent from the retained document"
                    ),
                );
            }
        }
    }
}

/// The index of `node` among its parent's element children —
/// one DOM-path component.
fn element_index(node: NodeRef<Node>) -> u32 {
    node.prev_siblings()
        .filter(|sibling| sibling.value().as_element().is_some())
        .count() as u32
}

/// Whether `name` is a known transparent container tag.
pub(super) fn is_container(name: &str) -> bool {
    CONTAINER_TAGS.contains(&name)
}

/// Whether `name` is an inline formatting tag.
pub(super) fn is_inline(name: &str) -> bool {
    INLINE_TAGS.contains(&name)
}

/// Whether `name` drops by the closed rule.
pub(super) fn is_dropped(name: &str) -> bool {
    DROP_TAGS.contains(&name)
}

/// Whether a dropped element carries content — element children or
/// non-whitespace text — so its drop produces a diagnostic.
pub(super) fn carries_content(node: NodeRef<Node>) -> bool {
    node.children().any(|child| {
        child.value().as_element().is_some()
            || child
                .value()
                .as_text()
                .is_some_and(|text| !text.text.trim().is_empty())
    })
}

/// Whether `name` is foreign or embedded content whose subtree is
/// skipped after one diagnostic.
pub(super) fn skips_subtree(name: &str) -> bool {
    SKIP_SUBTREE_TAGS.contains(&name)
}
