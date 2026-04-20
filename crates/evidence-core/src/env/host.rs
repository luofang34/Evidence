//! `Host` — per-OS host description.
//!
//! Replaces three loosely-coupled string fields (`host_os`, `host_arch`,
//! `libc_version`) with one tagged enum that forces each platform to
//! name the shape of its evidence. DO-178C / DO-330 audit wants the
//! schema to be unambiguous about what "libc" even means on Windows;
//! this enum answers that structurally rather than with `Option<String>`
//! sentinels.

use serde::{Deserialize, Serialize};

use crate::util::cmd_stdout;

/// Per-OS host description.
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
