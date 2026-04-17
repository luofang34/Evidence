//! Embed the evidence-engine's own provenance into the binary.
//!
//! Emits two `rustc-env` values read at generate time:
//!
//! - `EVIDENCE_ENGINE_GIT_SHA` — a 40-char hex commit SHA when we can
//!   capture one, otherwise a `release-v<CARGO_PKG_VERSION>` fallback.
//!   Never `"unknown"`: a bundle that can't say which engine produced
//!   it is worse than one that honestly says "this was a crates.io
//!   release build".
//!
//! - `EVIDENCE_ENGINE_BUILD_SOURCE` — which of the branches below
//!   produced the SHA, so `verify` can cross-check the shape of the
//!   value:
//!   * `"git"`     — caller set `EVIDENCE_ENGINE_GIT_SHA` explicitly
//!     (CI publish path: `${GITHUB_SHA}`), OR `git rev-parse HEAD`
//!     succeeded in this build tree.
//!   * `"release"` — no git, fallback string was embedded.
//!
//! Resolution order (first match wins):
//!   1. `EVIDENCE_ENGINE_GIT_SHA` env var — publish workflows set this
//!      from `${GITHUB_SHA}` so crates.io releases ship a real SHA
//!      even when cargo's publish tarball drops `.git/`.
//!   2. `git rev-parse HEAD` in the build directory.
//!   3. `release-v<CARGO_PKG_VERSION>` fallback.
//!
//! The env-var override is trusted; we don't validate its shape here.
//! If a publisher sets `EVIDENCE_ENGINE_GIT_SHA=bogus`, `verify` will
//! reject the resulting bundle at audit time.

fn main() {
    // Invalidate the build script when any input to the resolution
    // changes: the env override, the HEAD ref, or the crate version
    // (via Cargo.toml, tracked implicitly via CARGO_PKG_VERSION).
    println!("cargo:rerun-if-env-changed=EVIDENCE_ENGINE_GIT_SHA");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");

    let (sha, source) = resolve();

    println!("cargo:rustc-env=EVIDENCE_ENGINE_GIT_SHA={sha}");
    println!("cargo:rustc-env=EVIDENCE_ENGINE_BUILD_SOURCE={source}");
}

fn resolve() -> (String, &'static str) {
    if let Ok(explicit) = std::env::var("EVIDENCE_ENGINE_GIT_SHA")
        && !explicit.trim().is_empty()
    {
        return (explicit.trim().to_string(), "git");
    }

    if let Some(git_sha) = try_git_rev_parse() {
        return (git_sha, "git");
    }

    // Cargo guarantees CARGO_PKG_VERSION is set when invoking the
    // build script, so env! at build-script compile time is a safe
    // read. Using the macro also keeps a version-shaped string literal
    // out of this file (the `schema_versions_locked` test forbids
    // them outside `schema_versions.rs`).
    let version = env!("CARGO_PKG_VERSION");
    (format!("release-v{version}"), "release")
}

fn try_git_rev_parse() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}
