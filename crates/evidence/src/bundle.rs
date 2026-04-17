//! Evidence bundle creation and management.
//!
//! This module handles the creation and manipulation of evidence bundles
//! that capture build artifacts, hashes, and metadata for certification compliance.

use anyhow::{Context, Result, bail};
use hmac::{Hmac, Mac};
use log;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::git::{GitSnapshot, RealGitProvider};
use crate::hash::{hash_file_into, hash_file_relative_into, write_sha256sums};
use crate::policy::{Dal, Profile};
use crate::traits::GitProvider;

type HmacSha256 = Hmac<Sha256>;

// ============================================================================
// Command Recording
// ============================================================================

/// Record of a command execution for evidence.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommandRecord {
    /// Command and arguments
    pub argv: Vec<String>,
    /// Working directory
    pub cwd: String,
    /// Exit code
    pub exit_code: i32,
    /// Path to stdout file (relative to bundle)
    pub stdout_path: Option<String>,
    /// Path to stderr file (relative to bundle)
    pub stderr_path: Option<String>,
}

// ============================================================================
// Test Summary
// ============================================================================

/// Parsed summary of `cargo test` output.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TestSummary {
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub ignored: u32,
    pub filtered_out: u32,
}

/// Parse cargo test result lines into an accumulated `TestSummary`.
///
/// In a workspace, `cargo test` produces multiple `test result:` lines
/// (one per crate). This function accumulates ALL of them to avoid
/// silently discarding failures in later crates.
///
/// Normalizes `\r\n` → `\n` on entry so output captured from a Windows
/// cargo run (which terminates lines with CRLF) is parsed the same way
/// as Linux/macOS output — a stray trailing `\r` would otherwise break
/// the `trim_end_matches(';')` / split-by-space tokenization on the
/// last segment of every line.
///
/// Returns `None` if no matching line is found.
pub fn parse_cargo_test_output(output: &str) -> Option<TestSummary> {
    let output = output.replace("\r\n", "\n");

    let mut total_passed = 0u64;
    let mut total_failed = 0u64;
    let mut total_ignored = 0u64;
    let mut total_filtered_out = 0u64;
    let mut found = false;

    for line in output.lines() {
        let line = line.trim();
        if !line.starts_with("test result:") {
            continue;
        }
        let after_prefix = if let Some(rest) = line.strip_prefix("test result: ok. ") {
            rest
        } else if let Some(rest) = line.strip_prefix("test result: FAILED. ") {
            rest
        } else {
            continue;
        };

        found = true;

        for segment in after_prefix.split(';') {
            let segment = segment.trim().trim_end_matches(';');
            let parts: Vec<&str> = segment.splitn(2, ' ').collect();
            if parts.len() != 2 {
                continue;
            }
            let n: u64 = match parts[0].parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            match parts[1].trim() {
                "passed" => total_passed += n,
                "failed" => total_failed += n,
                "ignored" => total_ignored += n,
                "filtered out" => total_filtered_out += n,
                _ => {}
            }
        }
    }

    if !found {
        return None;
    }

    let total = total_passed
        .saturating_add(total_failed)
        .saturating_add(total_ignored)
        .saturating_add(total_filtered_out);

    Some(TestSummary {
        total: total as u32,
        passed: total_passed as u32,
        failed: total_failed as u32,
        ignored: total_ignored as u32,
        filtered_out: total_filtered_out as u32,
    })
}

// ============================================================================
// Captured Output Normalization
// ============================================================================

