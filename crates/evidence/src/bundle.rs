//! Evidence bundle creation and management.
//!
//! This module handles the creation and manipulation of evidence bundles
//! that capture build artifacts, hashes, and metadata for certification compliance.

use anyhow::{bail, Context, Result};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::git::GitSnapshot;
use crate::hash::{hash_file_into, hash_file_relative_into, write_sha256sums};

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

/// Parse the standard cargo test result line into a `TestSummary`.
///
/// Looks for a line matching:
/// `test result: ok. N passed; N failed; N ignored; N measured; N filtered out`
/// (or `FAILED` instead of `ok`).
///
/// Returns `None` if no matching line is found.
pub fn parse_cargo_test_output(output: &str) -> Option<TestSummary> {
    for line in output.lines() {
        let line = line.trim();
        if !line.starts_with("test result:") {
            continue;
        }
        // Extract the part after "test result: ok. " or "test result: FAILED. "
        let after_prefix = if let Some(rest) = line.strip_prefix("test result: ok. ") {
            rest
        } else if let Some(rest) = line.strip_prefix("test result: FAILED. ") {
            rest
        } else {
            continue;
        };

        let mut passed = 0u32;
        let mut failed = 0u32;
        let mut ignored = 0u32;
        let mut filtered_out = 0u32;

        for segment in after_prefix.split(';') {
            let segment = segment.trim().trim_end_matches(';');
            let parts: Vec<&str> = segment.splitn(2, ' ').collect();
            if parts.len() != 2 {
                continue;
            }
            let n: u32 = match parts[0].parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            match parts[1].trim() {
                "passed" => passed = n,
                "failed" => failed = n,
                "ignored" => ignored = n,
                "filtered out" => filtered_out = n,
                _ => {}
            }
        }

        let total = passed + failed + ignored + filtered_out;
        return Some(TestSummary {
            total,
            passed,
            failed,
            ignored,
            filtered_out,
        });
    }
    None
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
    let sig_hex =
        fs::read_to_string(&sig_path).context("reading BUNDLE.sig for verification")?;
    let sig_hex = sig_hex.trim();

    let expected_bytes =
        hex::decode(sig_hex).context("BUNDLE.sig contains invalid hex")?;

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
    /// Evidence engine git SHA
    pub engine_git_sha: String,
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
    /// SHA-256 of the SHA256SUMS file (deterministic content hash).
    ///
    /// This hash covers all files in the content layer (everything except
    /// `index.json` and `SHA256SUMS` itself). It is reproducible across
    /// runs on the same commit with the same inputs.
    pub content_hash: String,
    /// Parsed test results summary, if cargo test was executed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_summary: Option<TestSummary>,
}

// ============================================================================
// Evidence Builder
// ============================================================================

/// Configuration for evidence bundle generation.
#[derive(Debug, Clone)]
pub struct EvidenceBuildConfig {
    /// Output directory for bundles
    pub output_root: PathBuf,
    /// Active profile
    pub profile: String,
    /// Crates in scope for certification
    pub in_scope_crates: Vec<String>,
    /// Trace roots to scan
    pub trace_roots: Vec<String>,
    /// Whether to skip running tests
    pub skip_tests: bool,
    /// Whether to require clean git
    pub require_clean_git: bool,
    /// Whether to fail on dirty git
    pub fail_on_dirty: bool,
}

/// Builder for creating evidence bundles.
pub struct EvidenceBuilder {
    config: EvidenceBuildConfig,
    git_snapshot: GitSnapshot,
    bundle_dir: PathBuf,
    commands: Vec<CommandRecord>,
    inputs: BTreeMap<String, String>,
    outputs: BTreeMap<String, String>,
}

