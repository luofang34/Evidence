//! The bounded tree walk of the bbox-layout parser (LLR-181):
//! structure validation and the per-element bounds, split from
//! `bbox.rs` under the 500-line module limit.

use super::{
    Bbox, BboxBlock, BboxDocument, BboxLine, BboxPage, BboxParseError, BboxWord,
    MAX_ATTRIBUTES_PER_ELEMENT, MAX_BLOCKS_PER_PAGE, MAX_COORDINATE, MAX_DEPTH, MAX_ELEMENTS,
    MAX_LINES_PER_BLOCK, MAX_PAGES, MAX_WORDS_PER_LINE,
};

/// Running totals enforcing the global bounds.
#[derive(Default)]
pub(super) struct ParseState {
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
pub(super) fn build_document(
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