/// Normalize captured subprocess text output to LF line endings.
///
/// Collapses every `\r\n` pair to a single `\n`. Lone `\r` bytes (e.g.
/// `cargo`'s progress spinners `Compiling …\r`) are deliberately
/// preserved — stripping them would corrupt legitimate carriage-return
/// usage. Lone `\n` bytes pass through unchanged.
///
/// This is a **schema-level tool invariant**: every file written by
/// [`EvidenceBuilder::run_capture`] into the bundle's capture directory
/// flows through this function. It is documented as "Captured Output
/// Normalization" in the README and is not opt-out. Recording raw
/// platform-native line endings would make the same logical test run on
/// Windows and Linux produce different `content_hash` values — a
/// cross-platform determinism leak that defeats the evidence chain.
pub(crate) fn normalize_captured_text(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\r' && raw.get(i + 1) == Some(&b'\n') {
            out.push(b'\n');
            i += 2;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    out
}

// ============================================================================
// HMAC Bundle Signing
// ============================================================================

/// Sign the SHA256SUMS content with HMAC-SHA256 and write `BUNDLE.sig`.
pub fn sign_bundle(bundle_dir: &Path, key: &[u8]) -> Result<PathBuf> {
    let sha256sums_path = bundle_dir.join("SHA256SUMS");
    let content = fs::read(&sha256sums_path).context("reading SHA256SUMS for signing")?;

    let mut mac =
        HmacSha256::new_from_slice(key).map_err(|e| anyhow::anyhow!("HMAC key error: {}", e))?;
    mac.update(&content);
    let result = mac.finalize();
    let sig_hex = hex::encode(result.into_bytes());

    let sig_path = bundle_dir.join("BUNDLE.sig");
    fs::write(&sig_path, &sig_hex).context("writing BUNDLE.sig")?;
    Ok(sig_path)
}

/// Verify the HMAC signature in `BUNDLE.sig` against SHA256SUMS content.
///
/// Returns `Ok(true)` if valid, `Ok(false)` if invalid, or an error on I/O failure.
pub fn verify_bundle_signature(bundle_dir: &Path, key: &[u8]) -> Result<bool> {
    let sha256sums_path = bundle_dir.join("SHA256SUMS");
    let sig_path = bundle_dir.join("BUNDLE.sig");

    let content = fs::read(&sha256sums_path).context("reading SHA256SUMS for verification")?;
    let sig_hex = fs::read_to_string(&sig_path).context("reading BUNDLE.sig for verification")?;
    let sig_hex = sig_hex.trim();

    let expected_bytes = hex::decode(sig_hex).context("BUNDLE.sig contains invalid hex")?;

    let mut mac =
        HmacSha256::new_from_slice(key).map_err(|e| anyhow::anyhow!("HMAC key error: {}", e))?;
    mac.update(&content);

    Ok(mac.verify_slice(&expected_bytes).is_ok())
}

// ============================================================================
// Evidence Index
// ============================================================================

/// Evidence bundle index (index.json).
///
/// Default for `EvidenceIndex::engine_build_source` when deserializing
/// a legacy bundle that predates the field.
fn default_engine_build_source() -> String {
    "unknown".to_string()
}

