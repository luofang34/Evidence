//! Build environment capture and representation.
//!
//! This module provides functionality to capture the build environment
//! including toolchain versions, environment variables, and system info.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::process::Command;

use crate::git::{git_branch, git_sha, is_dirty_or_unknown};
use crate::util::cmd_stdout;

// ============================================================================
// Host Platform Info
// ============================================================================

/// Per-OS host description.
///
/// Replaces three loosely-coupled string fields (`host_os`, `host_arch`,
/// `libc_version`) with one tagged enum that forces each platform to
/// name the shape of its evidence. DO-178C / DO-330 audit wants the
/// schema to be unambiguous about what "libc" even means on Windows;
/// this enum answers that structurally rather than with Option<String>
/// sentinels.
///
/// Serialized as `{"os": "linux" | "macos" | "windows", …variant fields…}`.
///
/// `target_triple` (the Rust build target, e.g. `aarch64-apple-darwin`)
/// is intentionally **not** folded in here — a Linux host
/// cross-compiling to a bare-metal target has distinct host and target
/// attributes, and merging them would lose evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "os", rename_all = "lowercase")]
pub enum Host {
    /// Linux host.
    Linux {
        /// CPU architecture (e.g. `"x86_64"`, `"aarch64"`).
        arch: String,
        /// Best-effort libc identifier (e.g. `"ldd (GNU libc) 2.31"`, `"musl"`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        libc: Option<String>,
        /// Kernel version from `uname -r`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kernel: Option<String>,
    },
    /// macOS host.
    Macos {
        /// CPU architecture (e.g. `"aarch64"`, `"x86_64"`).
        arch: String,
        /// Product version from `sw_vers -productVersion`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    /// Windows host.
    Windows {
        /// CPU architecture (e.g. `"x86_64"`, `"aarch64"`).
        arch: String,
        /// Raw `ver` command output, e.g. `"10.0.22000.2538"`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        /// Parsed build number from the version string (e.g. `22000`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        build: Option<u32>,
    },
}

impl Host {
    /// Capture the current host's platform description.
    ///
    /// Uses compile-time OS dispatch; adding a new target_os requires
    /// adding a matching variant. That's intentional — an unknown host
    /// slipping into a cert bundle as a stringly-typed tuple is exactly
    /// the kind of silent degradation we're trying to eliminate.
    pub fn detect() -> Self {
        let arch = std::env::consts::ARCH.to_string();
        #[cfg(target_os = "linux")]
        {
            Host::Linux {
                arch,
                libc: detect_libc_linux(),
                kernel: detect_kernel_linux(),
            }
        }
        #[cfg(target_os = "macos")]
        {
            Host::Macos {
                arch,
                version: detect_macos_version(),
            }
        }
        #[cfg(target_os = "windows")]
        {
            let version = detect_windows_version();
            let build = version.as_deref().and_then(parse_windows_build);
            Host::Windows {
                arch,
                version,
                build,
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            compile_error!(
                "Host::detect supports linux, macos, windows; add a variant \
                 if you need to target another OS"
            )
        }
    }
}

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
    /// Host platform description (per-OS shape).
    ///
    /// Replaces the former `host_os` / `host_arch` / `libc_version`
    /// trio; `target_triple` remains a sibling field because it
    /// describes the Rust build target, not the host.
    pub host: Host,
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
        profile: profile.to_string(),
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

/// Best-effort libc identifier on Linux.
#[cfg(target_os = "linux")]
fn detect_libc_linux() -> Option<String> {
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

/// Best-effort Linux kernel version (`uname -r`).
#[cfg(target_os = "linux")]
fn detect_kernel_linux() -> Option<String> {
    cmd_stdout("uname", &["-r"])
        .ok()
        .map(|s| s.trim().to_string())
}

/// macOS product version (`sw_vers -productVersion`).
#[cfg(target_os = "macos")]
fn detect_macos_version() -> Option<String> {
    cmd_stdout("sw_vers", &["-productVersion"])
        .ok()
        .map(|s| s.trim().to_string())
}

/// Windows version string from `cmd /c ver`.
///
/// Typical output: `"Microsoft Windows [Version 10.0.22000.2538]"`.
/// Returns the substring inside the brackets when present, otherwise
/// the trimmed full line.
#[cfg(target_os = "windows")]
fn detect_windows_version() -> Option<String> {
    let out = cmd_stdout("cmd", &["/c", "ver"]).ok()?;
    let line = out.lines().find(|l| !l.trim().is_empty())?.trim();
    if let Some(start) = line.find('[') {
        if let Some(end) = line[start..].find(']') {
            let inside = &line[start + 1..start + end];
            let stripped = inside.trim_start_matches("Version ").trim();
            return Some(stripped.to_string());
        }
    }
    Some(line.to_string())
}

/// Parse the Windows build number from a version string like
/// `"10.0.22000.2538"`. Returns the third dotted component.
#[cfg(target_os = "windows")]
fn parse_windows_build(version: &str) -> Option<u32> {
    version.split('.').nth(2).and_then(|s| s.parse().ok())
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
            host: Host::Linux {
                arch: "x86_64".to_string(),
                libc: Some("glibc 2.31".to_string()),
                kernel: Some("5.15.0-89-generic".to_string()),
            },
            cargo_lock_hash: None,
            rust_toolchain_toml: None,
            rustflags: None,
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
        };
        assert_eq!(fp.profile, "test");
        assert!(!fp.git_dirty);
        assert_eq!(fp.target_triple, "x86_64-unknown-linux-gnu");
        assert!(matches!(fp.host, Host::Linux { .. }));
    }

    #[test]
    fn test_host_linux_roundtrip() {
        let host = Host::Linux {
            arch: "x86_64".to_string(),
            libc: Some("glibc 2.31".to_string()),
            kernel: Some("5.15.0".to_string()),
        };
        let json = serde_json::to_string(&host).expect("serialize");
        assert_eq!(
            json,
            r#"{"os":"linux","arch":"x86_64","libc":"glibc 2.31","kernel":"5.15.0"}"#
        );
        let back: Host = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, host);
    }

    #[test]
    fn test_host_macos_roundtrip() {
        let host = Host::Macos {
            arch: "aarch64".to_string(),
            version: Some("14.2.1".to_string()),
        };
        let json = serde_json::to_string(&host).expect("serialize");
        assert_eq!(
            json,
            r#"{"os":"macos","arch":"aarch64","version":"14.2.1"}"#
        );
        let back: Host = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, host);
    }

    #[test]
    fn test_host_windows_roundtrip() {
        let host = Host::Windows {
            arch: "x86_64".to_string(),
            version: Some("10.0.22000.2538".to_string()),
            build: Some(22000),
        };
        let json = serde_json::to_string(&host).expect("serialize");
        assert_eq!(
            json,
            r#"{"os":"windows","arch":"x86_64","version":"10.0.22000.2538","build":22000}"#
        );
        let back: Host = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, host);
    }

    #[test]
    fn test_host_optional_fields_omitted_when_none() {
        let host = Host::Linux {
            arch: "x86_64".to_string(),
            libc: None,
            kernel: None,
        };
        let json = serde_json::to_string(&host).expect("serialize");
        // None fields must be omitted entirely, not serialized as null,
        // so bundles stay byte-stable when detection fails.
        assert_eq!(json, r#"{"os":"linux","arch":"x86_64"}"#);
    }
}
