//! The bounded, fail-closed `pdftotext -bbox-layout` XHTML parser
//! (LLR-181).
//!
//! [`parse_bbox_layout`] decodes the extractor output strictly as
//! UTF-8, strips one leading byte-order mark and the exact pinned
//! Poppler DOCTYPE declaration
//! ([`PINNED_DOCTYPE`]), and parses through `roxmltree`, which
//! never expands DTDs or external entities by construction. Any
//! `<!ENTITY` declaration and any other `<!DOCTYPE` fails closed
//! before parsing.
//!
//! Only the documented Poppler `-bbox-layout` structure is
//! accepted: `html` (with an empty `head`) → `body` → `doc` →
//! `page` (positive finite `width`/`height`) → `flow` → `block` →
//! `line` → `word` (text plus `xMin`/`yMin`/`xMax`/`yMax`).
//! Unknown structural elements, missing page dimensions,
//! non-finite, negative, reversed, or out-of-page coordinates,
//! and out-of-bounds counts each produce a typed
//! [`BboxParseError`] carrying the 1-based page index and the
//! element path. Depth, element count, attribute count, text
//! bytes, pages, blocks, lines, words, and coordinate magnitude
//! are bounded by explicit constants.

use thiserror::Error;

/// The exact DOCTYPE declaration Poppler emits; it is stripped
/// before parsing and no other DOCTYPE is accepted (LLR-181).
pub const PINNED_DOCTYPE: &str = "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\" \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\">";

/// Maximum element nesting depth, counting the document root:
/// root→html→body→doc→page→flow→block→line→word is 9.
pub const MAX_DEPTH: usize = 10;
/// Maximum total element count.
pub const MAX_ELEMENTS: usize = 65_536;
/// Maximum attributes on one element.
pub const MAX_ATTRIBUTES_PER_ELEMENT: usize = 8;
/// Maximum decoded text bytes.
pub const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;
/// Maximum page count.
pub const MAX_PAGES: usize = 1024;
/// Maximum blocks per page.
pub const MAX_BLOCKS_PER_PAGE: usize = 1024;
/// Maximum lines per block.
pub const MAX_LINES_PER_BLOCK: usize = 1024;
/// Maximum words per line.
pub const MAX_WORDS_PER_LINE: usize = 256;
/// Maximum absolute coordinate magnitude (points).
pub const MAX_COORDINATE: f64 = 1_000_000.0;

/// The parsed bounding box of one element: `[xMin, yMin, xMax,
/// yMax]` in points from the page's top-left corner.
pub type Bbox = [f64; 4];

/// One word with its bounding box (LLR-181).
#[derive(Debug, Clone, PartialEq)]
pub struct BboxWord {
    /// The word's bounding box.
    pub bbox: Bbox,
    /// The word's exact text.
    pub text: String,
}

/// One line of words (LLR-181).
#[derive(Debug, Clone, PartialEq)]
pub struct BboxLine {
    /// The line's bounding box.
    pub bbox: Bbox,
    /// The line's words, in extractor order.
    pub words: Vec<BboxWord>,
}

/// One block of lines (LLR-181).
#[derive(Debug, Clone, PartialEq)]
pub struct BboxBlock {
    /// The block's bounding box.
    pub bbox: Bbox,
    /// The block's lines, in extractor order.
    pub lines: Vec<BboxLine>,
}

/// One page with its dimensions and blocks (LLR-181).
#[derive(Debug, Clone, PartialEq)]
pub struct BboxPage {
    /// Page width in points.
    pub width: f64,
    /// Page height in points.
    pub height: f64,
    /// The page's blocks across all flows, in extractor order.
    pub blocks: Vec<BboxBlock>,
}

/// The parsed bbox-layout document (LLR-181).
#[derive(Debug, Clone, PartialEq)]
pub struct BboxDocument {
    /// The pages, in document order; never empty.
    pub pages: Vec<BboxPage>,
}

