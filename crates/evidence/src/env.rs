//! Build environment capture and representation.
//!
//! This module provides functionality to capture the build environment
//! including toolchain versions, environment variables, and system info.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::process::Command;

use crate::git::{git_branch, git_dirty, git_sha};
use crate::util::cmd_stdout;

/// Complete build environment fingerprint.
///
/// Captures all relevant environment information for reproducibility
/// verification and evidence generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvFingerprint {
    /// Active profile name
    pub profile: String,
    /// rustc version string
    pub rustc: String,
    /// cargo version string
    pub cargo: String,
    /// Current git commit SHA
    pub git_sha: String,
    /// Current git branch
    pub git_branch: String,
    /// Whether git working directory is dirty
    pub git_dirty: bool,
    /// Whether running in a Nix shell
    pub in_nix_shell: bool,
    /// Map of tool name to availability
    pub tools: BTreeMap<String, bool>,
    /// NAV_* environment variables
    pub nav_env: BTreeMap<String, String>,
    /// LLVM version from rustc (for platform capsule)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llvm_version: Option<String>,
    /// Host operating system
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_os: Option<String>,
    /// Host CPU architecture
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_arch: Option<String>,
    /// libc version (platform-specific)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub libc_version: Option<String>,
    /// SHA-256 of Cargo.lock if present in the workspace root
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cargo_lock_hash: Option<String>,
    /// Contents of rust-toolchain.toml if present in the workspace root
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust_toolchain_toml: Option<String>,
    /// Value of the RUSTFLAGS environment variable if set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rustflags: Option<String>,
    /// Host target triple from `rustc -vV`
    pub target_triple: String,
}

impl EnvFingerprint {
    /// Capture the current build environment for a given profile.
    ///
    /// When `strict` is true (cert/record profiles), critical tools (rustc,
    /// cargo) must be detectable or an error is raised. This satisfies
    /// cert-mode strict error handling requirements.
    pub fn capture(profile: &str, strict: bool) -> Result<Self> {
        env_fingerprint(profile, strict)
    }
}

/// Capture a complete environment fingerprint.
///
/// When `strict` is true (cert/record profiles), critical tools (rustc, cargo)
/// must be found or the function returns an error. This prevents evidence
/// bundles from being generated in an incomplete environment.
pub fn env_fingerprint(profile: &str, strict: bool) -> Result<EnvFingerprint> {
    let rustc = cmd_stdout("rustc", &["--version"]);
    let cargo = cmd_stdout("cargo", &["--version"]);

    if strict {
        if rustc.is_err() {
            bail!(
                "cert/record profile requires rustc to be installed and on PATH. \
                 rustc --version failed."
            );
        }
        if cargo.is_err() {
            bail!(
                "cert/record profile requires cargo to be installed and on PATH. \
                 cargo --version failed."
            );
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
    let libc_version = detect_libc_version();
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
        profile: profile.to_string(),
        rustc: rustc_str.trim().to_string(),
        cargo: cargo_str.trim().to_string(),
        git_sha: git_sha().unwrap_or_else(|_| "unknown".to_string()),
        git_branch: git_branch().unwrap_or_else(|_| "unknown".to_string()),
        git_dirty: git_dirty().unwrap_or(false),
        in_nix_shell: in_nix_shell(),
        tools,
        nav_env,
        llvm_version,
        host_os: Some(std::env::consts::OS.to_string()),
        host_arch: Some(std::env::consts::ARCH.to_string()),
        libc_version,
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

/// Best-effort libc version detection (platform-specific).
pub fn detect_libc_version() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        // Try ldd --version (glibc) or check for musl
        if let Ok(out) = cmd_stdout("ldd", &["--version"]) {
            // First line typically contains version: "ldd (GNU libc) 2.31"
            if let Some(line) = out.lines().next() {
                return Some(line.trim().to_string());
            }
        }
        // Fallback: check /lib for musl
        if std::path::Path::new("/lib/ld-musl-x86_64.so.1").exists() {
            return Some("musl".to_string());
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        // macOS system version implies libSystem version
        cmd_stdout("sw_vers", &["-productVersion"])
            .ok()
            .map(|s| format!("macOS {}", s.trim()))
    }
    #[cfg(target_os = "windows")]
    {
        // Windows doesn't have a traditional libc; MSVC CRT version is complex to detect
        Some("msvc".to_string())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
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
mod tests {
    use super::*;

    #[test]
    fn test_in_nix_shell_detection() {
        // This test just verifies the function runs without panic
        let _ = in_nix_shell();
    }

    #[test]
    fn test_env_fingerprint_fields() {
        let fp = EnvFingerprint {
            profile: "test".to_string(),
            rustc: "rustc 1.70.0".to_string(),
            cargo: "cargo 1.70.0".to_string(),
            git_sha: "abc123".to_string(),
            git_branch: "main".to_string(),
            git_dirty: false,
            in_nix_shell: false,
            tools: BTreeMap::new(),
            nav_env: BTreeMap::new(),
            llvm_version: Some("16.0.0".to_string()),
            host_os: Some("linux".to_string()),
            host_arch: Some("x86_64".to_string()),
            libc_version: Some("glibc 2.31".to_string()),
            cargo_lock_hash: None,
            rust_toolchain_toml: None,
            rustflags: None,
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
        };
        assert_eq!(fp.profile, "test");
        assert!(!fp.git_dirty);
        assert_eq!(fp.target_triple, "x86_64-unknown-linux-gnu");
    }
}
