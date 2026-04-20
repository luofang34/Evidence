//! Runtime capture of the current host's build environment into an
//! `EnvFingerprint`: tool detection, rustc/cargo version extraction,
//! Nix-shell detection, and Cargo.lock / rust-toolchain.toml / RUSTFLAGS
//! snapshotting.

use std::collections::BTreeMap;
use std::process::Command;

use thiserror::Error;

use crate::diagnostic::{DiagnosticCode, Severity};
use crate::git::{git_branch, git_sha, is_dirty_or_unknown};
use crate::policy::Profile;
use crate::util::cmd_stdout;

use super::fingerprint::EnvFingerprint;
use super::host::Host;

/// Errors returned by [`env_fingerprint`].
#[derive(Debug, Error)]
pub enum EnvCaptureError {
    /// A cert/record profile was requested but `rustc --version` failed.
    #[error(
        "cert/record profile requires rustc to be installed and on PATH. rustc --version failed."
    )]
    StrictRustcRequired,
    /// A cert/record profile was requested but `cargo --version` failed.
    #[error(
        "cert/record profile requires cargo to be installed and on PATH. cargo --version failed."
    )]
    StrictCargoRequired,
}

impl DiagnosticCode for EnvCaptureError {
    fn code(&self) -> &'static str {
        match self {
            EnvCaptureError::StrictRustcRequired => "ENV_STRICT_RUSTC_REQUIRED",
            EnvCaptureError::StrictCargoRequired => "ENV_STRICT_CARGO_REQUIRED",
        }
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }
}

/// Capture a complete environment fingerprint.
///
/// When `strict` is true (cert/record profiles), critical tools (rustc, cargo)
/// must be found or the function returns an error. This prevents evidence
/// bundles from being generated in an incomplete environment.
pub fn env_fingerprint(profile: Profile, strict: bool) -> Result<EnvFingerprint, EnvCaptureError> {
    let rustc = cmd_stdout("rustc", &["--version"]);
    let cargo = cmd_stdout("cargo", &["--version"]);

    if strict {
        if rustc.is_err() {
            return Err(EnvCaptureError::StrictRustcRequired);
        }
        if cargo.is_err() {
            return Err(EnvCaptureError::StrictCargoRequired);
        }
    }

    let rustc_str = rustc.unwrap_or_else(|_| "unknown".to_string());
    let cargo_str = cargo.unwrap_or_else(|_| "unknown".to_string());

    let mut tools = BTreeMap::new();
    tools.insert("git".to_string(), tool_exists("git", &["--version"]));
    tools.insert(
        "cargo-llvm-cov".to_string(),
        tool_exists("cargo", &["llvm-cov", "--version"]),
    );

    let mut nav_env = BTreeMap::new();
    for (k, v) in std::env::vars() {
        if k.starts_with("NAV_") {
            nav_env.insert(k, v);
        }
    }

    // Platform capsule: extract LLVM version from rustc -vV
    let llvm_version = extract_llvm_version();
    let host = Host::detect();
    let target_triple = extract_target_triple().unwrap_or_else(|| "unknown".to_string());

    // Cargo.lock hash if present. "file absent" is a legitimate None
    // (projects without Cargo.lock). "file exists but I/O failed" is
    // NOT — it silently drops a reproducibility input. Warn on the
    // I/O-error path so downstream tooling can trace why a cert-
    // profile bundle's manifest lacks the hash.
    let cargo_lock_hash = if std::path::Path::new("Cargo.lock").exists() {
        match std::fs::read("Cargo.lock") {
            Ok(data) => Some(crate::hash::sha256(&data)),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Cargo.lock exists but read failed; cargo_lock_hash will be null \
                     in env.json, which may cause cross-time determinism drift against \
                     prior bundles that captured it"
                );
                None
            }
        }
    } else {
        None
    };

    // rust-toolchain.toml contents if present. Same "absent vs
    // unreadable" distinction as Cargo.lock above.
    let rust_toolchain_toml = match std::fs::read_to_string("rust-toolchain.toml") {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "rust-toolchain.toml exists but read failed; rust_toolchain_toml will be \
                 null in env.json, which may cause cross-time determinism drift"
            );
            None
        }
    };

    // RUSTFLAGS env var
    let rustflags = std::env::var("RUSTFLAGS").ok();

    Ok(EnvFingerprint {
        profile,
        rustc: rustc_str.trim().to_string(),
        cargo: cargo_str.trim().to_string(),
        git_sha: git_sha().unwrap_or_else(|_| "unknown".to_string()),
        git_branch: git_branch().unwrap_or_else(|_| "unknown".to_string()),
        // Safe default: treat unknown git state as dirty. This
        // matches `is_git_dirty` in the CLI and `GitSnapshot::capture`
        // inside the bundler, closing the previous three-site
        // divergence where env.json could silently claim "clean" when
        // git failed.
        git_dirty: is_dirty_or_unknown(),
        in_nix_shell: in_nix_shell(),
        tools,
        nav_env,
        llvm_version,
        host,
        cargo_lock_hash,
        rust_toolchain_toml,
        rustflags,
        target_triple,
        tool_prerelease: TOOL_IS_PRERELEASE,
    })
}

