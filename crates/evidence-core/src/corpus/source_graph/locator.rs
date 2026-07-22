//! Closed per-format locator variants for structural source nodes
//! (LLR-153).
//!
//! Every committed source node carries exactly one typed locator
//! pinning where in its source medium the node was found. The
//! locator is diagnostic only: page, DOM, byte, and line positions
//! locate content for a human reader and never enter permanent
//! identity, content digests, or structural fingerprints.
//!
//! [`SourceLocator`] is an internally tagged (`format`),
//! snake_case, unknown-field-denying enum with exactly three
//! variants, so a mixed-format field combination — a DOM path on a
//! Markdown locator, a bounding box on an HTML locator — fails
//! deserialization rather than loading as a valid record. Fields
//! each variant carries:
//!
//! - `markdown` — `path` (a [`SafeRelPath`]), optional `git_blob`
//!   (40- or 64-character lowercase hex), optional `anchor`
//!   (non-blank), `heading_path` (each component non-blank), and
//!   `byte_range` (ordered bounds).
//! - `html` — `canonical_url` (absolute `<scheme>://` shape),
//!   optional `final_url` (same shape), optional `fragment`
//!   (non-blank), `heading_path`, and `dom_path` (child indexes
//!   from the document root; empty locates the root itself).
//! - `pdf` — `physical_page` (1-based), optional `printed_label`
//!   (non-blank), and `bbox` (four finite, non-negative,
//!   lower-left/upper-right ordered coordinates).
//!
//! URLs are opaque audit identity: their lexical shape is checked,
//! and they are never fetched, resolved, or normalized. Per-variant
//! validation failures return typed [`LocatorRule`] values that the
//! record loader wraps in
//! [`SourceGraphError::InvalidLocatorField`](super::error::SourceGraphError::InvalidLocatorField)
//! with the file path and node uid.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::super::source::error::VendoredPathRule;
use super::super::source::records::validate_vendored_wire_path;

/// A validated `/`-separated relative path in canonical wire form
/// (LLR-153).
///
/// The lexical rules are the vendored wire-path rules shared with
/// the source-capture domain: empty paths, absolute paths, drive or
/// UNC prefixes, backslashes, and empty, `.`, or `..` components
/// are rejected at every construction boundary — explicit
/// [`SafeRelPath::new`] and serde deserialization. The check is
/// lexical only; filesystem containment is the acquisition layer's
/// concern, not this type's.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SafeRelPath(String);

impl SafeRelPath {
    /// Validate `path` against the canonical relative wire-path
    /// rules and wrap it.
    ///
    /// # Errors
    ///
    /// Returns the [`VendoredPathRule`] the path violated when it
    /// is empty, absolute, drive- or UNC-prefixed,
    /// backslash-bearing, or carries an empty, `.`, or `..`
    /// component.
    pub fn new(path: &str) -> Result<Self, VendoredPathRule> {
        validate_vendored_wire_path(path)?;
        Ok(Self(path.to_string()))
    }

    /// The canonical wire-form path string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SafeRelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for SafeRelPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SafeRelPath {
    /// Deserialize through the validating constructor so an unsafe
    /// stored path fails closed instead of round-tripping.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::new(&raw).map_err(serde::de::Error::custom)
    }
}

/// The per-variant rule a locator field violated (LLR-153). Pure
/// data: the record loader carries it beside the file path, the
/// node uid, the field name, and the offending value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocatorRule {
    /// A byte range's lower bound exceeds its upper bound.
    ByteRangeReversed,
    /// A bounding-box coordinate is NaN or infinite.
    BboxNonFinite,
    /// A bounding-box coordinate is negative.
    BboxNegative,
    /// A bounding box's lower-left corner exceeds its upper-right
    /// corner on an axis.
    BboxReversed,
    /// A physical page number is zero; pages are 1-based.
    PageZero,
    /// A string that must carry content is blank.
    Blank,
    /// A URL lacks the absolute `<scheme>://` shape.
    UrlScheme,
    /// A git blob identifier is not 40- or 64-character lowercase
    /// hexadecimal.
    GitBlobHex,
}