/// Every fail-closed bbox-parse violation (LLR-181). Variants
/// carry the 1-based page index (`page`, 0 when the failure
/// precedes any page) and the element path where the violation
/// was found.
#[derive(Debug, Error)]
pub enum BboxParseError {
    /// The extractor output is not valid UTF-8.
    #[error(
        "extractor output is not valid UTF-8; first invalid sequence starts at byte offset {offset}"
    )]
    NonUtf8 {
        /// Byte offset of the first invalid sequence.
        offset: usize,
    },
    /// An `<!ENTITY` declaration appears anywhere in the output.
    #[error("an <!ENTITY declaration is rejected: entity resolution is disabled")]
    EntityDeclaration,
    /// A `<!DOCTYPE` other than the pinned Poppler declaration
    /// appears.
    #[error("a <!DOCTYPE other than the pinned Poppler declaration is rejected")]
    DoctypeRejected,
    /// The XML is malformed.
    #[error("extractor output is malformed XML: {detail}")]
    MalformedXml {
        /// The parser's error rendering.
        detail: String,
    },
    /// A bound was exceeded.
    #[error("bound {what} exceeded at {path} (page {page}): {actual} > {limit}")]
    BoundExceeded {
        /// The bounded quantity: `depth`, `elements`,
        /// `attributes`, `text-bytes`, `pages`, `blocks`, `lines`,
        /// or `words`.
        what: &'static str,
        /// The observed value.
        actual: usize,
        /// The configured bound.
        limit: usize,
        /// The 1-based page index, 0 before any page.
        page: u32,
        /// The element path.
        path: String,
    },
    /// An element outside the documented structure appeared.
    #[error("unknown structural element <{element}> at {path} (page {page})")]
    UnknownElement {
        /// The element name.
        element: String,
        /// The 1-based page index, 0 before any page.
        page: u32,
        /// The element path.
        path: String,
    },
    /// A page is missing or carries malformed dimensions.
    #[error("page {page} is missing positive finite width/height dimensions")]
    MissingDimensions {
        /// The 1-based page index.
        page: u32,
    },
    /// A coordinate is non-finite, negative, reversed, beyond the
    /// magnitude bound, or outside the page extent.
    #[error("invalid coordinate on <{element}> at {path} (page {page}): {detail}")]
    InvalidCoordinate {
        /// The element carrying the coordinate.
        element: &'static str,
        /// The 1-based page index.
        page: u32,
        /// The element path.
        path: String,
        /// What was wrong with the value.
        detail: String,
    },
    /// The document contains no pages.
    #[error("the document contains no pages")]
    EmptyDocument,
}

/// Parse the extractor's bbox-layout output into the bounded
/// document model (LLR-181). Pure and host-independent.
///
/// # Errors
///
/// Fails closed on any contract, structure, or bound violation;
/// the first violation wins, so error precedence is
/// deterministic.
pub fn parse_bbox_layout(bytes: &[u8]) -> Result<BboxDocument, BboxParseError> {
    let text = std::str::from_utf8(bytes).map_err(|err| BboxParseError::NonUtf8 {
        offset: err.valid_up_to(),
    })?;
    // The documented BOM rule: one leading U+FEFF strips.
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    if text.len() > MAX_TEXT_BYTES {
        return Err(BboxParseError::BoundExceeded {
            what: "text-bytes",
            actual: text.len(),
            limit: MAX_TEXT_BYTES,
            page: 0,
            path: String::new(),
        });
    }
    let text = strip_pinned_doctype(text)?;
    let document =
        roxmltree::Document::parse(text).map_err(|err| BboxParseError::MalformedXml {
            detail: err.to_string(),
        })?;
    let mut state = ParseState::default();
    build_document(&document, &mut state)
}

/// Strip the exact pinned Poppler DOCTYPE; reject any other
/// DOCTYPE and any ENTITY declaration.
fn strip_pinned_doctype(text: &str) -> Result<&str, BboxParseError> {
    fn declares(haystack: &str, needle: &str) -> bool {
        haystack
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
    }
    if declares(text, "<!entity") {
        return Err(BboxParseError::EntityDeclaration);
    }
    let rest = text.strip_prefix(PINNED_DOCTYPE).unwrap_or(text);
    if declares(rest, "<!doctype") {
        return Err(BboxParseError::DoctypeRejected);
    }
    Ok(rest)
}

/// Running totals enforcing the global bounds.
#[derive(Default)]
struct ParseState {
    elements: usize,
}