/// Contains metadata about the evidence bundle including schema versions,
/// timestamps, git state, and file references.
///
/// **Determinism design:** `index.json` is part of the metadata layer and is
/// EXCLUDED from SHA256SUMS. The `content_hash` field records the SHA-256 of
/// the SHA256SUMS file itself, which covers only the deterministic content
/// layer. Two runs on the same commit produce identical `content_hash` values
/// even though `timestamp_rfc3339` differs.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EvidenceIndex {
    /// Evidence schema version
    pub schema_version: String,
    /// Boundary config schema version
    pub boundary_schema_version: String,
    /// Trace schema version
    pub trace_schema_version: String,
    /// Active profile name
    pub profile: String,
    /// Bundle creation timestamp (RFC3339)
    pub timestamp_rfc3339: String,
    /// Git commit SHA
    pub git_sha: String,
    /// Git branch name
    pub git_branch: String,
    /// Whether git was dirty at bundle time
    pub git_dirty: bool,
    /// Evidence engine crate version
    pub engine_crate_version: String,
    /// Evidence engine commit SHA or release-version placeholder.
    ///
    /// When `engine_build_source == "git"` this is a 40-char hex SHA
    /// captured either by `build.rs`' `git rev-parse HEAD` or by an
    /// explicit `EVIDENCE_ENGINE_GIT_SHA` override at build time (CI
    /// publish path: `${GITHUB_SHA}`). When
    /// `engine_build_source == "release"` this is `release-v<version>`,
    /// embedded when no git metadata was reachable — typical of
    /// crates.io tarball builds. `"unknown"` only appears in legacy
    /// bundles written before `engine_build_source` existed.
    pub engine_git_sha: String,
    /// Origin of `engine_git_sha`: `"git"` | `"release"` | `"unknown"`.
    ///
    /// Every `EvidenceBuilder` populates this to `"git"` or `"release"`;
    /// `#[serde(default)]` returns `"unknown"` when deserializing a
    /// legacy bundle that predates the field so older fixtures still
    /// load. `verify` cross-checks the pair (source, sha) to catch a
    /// build that e.g. claims `"git"` but embeds a non-40-hex value.
    #[serde(default = "default_engine_build_source")]
    pub engine_build_source: String,
    /// Path to inputs hashes file
    pub inputs_hashes_file: String,
    /// Path to outputs hashes file
    pub outputs_hashes_file: String,
    /// Path to commands file
    pub commands_file: String,
    /// Path to environment fingerprint file
    pub env_fingerprint_file: String,
    /// Trace roots that were scanned
    pub trace_roots: Vec<String>,
    /// Generated trace output files
    pub trace_outputs: Vec<String>,
    /// Whether the bundle is complete
    pub bundle_complete: bool,
    /// SHA-256 of the SHA256SUMS file.
    ///
    /// Covers every byte in the content layer (all files except
    /// `index.json` and `SHA256SUMS` itself, plus `BUNDLE.sig` when
    /// present). Reproducible across runs **on the same host** for
    /// the same commit and inputs; differs across hosts because
    /// `env.json` records host identity (host.os, libc, tools). For
    /// cross-host equality see `deterministic_hash`.
    pub content_hash: String,
    /// SHA-256 of `deterministic-manifest.json`.
    ///
    /// The committed manifest is a projection of `env.json` down to
    /// fields that are cross-host stable (toolchain, target triple,
    /// source identity). Bundles built from the same commit with the
    /// same `rust-toolchain.toml` on Linux, macOS, and Windows share
    /// this hash. This is the tool's cross-host reproducibility
    /// contract, running alongside the full-content `content_hash`
    /// which stays in `SHA256SUMS` for audit-chain integrity.
    pub deterministic_hash: String,
    /// Parsed test results summary, if cargo test was executed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_summary: Option<TestSummary>,
    /// Per-crate DAL assignments. Key is crate name, value is DAL level string.
    /// Empty map for bundles generated before DAL support was added.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dal_map: BTreeMap<String, String>,
}

// ============================================================================
// Evidence Builder
// ============================================================================

/// Configuration for evidence bundle generation.
#[derive(Debug, Clone)]
pub struct EvidenceBuildConfig {
    /// Output directory for bundles
    pub output_root: PathBuf,
    /// Active profile (type-safe enum, not a free-form string)
    pub profile: Profile,
    /// Crates in scope for certification
    pub in_scope_crates: Vec<String>,
    /// Trace roots to scan
    pub trace_roots: Vec<String>,
    /// Whether to require clean git
    pub require_clean_git: bool,
    /// Whether to fail on dirty git
    pub fail_on_dirty: bool,
    /// Resolved per-crate DAL map (crate_name -> Dal).
    pub dal_map: BTreeMap<String, Dal>,
}

/// Builder for creating evidence bundles.
pub struct EvidenceBuilder {
    config: EvidenceBuildConfig,
    git_snapshot: GitSnapshot,
    git_provider: Box<dyn GitProvider>,
    bundle_dir: PathBuf,
    commands: Vec<CommandRecord>,
    inputs: BTreeMap<String, String>,
    outputs: BTreeMap<String, String>,
    test_summary: Option<TestSummary>,
}

