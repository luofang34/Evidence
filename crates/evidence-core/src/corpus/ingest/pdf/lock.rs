//! The strict PDF extractor tool-lock record (LLR-179).
//!
//! A [`PdfToolLock`] pins the exact extractor identity the blocking
//! runner may execute: the tool name (exactly `pdftotext`), the
//! exact `-v` version output, the per-platform executable SHA-256
//! digests, the exact pinned argv, the adapter version, and the
//! configuration digest over the argv and adapter version. The
//! record parses from TOML under `deny_unknown_fields` with a
//! closed [`PdfPlatform`] key set, so unknown fields and
//! unsupported platforms are unrepresentable, and validation
//! rejects an empty executable map (an unpinned executable),
//! malformed digests, any argv deviation, and a stale
//! configuration digest.
//!
//! [`PdfToolLock::canonical_bytes`] encodes under the
//! domain-tagged, length-prefixed framing of the other corpus
//! identity records, so [`PdfToolLock::digest`] moves when any
//! field moves. The PDF ingestion recipe binds the whole lock, so
//! any tool, version, digest, argv, or adapter change is a recipe
//! change.
//!
//! [`PdfToolLock::verify_executable`] checks a candidate
//! executable before any document parse: the current platform
//! must be locked, the executable's SHA-256 must equal the locked
//! digest, and the observed `-v` output must equal the locked
//! version string. Every check fails closed with the expected and
//! found values.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::super::super::digest::StructuralContentDigest;
use super::runner;

/// The only tool name a lock may pin (LLR-179).
pub const PDF_TOOL_NAME: &str = "pdftotext";

/// The exact pinned extractor argv, excluding the input and output
/// paths the runner appends (LLR-179). Any change is a recipe
/// change.
pub const PINNED_ARGV: [&str; 7] = [
    "-bbox-layout",
    "-enc",
    "UTF-8",
    "-eol",
    "unix",
    "-cropbox",
    "-q",
];

/// Domain/version tag prefixing the lock encoding.
const LOCK_DOMAIN_TAG: &[u8] = b"evidence/pdf-tool-lock/v1";

/// Domain/version tag prefixing the configuration digest.
const CONFIG_DOMAIN_TAG: &[u8] = b"evidence/pdf-tool-lock-config/v1";

/// The closed set of platforms a tool lock may pin (LLR-179).
/// Each variant is a supported platform artifact; an unsupported
/// platform key fails TOML deserialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PdfPlatform {
    /// Linux on x86-64.
    LinuxX86_64,
    /// macOS on Apple silicon.
    MacosAarch64,
    /// macOS on Intel.
    MacosX86_64,
    /// Windows on x86-64.
    WindowsX86_64,
}

impl PdfPlatform {
    /// The wire key of the platform.
    pub fn as_str(&self) -> &'static str {
        match self {
            PdfPlatform::LinuxX86_64 => "linux_x86_64",
            PdfPlatform::MacosAarch64 => "macos_aarch64",
            PdfPlatform::MacosX86_64 => "macos_x86_64",
            PdfPlatform::WindowsX86_64 => "windows_x86_64",
        }
    }

    /// The platform this build runs on, when it is one of the
    /// supported artifacts; `None` on anything else, which
    /// [`PdfToolLock::verify_executable`] reports as unsupported.
    pub fn current() -> Option<Self> {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            Some(PdfPlatform::LinuxX86_64)
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            Some(PdfPlatform::MacosAarch64)
        }
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        {
            Some(PdfPlatform::MacosX86_64)
        }
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            Some(PdfPlatform::WindowsX86_64)
        }
        #[cfg(not(any(
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "aarch64"),
            all(target_os = "macos", target_arch = "x86_64"),
            all(target_os = "windows", target_arch = "x86_64")
        )))]
        {
            None
        }
    }
}

/// The strict tool-lock record for the PDF extractor (LLR-179).
/// Pure identity data; [`Self::validate`] enforces the schema
/// invariants the type system cannot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfToolLock {
    /// The tool name; must be [`PDF_TOOL_NAME`].
    pub tool_name: String,
    /// The exact first line of the tool's `-v` output
    /// (e.g. `pdftotext version 26.07.0`).
    pub version_output: String,
    /// Per-platform executable SHA-256 digests (64-character
    /// lowercase hex), keyed by the closed platform set. Must be
    /// non-empty: a lock without an executable digest is unpinned.
    pub executable_sha256: BTreeMap<PdfPlatform, String>,
    /// The exact pinned argv; must equal [`PINNED_ARGV`].
    pub argv: Vec<String>,
    /// The adapter version mapping extractor output to candidate
    /// nodes.
    pub adapter_version: String,
    /// The configuration digest: hex SHA-256 over the canonical
    /// framing of the argv and adapter version; recomputed at
    /// validation.
    pub config_digest: String,
}

