//! `EvidenceBuilder` — the stateful builder that assembles a bundle.
//!
//! Lifecycle: `new` captures a `GitSnapshot` and creates the bundle
//! directory with a `<profile>-<ts>-<sha8>` name.
//! `hash_input` / `hash_output` / `record_command` / `run_capture`
//! accumulate content-layer state.
//! `finalize` writes `deterministic-manifest.json` + `SHA256SUMS`
//! (content layer) and then `index.json` (metadata layer), with a
//! TOCTOU re-check on `git_sha` so a mid-run repo mutation is caught.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::capture::normalize_captured_text;
use super::command::CommandRecord;
use super::error::BuilderError;
use super::index::EvidenceIndex;
use super::test_summary::TestSummary;
use super::time::{utc_compact_stamp, utc_now_rfc3339};
use crate::git::{GitSnapshot, RealGitProvider};
use crate::hash::{hash_file_into, hash_file_relative_into, write_sha256sums};
use crate::policy::{Dal, Profile};
use crate::traits::GitProvider;

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
    /// Uses the real git provider. For testing, use [`Self::new_with_provider`].
    pub fn new(config: EvidenceBuildConfig) -> Result<Self, BuilderError> {
        Self::new_with_provider(config, RealGitProvider)
    }

    /// Create a new evidence builder with a custom git provider.
    ///
    /// The provider is used both for the initial git snapshot and for the
    /// TOCTOU re-check at [`Self::finalize`] time.
    pub fn new_with_provider<G: GitProvider + 'static>(
        config: EvidenceBuildConfig,
        provider: G,
    ) -> Result<Self, BuilderError> {
        // Determine strict mode from profile (cert and record require strictness)
        let strict = matches!(config.profile, Profile::Cert | Profile::Record);

        // Snapshot git state at the START (strict mode for cert/record)
        let git_snapshot = GitSnapshot::capture_with(&provider, strict)?;

        // Check for shallow clone (shared with CLI's preflight).
        crate::git::check_shallow_clone()?;

        // Check git clean requirements
        if (config.require_clean_git || config.fail_on_dirty) && git_snapshot.dirty {
            let dirty_files = provider.dirty_files().unwrap_or_default();
            let suffix = if dirty_files.is_empty() {
                String::new()
            } else {
                let capped: Vec<_> = dirty_files.iter().take(10).cloned().collect();
                let more = if dirty_files.len() > 10 {
                    format!("\n  ... and {} more", dirty_files.len() - 10)
                } else {
                    String::new()
                };
                format!(
                    "\n\nDirty files:\n  {}{}\n\nTo fix:\n  git add <files> && git commit -m \"...\"\n\nTo override (dev only):\n  cargo xtask evidence --profile dev",
                    capped.join("\n  "),
                    more
                )
            };
            return Err(BuilderError::DirtyGitTree {
                profile: config.profile,
                suffix,
            });
        }

        // Create bundle directory with profile prefix, timestamp, and SHA.
        // Format: <profile>-<YYYYMMDD-HHMMSSZ>-<sha8>
        // The profile prefix makes it visually obvious which profile generated
        // the bundle and prevents accidental submission of dev bundles as cert.
        let ts = utc_compact_stamp();
        let sha_short = if git_snapshot.sha.len() >= 8 {
            &git_snapshot.sha[..8]
        } else {
            &git_snapshot.sha
        };
        let bundle_dir = config
            .output_root
            .join(format!("{}-{}-{}", config.profile, ts, sha_short));

        if bundle_dir.exists() {
            return Err(BuilderError::BundleExists { path: bundle_dir });
        }

        fs::create_dir_all(&bundle_dir).map_err(|source| BuilderError::Io {
            op: "creating",
            path: bundle_dir.clone(),
            source,
        })?;
        let tests_dir = bundle_dir.join("tests");
        fs::create_dir_all(&tests_dir).map_err(|source| BuilderError::Io {
            op: "creating",
            path: tests_dir,
            source,
        })?;
        let trace_dir = bundle_dir.join("trace");
        fs::create_dir_all(&trace_dir).map_err(|source| BuilderError::Io {
            op: "creating",
            path: trace_dir,
            source,
        })?;

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
    pub fn hash_input(&mut self, path: &str) -> Result<(), BuilderError> {
        Ok(hash_file_into(&mut self.inputs, path)?)
    }

    /// Hash a file with relative path and add to outputs.
    pub fn hash_output(&mut self, path: &Path) -> Result<(), BuilderError> {
        Ok(hash_file_relative_into(
            &mut self.outputs,
            path,
            &self.bundle_dir,
        )?)
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
    ) -> Result<(Vec<u8>, Vec<u8>), BuilderError> {
        let cwd = std::env::current_dir()
            .map_err(BuilderError::CurrentDir)?
            .display()
            .to_string();
        let argv = {
            let mut v = Vec::new();
            v.push(cmd.get_program().to_string_lossy().to_string());
            v.extend(cmd.get_args().map(|a| a.to_string_lossy().to_string()));
            v
        };

        tracing::info!("evidence: running {}...", display_name);
        let output = cmd.output().map_err(|source| BuilderError::RunCommand {
            display_name: display_name.to_string(),
            source,
        })?;
        let exit_code = output.status.code().unwrap_or(-1);

        if !output.status.success() {
            tracing::error!("{} failed with exit code {}", display_name, exit_code);
            tracing::error!("stderr: {}", String::from_utf8_lossy(&output.stderr));
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
                fs::create_dir_all(parent).map_err(|source| BuilderError::Io {
                    op: "creating",
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            fs::write(&abs, &stdout_norm).map_err(|source| BuilderError::Io {
                op: "writing stdout to",
                path: abs.clone(),
                source,
            })?;
        }
        if let Some(ref ep) = stderr_path {
            let abs = self.bundle_dir.join(ep);
            if let Some(parent) = abs.parent() {
                fs::create_dir_all(parent).map_err(|source| BuilderError::Io {
                    op: "creating",
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            fs::write(&abs, &stderr_norm).map_err(|source| BuilderError::Io {
                op: "writing stderr to",
                path: abs.clone(),
                source,
            })?;
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
    pub fn write_inputs(&self) -> Result<PathBuf, BuilderError> {
        let path = self.bundle_dir.join("inputs_hashes.json");
        let bytes =
            serde_json::to_vec_pretty(&self.inputs).map_err(|source| BuilderError::Serialize {
                kind: "inputs_hashes.json",
                source,
            })?;
        fs::write(&path, bytes).map_err(|source| BuilderError::Io {
            op: "writing",
            path: path.clone(),
            source,
        })?;
        Ok(path)
    }

    /// Write the outputs hashes file.
    pub fn write_outputs(&self) -> Result<PathBuf, BuilderError> {
        let path = self.bundle_dir.join("outputs_hashes.json");
        let bytes =
            serde_json::to_vec_pretty(&self.outputs).map_err(|source| BuilderError::Serialize {
                kind: "outputs_hashes.json",
                source,
            })?;
        fs::write(&path, bytes).map_err(|source| BuilderError::Io {
            op: "writing",
            path: path.clone(),
            source,
        })?;
        Ok(path)
    }

    /// Write the commands file.
    pub fn write_commands(&self) -> Result<PathBuf, BuilderError> {
        let path = self.bundle_dir.join("commands.json");
        let bytes = serde_json::to_vec_pretty(&self.commands).map_err(|source| {
            BuilderError::Serialize {
                kind: "commands.json",
                source,
            }
        })?;
        fs::write(&path, bytes).map_err(|source| BuilderError::Io {
            op: "writing",
            path: path.clone(),
            source,
        })?;
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
    pub fn finalize(&self, trace_outputs: Vec<PathBuf>) -> Result<PathBuf, BuilderError> {
        // TOCTOU check: verify git HEAD hasn't changed since builder was created.
        // A changed HEAD means source files may have been modified between the
        // initial snapshot and finalize, invalidating the evidence chain.
        if self.git_snapshot.sha != "unknown" {
            if let Ok(current_sha) = self.git_provider.sha() {
                let current_sha = current_sha.trim().to_string();
                if current_sha != self.git_snapshot.sha {
                    return Err(BuilderError::Toctou {
                        snapshot_sha: self.git_snapshot.sha.clone(),
                        current_sha,
                    });
                }
            }
        }

        let ts = utc_now_rfc3339();
        let sha256sums_path = self.bundle_dir.join("SHA256SUMS");

        // Step 1: Project env.json onto the cross-host-stable subset
        // and write `deterministic-manifest.json`. The manifest is
        // the committed artifact whose hash becomes the cross-host
        // reproducibility contract; writing it before SHA256SUMS is
        // assembled means `write_sha256sums` picks it up for free
        // and the integrity chain binds it like any other content
        // file.
        let env_path = self.bundle_dir.join("env.json");
        let env_bytes = fs::read(&env_path).map_err(|source| BuilderError::Io {
            op: "reading",
            path: env_path.clone(),
            source,
        })?;
        let env_fp: crate::env::EnvFingerprint =
            serde_json::from_slice(&env_bytes).map_err(BuilderError::ParseEnv)?;
        let manifest = env_fp.deterministic_manifest();
        let manifest_path = self.bundle_dir.join("deterministic-manifest.json");
        let manifest_bytes =
            serde_json::to_vec_pretty(&manifest).map_err(|source| BuilderError::Serialize {
                kind: "deterministic-manifest.json",
                source,
            })?;
        fs::write(&manifest_path, manifest_bytes).map_err(|source| BuilderError::Io {
            op: "writing",
            path: manifest_path.clone(),
            source,
        })?;

        // Step 2: Write SHA256SUMS covering the content layer only.
        // index.json does not exist yet so it is naturally excluded.
        write_sha256sums(&self.bundle_dir, &sha256sums_path)?;

        // Step 3: Compute full content_hash and the narrower
        // deterministic_hash.
        let content_hash = crate::hash::sha256_file(&sha256sums_path)?;
        let deterministic_hash = crate::hash::sha256_file(&manifest_path)?;

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
        let index_bytes =
            serde_json::to_vec_pretty(&idx).map_err(|source| BuilderError::Serialize {
                kind: "index.json",
                source,
            })?;
        fs::write(&index_path, index_bytes).map_err(|source| BuilderError::Io {
            op: "writing",
            path: index_path.clone(),
            source,
        })?;

        Ok(self.bundle_dir.clone())
    }
}
