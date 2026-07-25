//! Bounded fail-closed bbox-parser attack tests (TEST-197):
//! entity, doctype, depth, count, encoding, coordinate, and
//! page-order attacks all fail closed with page and element
//! context.

use super::*;

/// Wrap page bodies in the pinned document shape.
fn doc(pages: &str) -> String {
    format!(
        "{PINNED_DOCTYPE}<html xmlns=\"http://www.w3.org/1999/xhtml\"><head></head><body><doc>{pages}</doc></body></html>"
    )
}

/// One minimal valid page body.
fn page(body: &str) -> String {
    format!(
        "<page width=\"612.000000\" height=\"792.000000\"><flow><block xMin=\"1\" yMin=\"1\" xMax=\"2\" yMax=\"2\"><line xMin=\"1\" yMin=\"1\" xMax=\"2\" yMax=\"2\">{body}</line></block></flow></page>"
    )
}

/// One minimal valid word element.
const WORD: &str = "<word xMin=\"1\" yMin=\"1\" xMax=\"2\" yMax=\"2\">w</word>";

#[test]
fn entity_and_doctype_attacks_fail_closed() {
    // An ENTITY declaration is rejected before parsing, whatever
    // its case.
    let attack = doc(&page(WORD)).replace(
        "<doc>",
        "<doc><!-- x --><!ENTITY xxe SYSTEM \"file:///etc/passwd\">",
    );
    assert!(matches!(
        parse_bbox_layout(attack.as_bytes()),
        Err(BboxParseError::EntityDeclaration)
    ));

    // An entity reference never expands: the parser fails closed.
    let attack = doc(&page(
        "<word xMin=\"1\" yMin=\"1\" xMax=\"2\" yMax=\"2\">&xxe;</word>",
    ));
    assert!(matches!(
        parse_bbox_layout(attack.as_bytes()),
        Err(BboxParseError::MalformedXml { .. })
    ));

    // A DOCTYPE other than the pinned Poppler declaration is
    // rejected.
    let attack = doc(&page(WORD)).replace(
        "http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd",
        "http://attacker.example/evil.dtd",
    );
    assert!(matches!(
        parse_bbox_layout(attack.as_bytes()),
        Err(BboxParseError::DoctypeRejected)
    ));

    // A second DOCTYPE after the pinned one is rejected.
    let attack = format!("{}{}", doc(&page(WORD)), PINNED_DOCTYPE);
    assert!(matches!(
        parse_bbox_layout(attack.as_bytes()),
        Err(BboxParseError::DoctypeRejected)
    ));

    // Malformed XML fails closed.
    assert!(matches!(
        parse_bbox_layout(b"<html><body>"),
        Err(BboxParseError::MalformedXml { .. })
    ));

    // An unknown structural element is rejected with context.
    let attack = doc(&page(WORD)).replace("<doc>", "<doc><script>alert(1)</script>");
    assert!(matches!(
        parse_bbox_layout(attack.as_bytes()),
        Err(BboxParseError::UnknownElement { .. })
    ));
}

#[test]
fn depth_count_and_encoding_bounds_fail_closed() {
    // Invalid UTF-8 carries the first invalid offset.
    let mut bytes = doc(&page(WORD)).into_bytes();
    bytes.push(0xFF);
    assert!(matches!(
        parse_bbox_layout(&bytes),
        Err(BboxParseError::NonUtf8 { .. })
    ));

    // Words beyond the per-line bound fail closed.
    let words = WORD.repeat(MAX_WORDS_PER_LINE + 1);
    assert!(matches!(
        parse_bbox_layout(doc(&page(&words)).as_bytes()),
        Err(BboxParseError::BoundExceeded { what: "words", .. })
    ));

    // An element with too many attributes fails closed.
    let fat = "<word xMin=\"1\" yMin=\"1\" xMax=\"2\" yMax=\"2\" a=\"1\" b=\"2\" c=\"3\" d=\"4\" e=\"5\">w</word>";
    assert!(matches!(
        parse_bbox_layout(doc(&page(fat)).as_bytes()),
        Err(BboxParseError::BoundExceeded {
            what: "attributes",
            ..
        })
    ));

    // Pages beyond the page bound fail closed.
    let pages = page(WORD).repeat(MAX_PAGES + 1);
    assert!(matches!(
        parse_bbox_layout(doc(&pages).as_bytes()),
        Err(BboxParseError::BoundExceeded { what: "pages", .. })
    ));

    // An element nested inside a word is an unknown structural
    // element, never silently skipped.
    let nested = "<word xMin=\"1\" yMin=\"1\" xMax=\"2\" yMax=\"2\"><word xMin=\"1\" yMin=\"1\" xMax=\"2\" yMax=\"2\">x</word></word>";
    assert!(matches!(
        parse_bbox_layout(doc(&page(nested)).as_bytes()),
        Err(BboxParseError::UnknownElement { .. })
    ));
}

#[test]
fn coordinate_and_page_order_attacks_fail_closed() {
    let parse = |word: &str| parse_bbox_layout(doc(&page(word)).as_bytes());

    // A negative coordinate fails closed.
    assert!(matches!(
        parse("<word xMin=\"-1\" yMin=\"1\" xMax=\"2\" yMax=\"2\">w</word>"),
        Err(BboxParseError::InvalidCoordinate { .. })
    ));
    // A non-finite coordinate fails closed.
    assert!(matches!(
        parse("<word xMin=\"NaN\" yMin=\"1\" xMax=\"2\" yMax=\"2\">w</word>"),
        Err(BboxParseError::InvalidCoordinate { .. })
    ));
    // Reversed corners fail closed.
    assert!(matches!(
        parse("<word xMin=\"3\" yMin=\"1\" xMax=\"2\" yMax=\"2\">w</word>"),
        Err(BboxParseError::InvalidCoordinate { .. })
    ));
    // An out-of-page coordinate fails closed.
    assert!(matches!(
        parse("<word xMin=\"1\" yMin=\"1\" xMax=\"999999\" yMax=\"2\">w</word>"),
        Err(BboxParseError::InvalidCoordinate { .. })
    ));
    // A missing coordinate attribute fails closed.
    assert!(matches!(
        parse("<word yMin=\"1\" xMax=\"2\" yMax=\"2\">w</word>"),
        Err(BboxParseError::InvalidCoordinate { .. })
    ));
    // A page without dimensions fails closed.
    let no_dims = doc("<page><flow></flow></page>");
    assert!(matches!(
        parse_bbox_layout(no_dims.as_bytes()),
        Err(BboxParseError::MissingDimensions { .. })
    ));
    // A document without pages fails closed.
    assert!(matches!(
        parse_bbox_layout(doc("").as_bytes()),
        Err(BboxParseError::EmptyDocument)
    ));
    // A page nested where a block belongs is an unknown structural
    // element — document order cannot be smuggled.
    let smuggled = doc(&page(&format!(
        "<page width=\"1\" height=\"1\"></page>{WORD}"
    )));
    assert!(matches!(
        parse_bbox_layout(smuggled.as_bytes()),
        Err(BboxParseError::UnknownElement { .. })
    ));
}

#[test]
fn the_committed_fixture_outputs_parse() {
    for name in ["pdf_sdls_bbox_v1.xhtml", "pdf_pics_bbox_v1.xhtml"] {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/corpus")
            .join(name);
        let bytes = std::fs::read(path).expect("read fixture");
        parse_bbox_layout(&bytes).expect("committed extractor output parses");
    }
}
