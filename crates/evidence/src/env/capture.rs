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

    // Cargo.lock hash if present
    let cargo_lock_hash = if std::path::Path::new("Cargo.lock").exists() {
        std::fs::read("Cargo.lock")
            .ok()
            .map(|data| crate::hash::sha256(&data))
    } else {
        None
    };

    // rust-toolchain.toml contents if present
    let rust_toolchain_toml = std::fs::read_to_string("rust-toolchain.toml").ok();

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
    })
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