/// Compile-time pre-release flag derived from
/// `env!("CARGO_PKG_VERSION")`. Per semver §9, a pre-release
/// version is denoted by a `-` after the `MAJOR.MINOR.PATCH`
/// triple (`1.0.0-alpha.1`, `0.1.0-pre.1`). Build metadata
/// (`1.0.0+build.42`) uses `+` and is NOT pre-release. The
/// byte-scan below flips `true` on the first `-`, missing `+`
/// by design.
///
/// `const` so the value is baked into the binary at compile
/// time — no runtime parsing surface for fuzzing, and the
/// auditor inspecting a release binary can confirm the flag by
/// disassembly.
pub const TOOL_IS_PRERELEASE: bool = is_prerelease_version(env!("CARGO_PKG_VERSION"));

/// `const fn` byte-scan for a `-` character — the semver
/// pre-release marker. `str::contains('-')` isn't `const` on
/// stable 1.95, hence the byte loop.
pub const fn is_prerelease_version(version: &str) -> bool {
    let bytes = version.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'-' {
            return true;
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod prerelease_tests {
    use super::is_prerelease_version;

    #[test]
    fn classifies_semver_shapes() {
        // Pre-release shapes — any `-` suffix fires.
        assert!(is_prerelease_version("0.1.0-pre.1"));
        assert!(is_prerelease_version("0.0.1-alpha.2"));
        assert!(is_prerelease_version("1.0.0-rc.3"));
        assert!(is_prerelease_version("2.0.0-dev.20260421"));
        assert!(is_prerelease_version("0.1.0-beta"));

        // Release shapes — no `-` in the identifier.
        assert!(!is_prerelease_version("0.1.0"));
        assert!(!is_prerelease_version("1.0.0"));

        // Build metadata only (no pre-release) — `+` is not `-`.
        assert!(!is_prerelease_version("1.0.0+build.42"));
        assert!(!is_prerelease_version("0.1.0+sha.abc123"));
    }

    #[test]
    fn trailing_dash_counts_as_prerelease() {
        // Not strictly semver-valid but the byte-scan classifies
        // it as pre-release. Pinning the behavior so a future
        // switch to a semver-crate parser doesn't silently shift
        // edge cases.
        assert!(is_prerelease_version("1.0.0-"));
    }

    #[test]
    fn empty_and_plain_version_are_release() {
        assert!(!is_prerelease_version(""));
        assert!(!is_prerelease_version("1"));
        assert!(!is_prerelease_version("1.2.3"));
    }
}

/// Check if running inside a Nix shell.
pub fn in_nix_shell() -> bool {
    std::env::var("IN_NIX_SHELL").is_ok()
}

/// Extract LLVM version from `rustc -vV` output.
pub fn extract_llvm_version() -> Option<String> {
    let output = cmd_stdout("rustc", &["-vV"]).ok()?;
    for line in output.lines() {
        if let Some(ver) = line.strip_prefix("LLVM version: ") {
            return Some(ver.trim().to_string());
        }
    }
    None
}

/// Extract the host target triple from `rustc -vV` output.
pub fn extract_target_triple() -> Option<String> {
    let output = cmd_stdout("rustc", &["-vV"]).ok()?;
    for line in output.lines() {
        if let Some(triple) = line.strip_prefix("host: ") {
            return Some(triple.trim().to_string());
        }
    }
    None
}

/// Check if a tool exists by running it with the given arguments.
pub fn tool_exists(prog: &str, args: &[&str]) -> bool {
    Command::new(prog)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
mod tests {
    use super::*;

    #[test]
    fn test_in_nix_shell_detection() {
        // This test just verifies the function runs without panic
        let _ = in_nix_shell();
    }
}