impl fmt::Display for LocatorRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rule = match self {
            LocatorRule::ByteRangeReversed => "byte range bounds are reversed",
            LocatorRule::BboxNonFinite => "bounding-box coordinate is not finite",
            LocatorRule::BboxNegative => "bounding-box coordinate is negative",
            LocatorRule::BboxReversed => "bounding-box corners are reversed on an axis",
            LocatorRule::PageZero => "physical page is zero; pages are 1-based",
            LocatorRule::Blank => "value is blank",
            LocatorRule::UrlScheme => "URL lacks an absolute <scheme>:// shape",
            LocatorRule::GitBlobHex => {
                "git blob identifier is not 40- or 64-character lowercase hexadecimal"
            }
        };
        f.write_str(rule)
    }
}

/// One invalid locator field: its name, its rendered offending
/// value, and the rule it violated. The record loader adds the
/// file path and node uid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocatorFieldError {
    /// The offending field's wire name.
    pub field: &'static str,
    /// The offending value, rendered for diagnostics.
    pub value: String,
    /// The rule the value violated.
    pub rule: LocatorRule,
}

/// The closed typed locator of one structural source node
/// (LLR-153). Variant membership and per-variant field rules are
/// pinned by the module docs.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "format", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceLocator {
    /// A node inside a Markdown source document.
    Markdown {
        /// Canonical relative path of the Markdown file.
        path: SafeRelPath,
        /// Optional git blob identifier of the file revision.
        #[serde(default)]
        git_blob: Option<String>,
        /// Optional document-author anchor naming the node.
        #[serde(default)]
        anchor: Option<String>,
        /// Heading trail from the document root to the node.
        #[serde(default)]
        heading_path: Vec<String>,
        /// Byte offsets `[start, end]` the node spans; ordered.
        byte_range: (u64, u64),
    },
    /// A node inside an HTML source document.
    Html {
        /// Canonical URL of the document; absolute shape, opaque.
        canonical_url: String,
        /// Optional post-redirect URL; absolute shape, opaque.
        #[serde(default)]
        final_url: Option<String>,
        /// Optional document-author fragment naming the node.
        #[serde(default)]
        fragment: Option<String>,
        /// Heading trail from the document root to the node.
        #[serde(default)]
        heading_path: Vec<String>,
        /// Child indexes from the document root to the node.
        #[serde(default)]
        dom_path: Vec<u32>,
    },
    /// A node inside a PDF source document.
    Pdf {
        /// 1-based physical page index.
        physical_page: u32,
        /// Optional page label as printed on the page.
        #[serde(default)]
        printed_label: Option<String>,
        /// Bounding box `[llx, lly, urx, ury]`: finite,
        /// non-negative, ordered coordinates.
        bbox: [f64; 4],
    },
}

