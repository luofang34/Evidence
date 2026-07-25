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
    let mut state = walk::ParseState::default();
    walk::build_document(&document, &mut state)
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

mod walk;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "bbox_tests.rs"]
mod tests;