/// Every fail-closed tool-lock violation (LLR-179).
#[derive(Debug, Error)]
pub enum PdfToolLockError {
    /// The TOML record is malformed or carries an unknown field or
    /// platform key.
    #[error("tool-lock record does not parse: {source}")]
    RecordParse {
        /// The TOML deserialization failure.
        source: Box<toml::de::Error>,
    },
    /// The tool name is not `pdftotext`.
    #[error("tool name {found:?} is not the required {PDF_TOOL_NAME:?}")]
    UnsupportedTool {
        /// The declared tool name.
        found: String,
    },
    /// The version-output string is blank.
    #[error("version output is blank")]
    BlankVersionOutput,
    /// The executable map is empty — the lock pins no executable.
    #[error("tool lock pins no executable digest for any platform")]
    UnpinnedExecutable,
    /// An executable digest is not 64-character lowercase hex.
    #[error("executable digest for {platform} is not 64-character lowercase hex: {found:?}")]
    MalformedDigest {
        /// The platform whose digest is malformed.
        platform: &'static str,
        /// The offending value.
        found: String,
    },
    /// The argv deviates from the pinned argv.
    #[error("argv {found:?} deviates from the pinned argv {PINNED_ARGV:?}")]
    ArgvMismatch {
        /// The declared argv.
        found: Vec<String>,
    },
    /// The configuration digest does not recompute over the argv
    /// and adapter version.
    #[error("configuration digest mismatch: declared {declared}, recomputed {recomputed}")]
    ConfigDigestMismatch {
        /// The declared digest.
        declared: String,
        /// The recomputed digest.
        recomputed: String,
    },
    /// The current platform is not one of the supported artifacts
    /// or carries no locked digest.
    #[error("platform {platform} carries no locked executable digest")]
    UnsupportedPlatform {
        /// The current platform wire key, or `unknown`.
        platform: &'static str,
    },
    /// The candidate executable's SHA-256 differs from the locked
    /// digest.
    #[error("executable digest mismatch: locked {expected}, found {found}")]
    ExecutableDigestMismatch {
        /// The locked digest.
        expected: String,
        /// The digest of the candidate executable.
        found: String,
    },
    /// The candidate executable's `-v` output differs from the
    /// locked version output.
    #[error("version output mismatch: locked {expected:?}, found {found:?}")]
    VersionMismatch {
        /// The locked version output.
        expected: String,
        /// The observed first line of `-v` output.
        found: String,
    },
    /// The candidate executable could not be read or probed.
    #[error("executable probe failed for {path}: {source}")]
    Probe {
        /// The executable path.
        path: String,
        /// The I/O failure.
        source: std::io::Error,
    },
}

impl PdfToolLock {
    /// Parse and validate a tool lock from its TOML wire form
    /// (LLR-179). Unknown fields and unsupported platform keys
    /// fail deserialization; schema invariants fail validation.
    ///
    /// # Errors
    ///
    /// The first violation wins, so error precedence is
    /// deterministic.
    pub fn from_toml(raw: &str) -> Result<Self, PdfToolLockError> {
        let lock: Self = toml::from_str(raw).map_err(|source| PdfToolLockError::RecordParse {
            source: Box::new(source),
        })?;
        lock.validate()?;
        Ok(lock)
    }