impl SourceLocator {
    /// The variant's `format` wire string.
    pub fn format_str(&self) -> &'static str {
        match self {
            SourceLocator::Markdown { .. } => "markdown",
            SourceLocator::Html { .. } => "html",
            SourceLocator::Pdf { .. } => "pdf",
        }
    }

    /// The RFC 6838 media type a source revision must declare for
    /// this locator variant to agree with it (LLR-157).
    pub(crate) fn expected_media_type(&self) -> &'static str {
        match self {
            SourceLocator::Markdown { .. } => "text/markdown",
            SourceLocator::Html { .. } => "text/html",
            SourceLocator::Pdf { .. } => "application/pdf",
        }
    }

    /// The document-author anchor feeding the structural key's
    /// first precedence tier: the Markdown `anchor` or the HTML
    /// `fragment`. PDF locators carry no node-level explicit
    /// anchor; explicit numbering there arrives through the label.
    pub(crate) fn explicit_anchor(&self) -> Option<&str> {
        match self {
            SourceLocator::Markdown { anchor, .. } => anchor.as_deref(),
            SourceLocator::Html { fragment, .. } => fragment.as_deref(),
            SourceLocator::Pdf { .. } => None,
        }
    }

    /// Validate the per-variant field rules pinned by the module
    /// docs. The `path` and digest fields validate at
    /// deserialization, before this pass runs; the first failure
    /// here wins, so error precedence is deterministic.
    pub(crate) fn validate(&self) -> Result<(), LocatorFieldError> {
        match self {
            SourceLocator::Markdown {
                git_blob,
                anchor,
                heading_path,
                byte_range,
                ..
            } => {
                if let Some(blob) = git_blob {
                    if !is_valid_git_blob(blob) {
                        return Err(field_error("git_blob", blob, LocatorRule::GitBlobHex));
                    }
                }
                if let Some(anchor) = anchor {
                    check_non_blank("anchor", anchor)?;
                }
                check_heading_path(heading_path)?;
                if byte_range.0 > byte_range.1 {
                    return Err(field_error(
                        "byte_range",
                        &format!("[{}, {}]", byte_range.0, byte_range.1),
                        LocatorRule::ByteRangeReversed,
                    ));
                }
                Ok(())
            }
            SourceLocator::Html {
                canonical_url,
                final_url,
                fragment,
                heading_path,
                ..
            } => {
                check_url("canonical_url", canonical_url)?;
                if let Some(final_url) = final_url {
                    check_url("final_url", final_url)?;
                }
                if let Some(fragment) = fragment {
                    check_non_blank("fragment", fragment)?;
                }
                check_heading_path(heading_path)
            }
            SourceLocator::Pdf {
                physical_page,
                printed_label,
                bbox,
            } => {
                if *physical_page == 0 {
                    return Err(field_error("physical_page", "0", LocatorRule::PageZero));
                }
                if let Some(printed_label) = printed_label {
                    check_non_blank("printed_label", printed_label)?;
                }
                check_bbox(bbox)
            }
        }
    }
}

/// Build one invalid-field triple.
fn field_error(field: &'static str, value: &str, rule: LocatorRule) -> LocatorFieldError {
    LocatorFieldError {
        field,
        value: value.to_string(),
        rule,
    }
}

/// A git blob identifier is 40 (SHA-1) or 64 (SHA-256) lowercase
/// hexadecimal characters.
fn is_valid_git_blob(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A string that must carry content rejects whitespace-only
/// values; accepted text is stored untrimmed.
fn check_non_blank(field: &'static str, value: &str) -> Result<(), LocatorFieldError> {
    if value.trim().is_empty() {
        return Err(field_error(field, value, LocatorRule::Blank));
    }
    Ok(())
}

/// Every heading-path component carries content.
fn check_heading_path(heading_path: &[String]) -> Result<(), LocatorFieldError> {
    for component in heading_path {
        check_non_blank("heading_path", component)?;
    }
    Ok(())
}

/// A URL is opaque audit identity with an absolute
/// `<scheme>://<rest>` shape: the scheme starts with an ASCII
/// letter and continues with ASCII letters, digits, `+`, `-`, or
/// `.`; the rest is non-empty; no whitespace appears anywhere.
fn check_url(field: &'static str, value: &str) -> Result<(), LocatorFieldError> {
    let valid = match value.split_once("://") {
        Some((scheme, rest)) => {
            !rest.is_empty()
                && scheme
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_alphabetic())
                && scheme
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
                && !value.chars().any(char::is_whitespace)
        }
        None => false,
    };
    if !valid {
        return Err(field_error(field, value, LocatorRule::UrlScheme));
    }
    Ok(())
}

/// Bounding-box coordinates are finite, non-negative, and ordered
/// lower-left to upper-right on both axes.
fn check_bbox(bbox: &[f64; 4]) -> Result<(), LocatorFieldError> {
    let rendered = || format!("[{}, {}, {}, {}]", bbox[0], bbox[1], bbox[2], bbox[3]);
    if bbox.iter().any(|c| !c.is_finite()) {
        return Err(field_error("bbox", &rendered(), LocatorRule::BboxNonFinite));
    }
    if bbox.iter().any(|c| *c < 0.0) {
        return Err(field_error("bbox", &rendered(), LocatorRule::BboxNegative));
    }
    if bbox[0] > bbox[2] || bbox[1] > bbox[3] {
        return Err(field_error("bbox", &rendered(), LocatorRule::BboxReversed));
    }
    Ok(())
}