impl ParseState {
    /// Count one element, enforcing the global element bound, the
    /// depth bound, and the per-element attribute bound.
    fn enter(
        &mut self,
        node: &roxmltree::Node,
        path: &str,
        page: u32,
    ) -> Result<(), BboxParseError> {
        self.elements += 1;
        if self.elements > MAX_ELEMENTS {
            return Err(BboxParseError::BoundExceeded {
                what: "elements",
                actual: self.elements,
                limit: MAX_ELEMENTS,
                page,
                path: path.to_string(),
            });
        }
        let depth = node.ancestors().count();
        if depth > MAX_DEPTH {
            return Err(BboxParseError::BoundExceeded {
                what: "depth",
                actual: depth,
                limit: MAX_DEPTH,
                page,
                path: path.to_string(),
            });
        }
        let attributes = node.attributes().count();
        if attributes > MAX_ATTRIBUTES_PER_ELEMENT {
            return Err(BboxParseError::BoundExceeded {
                what: "attributes",
                actual: attributes,
                limit: MAX_ATTRIBUTES_PER_ELEMENT,
                page,
                path: path.to_string(),
            });
        }
        Ok(())
    }
}

/// Walk the parsed tree into the bounded document model.
fn build_document(
    document: &roxmltree::Document,
    state: &mut ParseState,
) -> Result<BboxDocument, BboxParseError> {
    let root = document.root_element();
    expect_element(&root, "html", "html", 0)?;
    state.enter(&root, "html", 0)?;
    let mut pages = Vec::new();
    for child in root.children().filter(roxmltree::Node::is_element) {
        let path = format!("html/{}", child.tag_name().name());
        state.enter(&child, &path, 0)?;
        match child.tag_name().name() {
            "head" => {
                if child.children().any(|node| node.is_element()) {
                    return Err(unknown(&child, &path, 0));
                }
            }
            "body" => {
                for body_child in child.children().filter(roxmltree::Node::is_element) {
                    let doc_path = format!("{path}/{}", body_child.tag_name().name());
                    state.enter(&body_child, &doc_path, 0)?;
                    if body_child.tag_name().name() != "doc" {
                        return Err(unknown(&body_child, &doc_path, 0));
                    }
                    build_pages(&body_child, state, &doc_path, &mut pages)?;
                }
            }
            _ => return Err(unknown(&child, &path, 0)),
        }
    }
    if pages.is_empty() {
        return Err(BboxParseError::EmptyDocument);
    }
    Ok(BboxDocument { pages })
}

/// Collect the pages of one `doc` element in document order.
fn build_pages(
    doc: &roxmltree::Node,
    state: &mut ParseState,
    path: &str,
    pages: &mut Vec<BboxPage>,
) -> Result<(), BboxParseError> {
    for page_node in doc.children().filter(roxmltree::Node::is_element) {
        let page_index = pages.len() as u32 + 1;
        let page_path = format!("{path}/page[{page_index}]");
        state.enter(&page_node, &page_path, page_index)?;
        if page_node.tag_name().name() != "page" {
            return Err(unknown(&page_node, &page_path, page_index));
        }
        if pages.len() >= MAX_PAGES {
            return Err(BboxParseError::BoundExceeded {
                what: "pages",
                actual: pages.len() + 1,
                limit: MAX_PAGES,
                page: page_index,
                path: page_path,
            });
        }
        let width = dimension(&page_node, "width", page_index)?;
        let height = dimension(&page_node, "height", page_index)?;
        let mut blocks = Vec::new();
        for flow in page_node.children().filter(roxmltree::Node::is_element) {
            let flow_path = format!("{page_path}/flow");
            state.enter(&flow, &flow_path, page_index)?;
            if flow.tag_name().name() != "flow" {
                return Err(unknown(&flow, &flow_path, page_index));
            }
            build_blocks(
                &flow,
                state,
                &flow_path,
                page_index,
                width,
                height,
                &mut blocks,
            )?;
        }
        pages.push(BboxPage {
            width,
            height,
            blocks,
        });
    }
    Ok(())
}

