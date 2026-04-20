//! Build environment capture and representation.
//!
//! Split across sibling files under `env/`:
//!
//! | Sub-module       | Concern                                                    |
//! |------------------|------------------------------------------------------------|
//! | `host`           | `Host` enum (Linux/macOS/Windows) + per-OS detection fns   |
//! | `manifest`       | `DeterministicManifest` — cross-host reproducibility contract |
//! | `fingerprint`    | `EnvFingerprint` — full env.json struct + projection       |
//! | `capture`        | `env_fingerprint` runtime capture + tool detection helpers |
//!
//! Re-exports below keep the crate's public API flat — consumers
//! continue to `use evidence_core::env::{EnvFingerprint, Host, …}` without
//! caring about the split.

mod capture;
mod fingerprint;
mod host;
mod manifest;

pub use capture::{
    EnvCaptureError, TOOL_IS_PRERELEASE, env_fingerprint, extract_llvm_version,
    extract_target_triple, in_nix_shell, is_prerelease_version, tool_exists,
};
pub use fingerprint::EnvFingerprint;
pub use host::Host;
pub use manifest::DeterministicManifest;
