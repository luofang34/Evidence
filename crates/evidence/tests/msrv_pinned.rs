//! MSRV guard.
//!
//! The Rust toolchain version this project requires is stated in
//! four places — three canonical, one Nix-derived:
//!
//! | File                          | Field                     |
//! |-------------------------------|---------------------------|
//! | `rust-toolchain.toml`         | `[toolchain].channel`     |
//! | `Cargo.toml` (workspace)      | `workspace.package.rust-version` |
//! | `.github/workflows/ci.yml`    | every `toolchain: "X.Y"` |
//! | `flake.nix`                   | (reads `rust-toolchain.toml`, no hardcode) |
//!
//! Drift between any of the three non-derived sources is a silent
//! foot-gun: a contributor bumps one, CI still runs on the unbumped
//! pin, and the `rust-version` floor declared to downstream crates-io
//! consumers ends up lower than what the code actually requires.
//!
//! This test parses all three and fails if they disagree, or if the
//! canonical pin drops below `MSRV_FLOOR`. When bumping MSRV:
//!
//!   1. Edit `rust-toolchain.toml` channel.
//!   2. Edit workspace `Cargo.toml` `rust-version`.
//!   3. Edit every `toolchain:` line in `.github/workflows/ci.yml`
//!      (and the human-readable `- name: Install Rust …` step title
//!      if you want the log to stay honest).
//!   4. Bump `MSRV_FLOOR` below.
//!
//! Doing them out of order makes this test go red. That's the point.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::fs;
use std::path::PathBuf;

/// Minimum allowed canonical toolchain version. Bump this when the
/// project intentionally raises its MSRV; leave it pinned otherwise.
const MSRV_FLOOR: (u32, u32) = (1, 95);

fn workspace_root() -> PathBuf {
    // This test lives in `crates/evidence/tests/`, so the workspace
    // root is two levels up from CARGO_MANIFEST_DIR.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = workspace_root().join(rel);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("reading {}: {}", p.display(), e))
}

/// Extract the first `key = "value"` assignment (or `key = value`) at
/// top indentation from a TOML-ish file, ignoring comments.
///
/// We deliberately don't pull in a real TOML parser — the test needs
/// to stand alone, and our assertion surface is three unambiguous
/// scalar fields whose string layout the tree enforces via
/// `cargo fmt` / `taplo` in review.
fn scalar_value(content: &str, key: &str) -> Option<String> {
    for raw in content.lines() {
        let line = raw.split('#').next().unwrap_or(raw).trim();
        if let Some(rest) = line.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim();
                let v = rest.trim_matches(|c: char| c == '"' || c.is_whitespace());
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

fn parse_toolchain_channel() -> String {
    scalar_value(&read("rust-toolchain.toml"), "channel")
        .expect("rust-toolchain.toml must have a [toolchain] channel")
}

fn parse_workspace_rust_version() -> String {
    scalar_value(&read("Cargo.toml"), "rust-version")
        .expect("workspace Cargo.toml must have a [workspace.package] rust-version")
}

/// Every `toolchain: "X.Y"` (or `X.Y.Z`) line in `.github/workflows/ci.yml`,
/// paired with the 1-based line number so the failure message points
/// directly at the offender.
fn parse_ci_toolchains() -> Vec<(usize, String)> {
    let content = read(".github/workflows/ci.yml");
    let mut hits = Vec::new();
    for (i, raw) in content.lines().enumerate() {
        let line = raw.trim_start();
        if let Some(rest) = line.strip_prefix("toolchain:") {
            // Strip trailing comments, then quotes/whitespace.
            let rest = rest.split('#').next().unwrap_or(rest);
            let v = rest
                .trim()
                .trim_matches(|c: char| c == '"' || c.is_whitespace());
            if !v.is_empty() {
                hits.push((i + 1, v.to_string()));
            }
        }
    }
    hits
}

/// Parse a dotted version to `(major, minor)`. Patch component, if
/// present, is ignored — MSRV comparisons are done at minor
/// granularity.
fn major_minor(v: &str) -> (u32, u32) {
    let mut it = v.split('.');
    let major: u32 = it
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("version '{}' has no major component", v));
    let minor: u32 = it
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("version '{}' has no minor component", v));
    (major, minor)
}

#[test]
fn msrv_pins_agree_across_sources() {
    let channel = parse_toolchain_channel();
    let rust_version = parse_workspace_rust_version();
    assert_eq!(
        channel, rust_version,
        "rust-toolchain.toml channel '{}' disagrees with Cargo.toml rust-version '{}'. \
         Bump both together.",
        channel, rust_version
    );

    let ci = parse_ci_toolchains();
    assert!(
        !ci.is_empty(),
        "No `toolchain:` lines found in .github/workflows/ci.yml — has the workflow changed?"
    );
    for (lineno, value) in &ci {
        assert_eq!(
            *value, channel,
            ".github/workflows/ci.yml:{} pins toolchain '{}', but rust-toolchain.toml channel is '{}'. \
             Update every `toolchain:` line when bumping the MSRV.",
            lineno, value, channel
        );
    }
}

#[test]
fn msrv_meets_floor() {
    let channel = parse_toolchain_channel();
    let got = major_minor(&channel);
    assert!(
        got >= MSRV_FLOOR,
        "rust-toolchain.toml channel {}.{} is below the declared MSRV floor {}.{}. \
         Raising the floor is allowed only by intent — update MSRV_FLOOR in this test \
         in the same PR.",
        got.0,
        got.1,
        MSRV_FLOOR.0,
        MSRV_FLOOR.1
    );
}

#[test]
fn scalar_value_parser_sanity() {
    // Double-quoted string, canonical form.
    assert_eq!(
        scalar_value("channel = \"1.95\"", "channel").as_deref(),
        Some("1.95")
    );
    // Surrounding whitespace.
    assert_eq!(
        scalar_value("  rust-version  =  \"1.95\"  ", "rust-version").as_deref(),
        Some("1.95")
    );
    // Inline comment is stripped before parsing.
    assert_eq!(
        scalar_value("channel = \"1.95\"  # pinned", "channel").as_deref(),
        Some("1.95")
    );
    // Missing key returns None, not a panic.
    assert_eq!(scalar_value("foo = \"bar\"", "channel"), None);
    // Non-assignment line with the key as a prefix must not match.
    assert_eq!(scalar_value("channel_name = \"x\"", "channel"), None);
}