/// Collect the blocks of one `flow` element.
#[allow(
    clippy::too_many_arguments,
    reason = "the walk threads page context through"
)]
fn build_blocks(
    flow: &roxmltree::Node,
    state: &mut ParseState,
    flow_path: &str,
    page: u32,
    width: f64,
    height: f64,
    blocks: &mut Vec<BboxBlock>,
) -> Result<(), BboxParseError> {
    for block_node in flow.children().filter(roxmltree::Node::is_element) {
        let path = format!("{flow_path}/block");
        state.enter(&block_node, &path, page)?;
        if block_node.tag_name().name() != "block" {
            return Err(unknown(&block_node, &path, page));
        }
        if blocks.len() >= MAX_BLOCKS_PER_PAGE {
            return Err(BboxParseError::BoundExceeded {
                what: "blocks",
                actual: blocks.len() + 1,
                limit: MAX_BLOCKS_PER_PAGE,
                page,
                path,
            });
        }
        let bbox = coordinates(&block_node, "block", &path, page, width, height)?;
        let mut lines = Vec::new();
        for line_node in block_node.children().filter(roxmltree::Node::is_element) {
            let line_path = format!("{path}/line");
            state.enter(&line_node, &line_path, page)?;
            if line_node.tag_name().name() != "line" {
                return Err(unknown(&line_node, &line_path, page));
            }
            if lines.len() >= MAX_LINES_PER_BLOCK {
                return Err(BboxParseError::BoundExceeded {
                    what: "lines",
                    actual: lines.len() + 1,
                    limit: MAX_LINES_PER_BLOCK,
                    page,
                    path: line_path,
                });
            }
            let line_bbox = coordinates(&line_node, "line", &line_path, page, width, height)?;
            let mut words = Vec::new();
            for word_node in line_node.children().filter(roxmltree::Node::is_element) {
                let word_path = format!("{line_path}/word");
                state.enter(&word_node, &word_path, page)?;
                if word_node.tag_name().name() != "word" {
                    return Err(unknown(&word_node, &word_path, page));
                }
                if let Some(child) = word_node.children().find(|node| node.is_element()) {
                    let child_path = format!("{word_path}/{}", child.tag_name().name());
                    state.enter(&child, &child_path, page)?;
                    return Err(unknown(&child, &child_path, page));
                }
                if words.len() >= MAX_WORDS_PER_LINE {
                    return Err(BboxParseError::BoundExceeded {
                        what: "words",
                        actual: words.len() + 1,
                        limit: MAX_WORDS_PER_LINE,
                        page,
                        path: word_path,
                    });
                }
                let word_bbox = coordinates(&word_node, "word", &word_path, page, width, height)?;
                let text = word_node.text().unwrap_or("").to_string();
                words.push(BboxWord {
                    bbox: word_bbox,
                    text,
                });
            }
            lines.push(BboxLine {
                bbox: line_bbox,
                words,
            });
        }
        blocks.push(BboxBlock { bbox, lines });
    }
    Ok(())
}

/// A required element name check at the structure's root.
fn expect_element(
    node: &roxmltree::Node,
    name: &str,
    path: &str,
    page: u32,
) -> Result<(), BboxParseError> {
    if node.tag_name().name() != name {
        return Err(unknown(node, path, page));
    }
    Ok(())
}

/// One unknown-element error carrying the element name.
fn unknown(node: &roxmltree::Node, path: &str, page: u32) -> BboxParseError {
    BboxParseError::UnknownElement {
        element: node.tag_name().name().to_string(),
        page,
        path: path.to_string(),
    }
}

/// A page dimension: present, finite, positive, and within the
/// coordinate magnitude bound.
fn dimension(node: &roxmltree::Node, attr: &str, page: u32) -> Result<f64, BboxParseError> {
    let value = node
        .attribute(attr)
        .and_then(|raw| raw.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0 && *value <= MAX_COORDINATE);
    value.ok_or(BboxParseError::MissingDimensions { page })
}

/// One element bounding box: finite, non-negative, ordered,
/// within the magnitude bound, and inside the page extent.
fn coordinates(
    node: &roxmltree::Node,
    element: &'static str,
    path: &str,
    page: u32,
    width: f64,
    height: f64,
) -> Result<Bbox, BboxParseError> {
    let invalid = |detail: &str| BboxParseError::InvalidCoordinate {
        element,
        page,
        path: path.to_string(),
        detail: detail.to_string(),
    };
    let mut bbox = [0.0; 4];
    for (index, attr) in ["xMin", "yMin", "xMax", "yMax"].iter().enumerate() {
        let value = node
            .attribute(*attr)
            .and_then(|raw| raw.parse::<f64>().ok())
            .ok_or_else(|| invalid(&format!("{attr} is missing or not a number")))?;
        if !value.is_finite() {
            return Err(invalid(&format!("{attr} is not finite")));
        }
        if value < 0.0 {
            return Err(invalid(&format!("{attr} is negative")));
        }
        if value > MAX_COORDINATE {
            return Err(invalid(&format!("{attr} exceeds the magnitude bound")));
        }
        bbox[index] = value;
    }
    if bbox[0] > bbox[2] || bbox[1] > bbox[3] {
        return Err(invalid("the corners are reversed on an axis"));
    }
    if bbox[2] > width || bbox[3] > height {
        return Err(invalid("the box lies outside the page extent"));
    }
    Ok(bbox)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "bbox_tests.rs"]
mod tests;
