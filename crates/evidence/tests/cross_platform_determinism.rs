//! Cross-platform determinism test.
//!
//! The companion to the golden-bundle fixture: golden asserts a
//! *committed* bundle verifies, this asserts the *production path*
//! (SHA256SUMS + content_hash) is byte-identical on every CI host.
//!
//! Each of the three CI jobs (Linux / macOS / Windows) runs this test
//! against a synthetic bundle built from fixed bytes, then asserts the
//! resulting SHA-256 of `SHA256SUMS` matches a committed expected
//! value. If any host diverges — because walkdir reordering,
//! path-separator normalization, per-file hashing, or line-ending
//! handling in `SHA256SUMS` itself grew a platform-dependent branch —
//! that one CI job fails. The hash doesn't need to be independently
//! meaningful; it just needs to be the same everywhere.
//!
//! Scope rationale: `content_hash = SHA-256(SHA256SUMS)`, and
//! `index.json` (which carries the live timestamp and git SHA) is
//! excluded from `SHA256SUMS` by design. So cross-platform parity of
//! `content_hash` reduces to cross-platform parity of
//! `write_sha256sums` + `sha256_file` on identical input bytes. We do
//! not need a Clock trait, a mock `EnvFingerprint`, or a mock
//! `GitProvider` to test that property — just deterministic input
//! files and the real hash path.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use evidence::hash::{sha256_file, write_sha256sums};
use std::fs;
use tempfile::TempDir;

/// Baseline SHA-256 of `SHA256SUMS` for the synthetic bundle below.
///
/// Computed once from the real code on a reference host and committed
/// verbatim. Every CI host must reproduce this value. If the code
/// legitimately changes how SHA256SUMS is emitted (field format,
/// ordering, normalization), all three CI jobs will fail in unison
/// and the new expected hash goes in the same PR — mirroring the
/// regeneration discipline of the golden fixture.
const EXPECTED_CONTENT_HASH: &str =
    "8857c750960bb0a4b05df8892c0cc9c382eadff17a101c84c7075e46a6e56504";

fn populate_synthetic_bundle(root: &std::path::Path) {
    // Root-level files with diverse byte content, different extensions,
    // and content that's not trivially uniform (so a broken hash path
    // is more likely to produce a visibly-wrong digest).
    fs::write(root.join("alpha.txt"), b"alpha content\n").unwrap();
    fs::write(root.join("beta.json"), b"{\"k\":\"v\",\"n\":42}\n").unwrap();

    // Empty file — hashes to the well-known SHA-256 of zero bytes.
    fs::write(root.join("empty.bin"), b"").unwrap();

    // Non-ASCII bytes — guards against accidental UTF-8 re-encoding
    // somewhere in the read path. Raw bytes should flow through sha256
    // unchanged.
    fs::write(root.join("binary.dat"), [0x00, 0xff, 0x7f, 0x80, 0x0a]).unwrap();

    // Nested subdirectories force walkdir to cross directory
    // boundaries; the sort + forward-slash normalization in
    // `write_sha256sums` must produce the same relative paths on
    // Windows (native `\`) and on Unix (native `/`).
    fs::create_dir_all(root.join("nested/deeper")).unwrap();
    fs::write(root.join("nested/gamma.txt"), b"gamma\n").unwrap();
    fs::write(root.join("nested/deeper/delta.txt"), b"delta\n").unwrap();

    // Root-level uppercase filename — forces the lexicographic sort in
    // `write_sha256sums` to place 'Z' (0x5A) *before* lowercase
    // letters (0x61+), which a naive case-folding sort would get
    // wrong. We deliberately do NOT include a `zeta.txt` pair:
    // filenames that differ only in case alias on case-insensitive
    // filesystems (APFS default, NTFS default), so the on-disk file
    // set would itself be platform-dependent — a filesystem property,
    // not a code property, and out of scope for this test.
    fs::write(root.join("Zeta.txt"), b"upper\n").unwrap();
}

#[test]
fn content_hash_is_cross_platform_deterministic() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    populate_synthetic_bundle(root);

    let sums_path = root.join("SHA256SUMS");
    write_sha256sums(root, &sums_path).expect("write_sha256sums");

    let content_hash = sha256_file(&sums_path).expect("hash SHA256SUMS");

    assert_eq!(
        content_hash, EXPECTED_CONTENT_HASH,
        "\nCross-platform determinism drift.\n\
         Got:      {}\n\
         Expected: {}\n\n\
         This assertion runs on every CI host (Linux/macOS/Windows). \
         If ONE host sees a different hash, the production code path grew \
         a platform-dependent branch — review recent changes to \
         `hash::write_sha256sums`, `hash::sha256_file`, and any path \
         normalization in the bundle pipeline.\n\n\
         If ALL hosts see the same different hash, the SHA256SUMS format \
         has changed legitimately — update EXPECTED_CONTENT_HASH in this \
         file in the same PR that changes the emitter.\n",
        content_hash, EXPECTED_CONTENT_HASH
    );
}

#[test]
fn sha256sums_contents_are_cross_platform_deterministic() {
    // Complements the content_hash assertion with a byte-level check
    // on SHA256SUMS itself. A failing content_hash doesn't tell you
    // *what* diverged; asserting the raw lines does. Both must pass.
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    populate_synthetic_bundle(root);

    let sums_path = root.join("SHA256SUMS");
    write_sha256sums(root, &sums_path).expect("write_sha256sums");

    let actual = fs::read_to_string(&sums_path).expect("read SHA256SUMS");

    // Expected bytes: each line is `<64 hex>  <rel/path>`, separated
    // by LF, with a trailing LF. Forward slashes mandatory.
    // Sort is lexicographic on the *relative path string*; uppercase
    // 'Z' (0x5A) sorts before lowercase letters (0x61+).
    let expected = "\
e83189db38554920ea572093f9ad32facf682f28ccecdac085c1511735a2b492  Zeta.txt
7372b75dfb24271a231d7c882b0cdbd0df8a1bb075764ca16ec9df0df2582d65  alpha.txt
12908184dd918925061b5d6a4e9aedd4e5506f4f984a5dc64b0a78a71a105a39  beta.json
4f5944b267b0d4e2b313be5d8a4ad12a28813338d875bdfd8dc99da8cbdf39cd  binary.dat
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  empty.bin
673953e0ad7fc53247f4feadc2c2d4506396840d1f8796526f48d47333ac7652  nested/deeper/delta.txt
ae9a6306a205417afddd14316cc1d0d5e04a98f1be10865dce643925ee070ce2  nested/gamma.txt
";

    assert_eq!(
        actual, expected,
        "SHA256SUMS byte drift across platforms. See diff above."
    );
}