impl EvidenceBuilder {
    /// Create a new evidence builder with the given configuration.
    pub fn new(config: EvidenceBuildConfig) -> Result<Self> {
        // Determine strict mode from profile (cert and record require strictness)
        let strict = matches!(config.profile.as_str(), "cert" | "record");

        // Snapshot git state at the START (strict mode for cert/record)
        let git_snapshot = GitSnapshot::capture(strict)?;

        // Check for shallow clone
        if Path::new(".git/shallow").exists() {
            bail!(
                "Shallow clone detected. Evidence generation requires full repository history.\n\
                 Run: git fetch --unshallow"
            );
        }

        // Check git clean requirements
        if (config.require_clean_git || config.fail_on_dirty) && git_snapshot.dirty {
            let dirty_files = crate::git::git_dirty_files().unwrap_or_default();
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
            bundle_dir,
            commands: Vec::new(),
            inputs: BTreeMap::new(),
            outputs: BTreeMap::new(),
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

    /// Run a command and capture its output.
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

        println!("evidence: running {}...", display_name);
        let output = cmd
            .output()
            .with_context(|| format!("Running {}", display_name))?;
        let exit_code = output.status.code().unwrap_or(-1);

        if !output.status.success() {
            eprintln!("{} failed with exit code {}", display_name, exit_code);
            eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
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

        let rec = CommandRecord {
            argv,
            cwd,
            exit_code,
            stdout_path,
            stderr_path,
        };

        self.commands.push(rec);
        Ok((output.stdout, output.stderr))
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

    /// Finalize the bundle by writing SHA256SUMS (content layer) then index.json (metadata layer).
    ///
    /// The two-layer design ensures determinism:
    /// 1. SHA256SUMS is written first, covering all content-layer files (everything
    ///    except `index.json` and `SHA256SUMS` itself).
    /// 2. The `content_hash` is the SHA-256 of the SHA256SUMS file contents.
    /// 3. `index.json` is written last with `content_hash` embedded. Because
    ///    `index.json` is excluded from SHA256SUMS, timestamps do not affect
    ///    the content hash.
    pub fn finalize(
        &self,
        boundary_schema_version: &str,
        trace_schema_version: &str,
        trace_outputs: Vec<PathBuf>,
    ) -> Result<PathBuf> {
        let ts = utc_now_rfc3339()?;
        let sha256sums_path = self.bundle_dir.join("SHA256SUMS");

        // Step 1: Write SHA256SUMS covering the content layer only.
        // index.json does not exist yet so it is naturally excluded.
        write_sha256sums(&self.bundle_dir, &sha256sums_path)?;

        // Step 2: Compute deterministic content hash from SHA256SUMS.
        let content_hash =
            crate::hash::sha256_file(&sha256sums_path).context("hashing SHA256SUMS")?;

        // Step 3: Build and write index.json (metadata layer).
        let idx = EvidenceIndex {
            schema_version: "0.0.1".to_string(),
            boundary_schema_version: boundary_schema_version.to_string(),
            trace_schema_version: trace_schema_version.to_string(),
            profile: self.config.profile.clone(),
            timestamp_rfc3339: ts,
            git_sha: self.git_snapshot.sha.clone(),
            git_branch: self.git_snapshot.branch.clone(),
            git_dirty: self.git_snapshot.dirty,
            engine_crate_version: env!("CARGO_PKG_VERSION").to_string(),
            engine_git_sha: self.git_snapshot.sha.clone(),
            inputs_hashes_file: "inputs_hashes.json".to_string(),
            outputs_hashes_file: "outputs_hashes.json".to_string(),
            commands_file: "commands.json".to_string(),
            env_fingerprint_file: "env.json".to_string(),
            trace_roots: self.config.trace_roots.clone(),
            trace_outputs: trace_outputs
                .iter()
                .map(|p| {
                    p.strip_prefix(&self.bundle_dir)
                        .unwrap_or(p)
                        .to_string_lossy()
                        .to_string()
                })
                .collect(),
            bundle_complete: true,
            content_hash,
            test_summary: None,
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
mod tests {
    use super::*;

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
            schema_version: "0.0.1".to_string(),
            boundary_schema_version: "0.0.1".to_string(),
            trace_schema_version: "0.0.3".to_string(),
            profile: "cert".to_string(),
            timestamp_rfc3339: "2024-01-01T00:00:00Z".to_string(),
            git_sha: "abc123".to_string(),
            git_branch: "main".to_string(),
            git_dirty: false,
            engine_crate_version: "0.1.0".to_string(),
            engine_git_sha: "abc123".to_string(),
            inputs_hashes_file: "inputs_hashes.json".to_string(),
            outputs_hashes_file: "outputs_hashes.json".to_string(),
            commands_file: "commands.json".to_string(),
            env_fingerprint_file: "env.json".to_string(),
            trace_roots: vec!["cert/trace".to_string()],
            trace_outputs: vec!["trace/matrix.md".to_string()],
            bundle_complete: true,
            content_hash: "deadbeef".repeat(8),
            test_summary: None,
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
        let output = "test result: FAILED. 18 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out";
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
