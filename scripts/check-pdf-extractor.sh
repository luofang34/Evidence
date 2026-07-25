#!/usr/bin/env bash
# Pinned Poppler extractor lane (#222, LLR-179, LLR-180). Runs
# inside `nix develop`, where the flake.lock-pinned nixpkgs
# `poppler-utils` is on PATH. The lane:
#
#   1. verifies the executable's `-v` output and SHA-256 against
#      the committed tool lock
#      (`crates/evidence-core/tests/fixtures/corpus/pdf_tool_lock_v1.toml`)
#      for the current platform when the lock carries one;
#   2. runs the extractor over the committed SDLS acceptance PDF
#      with the exact pinned argv in an isolated temporary
#      directory; and
#   3. byte-compares the extractor output against the committed
#      golden — the extractor-output identity plane.
#
# PATH lookup is acceptable here: this script is the CI lane, not
# the core runner. The library runner
# (`run_pdftotext_blocking`) receives an explicit path and never
# searches PATH.

set -euo pipefail

fixture_dir="crates/evidence-core/tests/fixtures/corpus"
lock="$fixture_dir/pdf_tool_lock_v1.toml"
pdf="$fixture_dir/pdf_sdls_acceptance_v1.pdf"
golden="$fixture_dir/pdf_sdls_bbox_v1.xhtml"

exe="$(command -v pdftotext || true)"
if [ -z "$exe" ]; then
    printf 'poppler lane: pdftotext not on PATH (run inside nix develop).\n' >&2
    exit 1
fi
exe="$(readlink -f "$exe")"

field() {
    # field <key> — read a top-level string field from the lock.
    sed -n "s/^$1 = \"\\(.*\\)\"$/\\1/p" "$2"
}

platform="$(uname -s)-$(uname -m)"
case "$platform" in
    Linux-x86_64)  key="linux_x86_64" ;;
    Darwin-arm64)  key="macos_aarch64" ;;
    Darwin-x86_64) key="macos_x86_64" ;;
    *)             key="" ;;
esac

version_line="$(pdftotext -v 2>&1 | head -1)"
locked_version="$(field version_output "$lock")"
if [ "$version_line" != "$locked_version" ]; then
    printf 'poppler lane: version mismatch: locked %s, found %s\n' \
        "$locked_version" "$version_line" >&2
    exit 1
fi

if [ -n "$key" ]; then
    locked_digest="$(sed -n "s/^$key = \"\\(.*\\)\"$/\\1/p" "$lock")"
    if [ -n "$locked_digest" ]; then
        found_digest="$(shasum -a 256 "$exe" | cut -d' ' -f1)"
        if [ "$found_digest" != "$locked_digest" ]; then
            printf 'poppler lane: executable digest mismatch for %s: locked %s, found %s\n' \
                "$key" "$locked_digest" "$found_digest" >&2
            exit 1
        fi
        printf 'poppler lane: executable digest verified for %s.\n' "$key"
    else
        printf 'poppler lane: no locked digest for %s; skipping digest check.\n' "$key"
    fi
fi

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

pdftotext -bbox-layout -enc UTF-8 -eol unix -cropbox -q \
    "$pdf" "$tmpdir/output.xhtml"

if ! cmp -s "$tmpdir/output.xhtml" "$golden"; then
    printf 'poppler lane: extractor output diverges from the committed golden %s\n' \
        "$golden" >&2
    exit 1
fi
printf 'poppler lane: extractor output matches the committed golden.\n'