impl EvidenceBuilder {
    /// Create a new evidence builder with the given configuration.
    ///
    /// Uses the real git provider. For testing, use [`new_with_provider`].
    pub fn new(config: EvidenceBuildConfig) -> Result<Self> {
        Self::new_with_provider(config, RealGitProvider)
    }

    /// Create a new evidence builder with a custom git provider.
    ///
    /// The provider is used both for the initial git snapshot and for the
    /// TOCTOU re-check at [`finalize`] time.
    pub fn new_with_provider<G: GitProvider + 'static>(
        config: EvidenceBuildConfig,
        provider: G,
    ) -> Result<Self> {
        // Determine strict mode from profile (cert and record require strictness)
        let strict = matches!(config.profile, Profile::Cert | Profile::Record);

        // Snapshot git state at the START (strict mode for cert/record)
        let git_snapshot = GitSnapshot::capture_with(&provider, strict)?;

        // Check for shallow clone (shared with CLI's preflight).
        crate::git::check_shallow_clone()?;

        // Check git clean requirements
        if (config.require_clean_git || config.fail_on_dirty) && git_snapshot.dirty {
            let dirty_files = provider.dirty_files().unwrap_or_default();
            let file_list = if dirty_files.is_empty() {
                String::new()
            } else {
                let capped: Vec<_> = dirty_files.iter().take(10).cloned().collect();
                let suffix = if dirty_files.len() > 10 {
                    format!("\n  ... and {} more", dirty_files.len() - 10)
                } else {
                    String::new()
                };
                format!(
                    "\n\nDirty files:\n  {}{}\n\nTo fix:\n  git add <files> && git commit -m \"...\"\n\nTo override (dev only):\n  cargo xtask evidence --profile dev",
                    capped.join("\n  "),
                    suffix
                )
            };
            bail!(
                "profile '{}' requires clean git tree{}",
                config.profile,
                file_list
            );
        }

        // Create bundle directory with profile prefix, timestamp, and SHA.
        // Format: <profile>-<YYYYMMDD-HHMMSSZ>-<sha8>
        // The profile prefix makes it visually obvious which profile generated
        // the bundle and prevents accidental submission of dev bundles as cert.
        let ts = utc_compact_stamp()?;
        let sha_short = if git_snapshot.sha.len() >= 8 {
            &git_snapshot.sha[..8]
        } else {
            &git_snapshot.sha
        };
        let bundle_dir = config
            .output_root
            .join(format!("{}-{}-{}", config.profile, ts, sha_short));

        // FIX 4: Refuse to overwrite an existing bundle directory.
        if bundle_dir.exists() {
            bail!(
                "Bundle directory {:?} already exists. Remove it first or use a different --out-dir.",
                bundle_dir
            );
        }

        fs::create_dir_all(&bundle_dir).with_context(|| format!("Creating {:?}", bundle_dir))?;
        fs::create_dir_all(bundle_dir.join("tests"))?;
        fs::create_dir_all(bundle_dir.join("trace"))?;

        Ok(Self {
            config,
            git_snapshot,
            git_provider: Box::new(provider),
            bundle_dir,
            commands: Vec::new(),
            inputs: BTreeMap::new(),
            outputs: BTreeMap::new(),
            test_summary: None,
        })
    }

    /// Get the bundle directory path.
    pub fn bundle_dir(&self) -> &Path {
        &self.bundle_dir
    }

    /// Hash a file and add to inputs.
    pub fn hash_input(&mut self, path: &str) -> Result<()> {
        hash_file_into(&mut self.inputs, path)
    }

    /// Hash a file with relative path and add to outputs.
    pub fn hash_output(&mut self, path: &Path) -> Result<()> {
        hash_file_relative_into(&mut self.outputs, path, &self.bundle_dir)
    }

    /// Record a command execution.
    pub fn record_command(&mut self, record: CommandRecord) {
        self.commands.push(record);
    }

    /// Run a command, capture its output, and write stdout/stderr to the bundle.
    pub fn run_capture(
        &mut self,
        mut cmd: Command,
        rel_dir: &str,
        output_name_base: &str,
        display_name: &str,
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        let cwd = std::env::current_dir()?.display().to_string();
        let argv = {
            let mut v = Vec::new();
            v.push(cmd.get_program().to_string_lossy().to_string());
            v.extend(cmd.get_args().map(|a| a.to_string_lossy().to_string()));
            v
        };

        log::info!("evidence: running {}...", display_name);
        let output = cmd
            .output()
            .with_context(|| format!("Running {}", display_name))?;
        let exit_code = output.status.code().unwrap_or(-1);

        if !output.status.success() {
            log::error!("{} failed with exit code {}", display_name, exit_code);
            log::error!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        }

        let (stdout_path, stderr_path) = if rel_dir.is_empty() {
            (
                Some(format!("{}.json", output_name_base)),
                Some(format!("{}_stderr.txt", output_name_base)),
            )
        } else {
            (
                Some(format!("{}/{}_stdout.txt", rel_dir, output_name_base)),
                Some(format!("{}/{}_stderr.txt", rel_dir, output_name_base)),
            )
        };

        // Captured text is CRLF→LF normalized before being written so the
        // same logical run on Windows and Linux produces byte-identical
        // files (and therefore a stable content_hash). See README
        // "Captured Output Normalization" for the user-facing contract.
        let stdout_norm = normalize_captured_text(&output.stdout);
        let stderr_norm = normalize_captured_text(&output.stderr);

        if let Some(ref sp) = stdout_path {
            let abs = self.bundle_dir.join(sp);
            if let Some(parent) = abs.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&abs, &stdout_norm)
                .with_context(|| format!("Writing stdout to {:?}", abs))?;
        }
        if let Some(ref ep) = stderr_path {
            let abs = self.bundle_dir.join(ep);
            if let Some(parent) = abs.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&abs, &stderr_norm)
                .with_context(|| format!("Writing stderr to {:?}", abs))?;
        }

        let rec = CommandRecord {
            argv,
            cwd,
            exit_code,
            stdout_path,
            stderr_path,
        };

        self.commands.push(rec);
        // Return the normalized bytes so callers see exactly what the
        // bundle recorded — avoids subtle Windows/Linux divergence in
        // downstream parsers like `parse_cargo_test_output`.
        Ok((stdout_norm, stderr_norm))
    }

    /// Write the inputs hashes file.
    pub fn write_inputs(&self) -> Result<PathBuf> {
        let path = self.bundle_dir.join("inputs_hashes.json");
        fs::write(&path, serde_json::to_vec_pretty(&self.inputs)?)?;
        Ok(path)
    }

    /// Write the outputs hashes file.
    pub fn write_outputs(&self) -> Result<PathBuf> {
        let path = self.bundle_dir.join("outputs_hashes.json");
        fs::write(&path, serde_json::to_vec_pretty(&self.outputs)?)?;
        Ok(path)
    }

    /// Write the commands file.
    pub fn write_commands(&self) -> Result<PathBuf> {
        let path = self.bundle_dir.join("commands.json");
        fs::write(&path, serde_json::to_vec_pretty(&self.commands)?)?;
        Ok(path)
    }

    /// Store test results for inclusion in the evidence index.
    pub fn set_test_summary(&mut self, summary: TestSummary) {
        self.test_summary = Some(summary);
    }

    /// Pass/fail verdict derived from the stored `TestSummary`.
    ///
    /// - `None` when no test run was recorded (`cargo test` was
    ///   skipped, the command failed to execute, or
    ///   `parse_cargo_test_output` could not find a result line).
    /// - `Some(true)` when `failed == 0`.
    /// - `Some(false)` when any test failed.
    ///
    /// Note the asymmetry with "tests present": a summary with
    /// `total == 0` reports `Some(true)` — there were no failures
    /// because there were no tests. Callers that care about the
    /// distinction should check `test_summary` directly.
    pub fn tests_passed(&self) -> Option<bool> {
        self.test_summary.as_ref().map(|s| s.failed == 0)
    }

    /// Finalize the bundle by writing SHA256SUMS (content layer) then index.json (metadata layer).
    ///
    /// The two-layer design ensures determinism:
    /// 1. SHA256SUMS is written first, covering all content-layer files (everything
    ///    except `index.json` and `SHA256SUMS` itself).
    /// 2. The `content_hash` is the SHA-256 of the SHA256SUMS file contents.
    /// 3. `index.json` is written last with `content_hash` embedded. Because
    ///    `index.json` is excluded from SHA256SUMS, timestamps do not affect
    ///    the content hash.
    pub fn finalize(&self, trace_outputs: Vec<PathBuf>) -> Result<PathBuf> {
        // TOCTOU check: verify git HEAD hasn't changed since builder was created.
        // A changed HEAD means source files may have been modified between the
        // initial snapshot and finalize, invalidating the evidence chain.
        if self.git_snapshot.sha != "unknown" {
            if let Ok(current_sha) = self.git_provider.sha() {
                let current_sha = current_sha.trim().to_string();
                if current_sha != self.git_snapshot.sha {
                    bail!(
                        "TOCTOU: git HEAD changed during evidence generation.\n\
                         Snapshot SHA: {}\n\
                         Current SHA:  {}\n\
                         Source files may have changed. Re-run evidence generation.",
                        self.git_snapshot.sha,
                        current_sha
                    );
                }
            }
        }

        let ts = utc_now_rfc3339()?;
        let sha256sums_path = self.bundle_dir.join("SHA256SUMS");

        // Step 1: Project env.json onto the cross-host-stable subset
        // and write `deterministic-manifest.json`. The manifest is
        // the committed artifact whose hash becomes the cross-host
        // reproducibility contract; writing it before SHA256SUMS is
        // assembled means `write_sha256sums` picks it up for free
        // and the integrity chain binds it like any other content
        // file.
        let env_path = self.bundle_dir.join("env.json");
        let env_bytes = fs::read(&env_path)
            .with_context(|| format!("reading {:?} to build deterministic manifest", env_path))?;
        let env_fp: crate::env::EnvFingerprint = serde_json::from_slice(&env_bytes)
            .context("parsing env.json to derive deterministic manifest")?;
        let manifest = env_fp.deterministic_manifest();
        let manifest_path = self.bundle_dir.join("deterministic-manifest.json");
        fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;

        // Step 2: Write SHA256SUMS covering the content layer only.
        // index.json does not exist yet so it is naturally excluded.
        write_sha256sums(&self.bundle_dir, &sha256sums_path)?;

        // Step 3: Compute full content_hash and the narrower
        // deterministic_hash.
        let content_hash =
            crate::hash::sha256_file(&sha256sums_path).context("hashing SHA256SUMS")?;
        let deterministic_hash = crate::hash::sha256_file(&manifest_path)
            .context("hashing deterministic-manifest.json")?;

        // Step 4: Build and write index.json (metadata layer).
        let idx = EvidenceIndex {
            schema_version: crate::schema_versions::INDEX.to_string(),
            boundary_schema_version: crate::schema_versions::BOUNDARY.to_string(),
            trace_schema_version: crate::schema_versions::TRACE.to_string(),
            profile: self.config.profile.to_string(),
            timestamp_rfc3339: ts,
            git_sha: self.git_snapshot.sha.clone(),
            git_branch: self.git_snapshot.branch.clone(),
            git_dirty: self.git_snapshot.dirty,
            engine_crate_version: env!("CARGO_PKG_VERSION").to_string(),
            engine_git_sha: env!("EVIDENCE_ENGINE_GIT_SHA").to_string(),
            engine_build_source: env!("EVIDENCE_ENGINE_BUILD_SOURCE").to_string(),
            inputs_hashes_file: "inputs_hashes.json".to_string(),
            outputs_hashes_file: "outputs_hashes.json".to_string(),
            commands_file: "commands.json".to_string(),
            env_fingerprint_file: "env.json".to_string(),
            trace_roots: self.config.trace_roots.clone(),
            trace_outputs: trace_outputs
                .iter()
                .map(|p| {
                    crate::util::normalize_bundle_path(
                        p.strip_prefix(&self.bundle_dir).unwrap_or(p),
                    )
                })
                .collect(),
            bundle_complete: true,
            content_hash,
            deterministic_hash,
            test_summary: self.test_summary.clone(),
            dal_map: self
                .config
                .dal_map
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
        };

        let index_path = self.bundle_dir.join("index.json");
        fs::write(&index_path, serde_json::to_vec_pretty(&idx)?)?;

        Ok(self.bundle_dir.clone())
    }
}