    /// Validate the schema invariants in the module docs' order.
    ///
    /// # Errors
    ///
    /// The first violation wins, so error precedence is
    /// deterministic.
    pub fn validate(&self) -> Result<(), PdfToolLockError> {
        if self.tool_name != PDF_TOOL_NAME {
            return Err(PdfToolLockError::UnsupportedTool {
                found: self.tool_name.clone(),
            });
        }
        if self.version_output.trim().is_empty() {
            return Err(PdfToolLockError::BlankVersionOutput);
        }
        if self.executable_sha256.is_empty() {
            return Err(PdfToolLockError::UnpinnedExecutable);
        }
        for (platform, digest) in &self.executable_sha256 {
            if !is_hex_digest(digest) {
                return Err(PdfToolLockError::MalformedDigest {
                    platform: platform.as_str(),
                    found: digest.clone(),
                });
            }
        }
        let pinned: Vec<String> = PINNED_ARGV.iter().map(|arg| (*arg).to_string()).collect();
        if self.argv != pinned {
            return Err(PdfToolLockError::ArgvMismatch {
                found: self.argv.clone(),
            });
        }
        let recomputed = Self::compute_config_digest(&self.argv, &self.adapter_version);
        if self.config_digest != recomputed {
            return Err(PdfToolLockError::ConfigDigestMismatch {
                declared: self.config_digest.clone(),
                recomputed,
            });
        }
        Ok(())
    }

    /// The configuration digest: hex SHA-256 over the canonical
    /// framing of `argv` and `adapter_version` (LLR-179).
    pub fn compute_config_digest(argv: &[String], adapter_version: &str) -> String {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(CONFIG_DOMAIN_TAG);
        bytes.push(0);
        push_count(&mut bytes, argv.len());
        for arg in argv {
            push_str(&mut bytes, arg);
        }
        push_str(&mut bytes, adapter_version);
        crate::hash::sha256(&bytes)
    }

    /// The canonical byte encoding pinned by the module docs.
    /// Pure and host-independent.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(LOCK_DOMAIN_TAG);
        bytes.push(0);
        push_str(&mut bytes, &self.tool_name);
        push_str(&mut bytes, &self.version_output);
        push_count(&mut bytes, self.executable_sha256.len());
        for (platform, digest) in &self.executable_sha256 {
            push_str(&mut bytes, platform.as_str());
            push_str(&mut bytes, digest);
        }
        push_count(&mut bytes, self.argv.len());
        for arg in &self.argv {
            push_str(&mut bytes, arg);
        }
        push_str(&mut bytes, &self.adapter_version);
        push_str(&mut bytes, &self.config_digest);
        bytes
    }

    /// The lock identity: SHA-256 over [`Self::canonical_bytes`],
    /// as the validated structural digest domain.
    pub fn digest(&self) -> StructuralContentDigest {
        StructuralContentDigest::from_hasher_output(crate::hash::sha256(&self.canonical_bytes()))
    }

    /// Verify a candidate executable against the lock before any
    /// document parse (LLR-179): the current platform must carry a
    /// locked digest, the executable bytes must re-digest to it,
    /// and the observed `-v` output must equal the locked version
    /// string.
    ///
    /// # Errors
    ///
    /// Fails closed with the expected and found values on any
    /// mismatch; a read or probe failure is [`PdfToolLockError::Probe`].
    pub fn verify_executable(&self, executable: &Path) -> Result<(), PdfToolLockError> {
        let platform = PdfPlatform::current().ok_or(PdfToolLockError::UnsupportedPlatform {
            platform: "unknown",
        })?;
        let expected =
            self.executable_sha256
                .get(&platform)
                .ok_or(PdfToolLockError::UnsupportedPlatform {
                    platform: platform.as_str(),
                })?;
        let bytes = std::fs::read(executable).map_err(|source| PdfToolLockError::Probe {
            path: executable.display().to_string(),
            source,
        })?;
        let found = crate::hash::sha256(&bytes);
        if &found != expected {
            return Err(PdfToolLockError::ExecutableDigestMismatch {
                expected: expected.clone(),
                found,
            });
        }
        let observed = runner::probe_version_blocking(executable).map_err(|source| {
            PdfToolLockError::Probe {
                path: executable.display().to_string(),
                source,
            }
        })?;
        if observed != self.version_output {
            return Err(PdfToolLockError::VersionMismatch {
                expected: self.version_output.clone(),
                found: observed,
            });
        }
        Ok(())
    }
}

/// A digest string is 64-character lowercase hex.
fn is_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `str(s)` framing: `u64_be` byte length, then the exact UTF-8
/// bytes.
fn push_str(out: &mut Vec<u8>, value: &str) {
    push_count(out, value.len());
    out.extend_from_slice(value.as_bytes());
}

fn push_count(out: &mut Vec<u8>, count: usize) {
    out.extend_from_slice(&(count as u64).to_be_bytes());
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]
#[path = "lock_tests.rs"]
mod tests;
