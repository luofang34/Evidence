#!/usr/bin/env python3
"""Generate the minimal M4.6 PDF acceptance fixtures (#222).

Hand-crafts two tiny, valid, independently redistributable PDFs:

- `pdf_sdls_acceptance_v1.pdf` — an SDLS-shaped two-page fragment
  with numbered sections, document page labels ("Page N" footers),
  numbered paragraphs, a two-column region, a hyphenated word split
  across lines (proving the projection never dehyphenates), a note,
  and a figure caption.
- `pdf_pics_acceptance_v1.pdf` — a parser-hostile CCSDS
  PICS-shaped single page whose column-aligned table region the raw
  bbox projection cannot prove, exercising the structural-loss
  diagnostic and the approved curated-patch recovery.

The PDFs use only the built-in Helvetica base font, no compression,
no encryption, and no external references. Both are committed under
`crates/evidence-core/tests/fixtures/corpus/` together with the
exact `pdftotext -bbox-layout -enc UTF-8 -eol unix -cropbox -q`
output produced by the Nix-pinned Poppler (see
`pdf_tool_lock_v1.toml` for the pinned tool identity).

Usage: python3 tools/gen-pdf-acceptance-fixtures.py
"""

import io
import os

FIXTURE_DIR = os.path.join(
    os.path.dirname(__file__), "..", "crates", "evidence-core", "tests", "fixtures", "corpus"
)

PAGE_WIDTH = 612
PAGE_HEIGHT = 792


def escape(text):
    return text.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


def render_pdf(pages):
    """Render pages of (x, y, size, text) lines into a minimal PDF.

    Coordinates are PDF user space (origin bottom-left, points).
    Object numbering: 1 catalog, 2 pages, 3 font, then per page a
    page object and a content-stream object.
    """
    objects = []
    page_ids = [4 + 2 * i for i in range(len(pages))]
    content_ids = [5 + 2 * i for i in range(len(pages))]
    kids = " ".join(f"{pid} 0 R" for pid in page_ids)
    objects.append((1, "<< /Type /Catalog /Pages 2 0 R >>"))
    objects.append((2, f"<< /Type /Pages /Kids [{kids}] /Count {len(pages)} >>"))
    objects.append((3, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"))
    for i, page in enumerate(pages):
        ops = []
        for (x, y, size, text) in page:
            ops.append(f"BT /F1 {size} Tf {x} {y} Td ({escape(text)}) Tj ET")
        stream = "\n".join(ops)
        objects.append(
            (
                page_ids[i],
                f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] "
                f"/Resources << /Font << /F1 3 0 R >> >> /Contents {content_ids[i]} 0 R >>",
            )
        )
        objects.append(
            (content_ids[i], f"<< /Length {len(stream)} >>\nstream\n{stream}\nendstream")
        )
    buf = io.BytesIO()
    buf.write(b"%PDF-1.4\n")
    offsets = {}
    for number, body in objects:
        offsets[number] = buf.tell()
        buf.write(f"{number} 0 obj\n{body}\nendobj\n".encode())
    xref_pos = buf.tell()
    total = max(offsets) + 1
    buf.write(f"xref\n0 {total}\n".encode())
    buf.write(b"0000000000 65535 f \n")
    for number in range(1, total):
        buf.write(f"{offsets[number]:010d} 00000 n \n".encode())
    buf.write(
        f"trailer\n<< /Size {total} /Root 1 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n".encode()
    )
    return buf.getvalue()


def sdls_pages():
    """The SDLS-shaped two-page fragment (PDF bottom-up coordinates).

    Layout intent (top-down y after Poppler flips): header band at
    the very top, footer "Page N" at the very bottom, headings on
    their own lines, paragraph blocks with generous internal leading
    but wide gaps around headings so Poppler keeps headings as
    single-line blocks.
    """
    page1 = [
        (72, 756, 8, "SDLS-LIKE SPECIFICATION"),  # header band
        (72, 700, 14, "1 Scope"),  # heading, depth 1
        (72, 672, 10, "1.1 This document defines the"),
        (72, 660, 10, "inter-"),
        (72, 648, 10, "national profile of the exchange format."),
        (72, 606, 14, "1.1 Definitions"),  # heading, depth 2
        (72, 578, 10, "1.1.1 A datum is a typed value."),
        (72, 560, 10, "NOTE This note records a caution."),  # note
        (72, 24, 9, "Page 1"),  # footer band, printed label "1"
    ]
    page2 = [
        (72, 756, 8, "SDLS-LIKE SPECIFICATION"),  # header band
        (72, 700, 14, "2 Requirements"),  # heading, depth 1
        # Right column drawn first: reading order must still place
        # the left column first under the configured column split.
        (320, 640, 10, "2.2 Right column text reads"),
        (320, 628, 10, "after the left column block."),
        (72, 640, 10, "2.1 Left column text reads"),
        (72, 628, 10, "before the right column block."),
        (72, 584, 10, "Figure 1 Example two-column layout."),  # caption
        (72, 24, 9, "Page 2"),  # footer band, printed label "2"
    ]
    return [page1, page2]


def pics_pages():
    """The PICS-shaped single page: a column-aligned table region.

    Each row is one text run with space-separated cells so Poppler
    keeps rows as multi-word lines inside one tightly-spaced block
    whose row/cell structure the committed rules cannot prove.
    """
    rows = [
        "Item M Status",
        "R1 M Mandatory",
        "R2 O Optional",
        "R3 C1 Conditional",
    ]
    page = [
        (72, 756, 8, "CCSDS-LIKE PICS"),  # header band
        (72, 700, 14, "3 PICS Proforma"),  # heading, depth 1
        (72, 672, 10, "3.1 The implementation conformance statement"),
        (72, 660, 10, "follows the proforma below."),
    ]
    y = 612
    for row in rows:
        page.append((72, y, 10, row))
        y -= 12
    page.append((72, 24, 9, "Page 1"))  # footer band
    return [page]


def main():
    fixtures = {
        "pdf_sdls_acceptance_v1.pdf": render_pdf(sdls_pages()),
        "pdf_pics_acceptance_v1.pdf": render_pdf(pics_pages()),
    }
    for name, data in fixtures.items():
        path = os.path.join(FIXTURE_DIR, name)
        with open(path, "wb") as handle:
            handle.write(data)
        print(f"wrote {path} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