// ============================================================================
// Time Helpers
// ============================================================================

/// Get current UTC time in RFC3339 format.
pub fn utc_now_rfc3339() -> Result<String> {
    let now = chrono::Utc::now();
    Ok(now.to_rfc3339())
}

/// Get current UTC time as compact timestamp (YYYYMMDD-HHMMSSZ).
pub fn utc_compact_stamp() -> Result<String> {
    let now = chrono::Utc::now();
    Ok(now.format("%Y%m%d-%H%M%SZ").to_string())
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
    fn test_normalize_captured_text_converts_crlf_to_lf() {
        let input = b"line 1\r\nline 2\r\nline 3\r\n";
        let out = normalize_captured_text(input);
        assert_eq!(out, b"line 1\nline 2\nline 3\n");
    }

    #[test]
    fn test_normalize_captured_text_preserves_lone_cr() {
        // cargo emits `Compiling foo\r` to rewrite a progress line.
        // Stripping lone \r would corrupt that. Only strict CRLF pairs
        // collapse.
        let input = b"Compiling foo\rCompiling bar\r\nok\r\n";
        let out = normalize_captured_text(input);
        assert_eq!(out, b"Compiling foo\rCompiling bar\nok\n");
    }

    #[test]
    fn test_normalize_captured_text_passes_lone_lf_through() {
        let input = b"line 1\nline 2\n";
        let out = normalize_captured_text(input);
        assert_eq!(out, b"line 1\nline 2\n");
    }

    #[test]
    fn test_normalize_captured_text_empty_input() {
        assert_eq!(normalize_captured_text(b""), b"");
    }

    #[test]
    fn test_normalize_captured_text_trailing_cr_without_lf() {
        // A trailing \r with no following \n is kept — there is no CRLF
        // pair to collapse. Matches "lone \r preserved".
        let input = b"abc\r";
        let out = normalize_captured_text(input);
        assert_eq!(out, b"abc\r");
    }

    #[test]
    fn test_normalize_captured_text_mixed_content() {
        let input = b"header\r\n\rspinner\rdone\r\nfooter";
        let out = normalize_captured_text(input);
        assert_eq!(out, b"header\n\rspinner\rdone\nfooter");
    }

    #[test]
    fn test_parse_cargo_test_output_handles_crlf_line_endings() {
        // Windows `Command::output()` captures cargo test with CRLF line
        // endings; the parser must normalize before tokenizing or the
        // trailing `\r` on each `; N filtered out` segment breaks the
        // "filtered out" match.
        let crlf =
            "test result: ok. 3 passed; 1 failed; 2 ignored; 0 filtered out; finished in 0.01s\r\n";
        let summary = parse_cargo_test_output(crlf).expect("should parse CRLF output");
        assert_eq!(summary.passed, 3);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.ignored, 2);
        assert_eq!(summary.filtered_out, 0);
        assert_eq!(summary.total, 6);
    }

    #[test]
    fn test_command_record_fields() {
        let rec = CommandRecord {
            argv: vec!["cargo".to_string(), "test".to_string()],
            cwd: "/project".to_string(),
            exit_code: 0,
            stdout_path: Some("stdout.txt".to_string()),
            stderr_path: Some("stderr.txt".to_string()),
        };
        assert_eq!(rec.exit_code, 0);
        assert_eq!(rec.argv.len(), 2);
    }

    #[test]
    fn test_evidence_index_fields() {
        let idx = EvidenceIndex {
            schema_version: crate::schema_versions::INDEX.to_string(),
            boundary_schema_version: crate::schema_versions::BOUNDARY.to_string(),
            trace_schema_version: crate::schema_versions::TRACE.to_string(),
            profile: "cert".to_string(),
            timestamp_rfc3339: "2024-01-01T00:00:00Z".to_string(),
            git_sha: "abc123".to_string(),
            git_branch: "main".to_string(),
            git_dirty: false,
            engine_crate_version: "0.1.0".to_string(),
            engine_git_sha: "abc123".to_string(),
            engine_build_source: "git".to_string(),
            inputs_hashes_file: "inputs_hashes.json".to_string(),
            outputs_hashes_file: "outputs_hashes.json".to_string(),
            commands_file: "commands.json".to_string(),
            env_fingerprint_file: "env.json".to_string(),
            trace_roots: vec!["cert/trace".to_string()],
            trace_outputs: vec!["trace/matrix.md".to_string()],
            bundle_complete: true,
            content_hash: "deadbeef".repeat(8),
            deterministic_hash: "cafebabe".repeat(8),
            test_summary: None,
            dal_map: BTreeMap::new(),
        };
        assert!(idx.bundle_complete);
        assert_eq!(idx.profile, "cert");
        assert_eq!(idx.content_hash.len(), 64);
    }

    #[test]
    fn test_utc_compact_stamp_format() {
        let stamp = utc_compact_stamp().unwrap();
        // Format: YYYYMMDD-HHMMSSZ
        assert!(stamp.ends_with('Z'));
        assert!(stamp.contains('-'));
        assert_eq!(stamp.len(), 16);
    }

    #[test]
    fn test_parse_cargo_test_output_ok() {
        let output = "\
running 20 tests
test foo ... ok
test result: ok. 20 passed; 0 failed; 1 ignored; 0 measured; 3 filtered out; finished in 0.5s
";
        let summary = parse_cargo_test_output(output).expect("should parse");
        assert_eq!(summary.passed, 20);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.ignored, 1);
        assert_eq!(summary.filtered_out, 3);
        assert_eq!(summary.total, 24);
    }

    #[test]
    fn test_parse_cargo_test_output_failed() {
        let output =
            "test result: FAILED. 18 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out";
        let summary = parse_cargo_test_output(output).expect("should parse");
        assert_eq!(summary.passed, 18);
        assert_eq!(summary.failed, 2);
        assert_eq!(summary.total, 20);
    }

    #[test]
    fn test_parse_cargo_test_output_no_match() {
        let output = "compiling something\nfinished dev";
        assert!(parse_cargo_test_output(output).is_none());
    }

    #[test]
    fn test_hmac_sign_and_verify() {
        let dir = tempfile::tempdir().unwrap();
        let sha256sums_path = dir.path().join("SHA256SUMS");
        fs::write(&sha256sums_path, "abc123  file.txt\n").unwrap();

        let key = b"test-secret-key-bytes";
        let sig_path = sign_bundle(dir.path(), key).unwrap();
        assert!(sig_path.exists());

        // Verify with correct key
        assert!(verify_bundle_signature(dir.path(), key).unwrap());

        // Verify with wrong key
        assert!(!verify_bundle_signature(dir.path(), b"wrong-key").unwrap());
    }
}
