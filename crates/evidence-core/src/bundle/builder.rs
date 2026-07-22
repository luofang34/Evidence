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

use super::command::CommandRecord;
use super::command_failure::ToolCommandFailure;
use super::error::BuilderError;
use super::index::EvidenceIndex;
use super::outcome_record::TestOutcomeRecord;
use super::run_capture::run_capture as do_run_capture;
use super::test_summary::TestSummary;
use super::time::{utc_compact_stamp, utc_now_rfc3339};
use crate::git::{GitSnapshot, RealGitProvider};
use crate::hash::write_sha256sums;
use crate::policy::Profile;
use crate::traits::GitProvider;

mod config;
mod naming;
mod outputs;
#[cfg(test)]
mod tests;
pub use config::EvidenceBuildConfig;

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
    /// Per-test outcome records; non-empty triggers a
    /// `tests/test_outcomes.jsonl` artifact at finalize-adjacent
    /// time (see [`Self::write_test_outcomes`]).
    test_outcomes: Vec<TestOutcomeRecord>,
    /// Structural coverage report captured by the generate
    /// coverage phase. `None` when `--coverage=none` or the
    /// phase didn't run. When populated, aggregate percents
    /// feed the A-7 Obj-5/6 compliance evaluator.
    coverage_report: Option<crate::CoverageReport>,
    /// Captured-subprocess failures — non-zero exits from any
    /// `run_capture`'d command (cargo test, cargo check, etc.).
    /// Drives [`EvidenceIndex::bundle_complete`] (empty ⇒
    /// `true`) and the verify-time cross-check that a cert/
    /// record bundle carries no recorded failures.
    tool_command_failures: Vec<ToolCommandFailure>,
}

impl EvidenceBuilder {
    /// Create a new evidence builder. Uses the real git
    /// provider; for testing use [`Self::new_with_provider`].
    pub fn new(config: EvidenceBuildConfig) -> Result<Self, BuilderError> {
        Self::new_with_provider(config, RealGitProvider)
    }

    /// Create a new evidence builder with a custom git provider
    /// for tests. The provider feeds the initial snapshot and
    /// the TOCTOU re-check at [`Self::finalize`] time.
    pub fn new_with_provider<G: GitProvider + 'static>(
        config: EvidenceBuildConfig,
        provider: G,
    ) -> Result<Self, BuilderError> {
        Self::new_with_provider_at(config, provider, &utc_compact_stamp())
    }

    fn new_with_provider_at<G: GitProvider + 'static>(
        config: EvidenceBuildConfig,
        provider: G,
        timestamp: &str,
    ) -> Result<Self, BuilderError> {
        let strict = matches!(config.profile, Profile::Cert | Profile::Record);

        let git_snapshot = GitSnapshot::capture_with(&provider, strict)?;

        crate::git::check_shallow_clone()?;

        if (config.require_clean_git || config.fail_on_dirty) && git_snapshot.dirty {
            let dirty_files = match provider.dirty_files() {
                Ok(files) => files,
                Err(e) => {
                    // Git reported dirty via `git_snapshot.dirty` above, but
                    // we couldn't list the files. The cert check still fires
                    // (error returned below); surface the list-failure root
                    // cause so the audit trail can reconstruct why the user
                    // got a "dirty tree" error with no file list attached.
                    tracing::warn!(
                        error = %e,
                        "git reported dirty tree but could not list dirty files; \
                         error message will omit the file list"
                    );
                    Vec::new()
                }
            };
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

        let bundle_dir = naming::create_bundle_directories(&config, &git_snapshot, timestamp)?;

        Ok(Self {
            config,
            git_snapshot,
            git_provider: Box::new(provider),
            bundle_dir,
            commands: Vec::new(),
            inputs: BTreeMap::new(),
            outputs: BTreeMap::new(),
            test_summary: None,
            test_outcomes: Vec::new(),
            coverage_report: None,
            tool_command_failures: Vec::new(),
        })
    }

    /// Record a captured-subprocess failure. Called by
    /// [`Self::run_capture`] on non-zero exit and by phase
    /// orchestrators that caught a [`BuilderError::RunCommand`]
    /// spawn failure.
    ///
    /// Non-empty `tool_command_failures` flips
    /// [`EvidenceIndex::bundle_complete`] to `false` at
    /// [`Self::finalize`] time; verify cross-checks that
    /// cert/record bundles never ship with recorded failures.
    pub fn record_command_failure(&mut self, failure: ToolCommandFailure) {
        self.tool_command_failures.push(failure);
    }

    /// Read-only view of the captured-subprocess failures.
    /// Callers (the CLI's generate-exit-code logic) use this to
    /// decide whether to propagate a non-zero exit on cert/
    /// record profile even when the bundle assembled cleanly.
    pub fn tool_command_failures(&self) -> &[ToolCommandFailure] {
        &self.tool_command_failures
    }

    /// Get the bundle directory path.
    pub fn bundle_dir(&self) -> &Path {
        &self.bundle_dir
    }

    /// The dependency-resolution policy this bundle is being built
    /// under (from [`EvidenceBuildConfig::resolution_policy`]). Every
    /// cargo-invoking phase reads the policy here so the pipeline
    /// shares one source of truth (LLR-139 / LLR-140).
    pub fn resolution_policy(&self) -> crate::policy::ResolutionPolicy {
        self.config.resolution_policy
    }

    /// Hash a file into inputs, keyed by `path`. Wrapper over [`Self::hash_input_under`].
    pub fn hash_input(&mut self, path: &str) -> Result<(), BuilderError> {
        self.hash_input_under(Path::new("."), path)
    }

    /// Hash `base/rel_path`, keyed by the workspace-relative
    /// `rel_path`. The source-baseline write seam: production passes the
    /// CWD; the acceptance test passes a temp workspace so it drives the
    /// real [`Self::write_inputs`] without chdir.
    pub fn hash_input_under(&mut self, base: &Path, rel_path: &str) -> Result<(), BuilderError> {
        let hash = crate::hash::sha256_file(&base.join(rel_path))?;
        self.inputs.insert(rel_path.to_string(), hash);
        Ok(())
    }

    /// Record a command execution.
    pub fn record_command(&mut self, record: CommandRecord) {
        self.commands.push(record);
    }

    /// Run a command, capture its output, write stdout/stderr
    /// to the bundle, and record any non-zero exit on
    /// `self.tool_command_failures`. Library layer — no
    /// presentation logging (CLI owns severity/format).
    pub fn run_capture(
        &mut self,
        cmd: Command,
        rel_dir: &str,
        output_name_base: &str,
        display_name: &str,
    ) -> Result<(Vec<u8>, Vec<u8>), BuilderError> {
        let outcome = do_run_capture(
            cmd,
            rel_dir,
            output_name_base,
            display_name,
            &self.bundle_dir,
        )?;
        self.commands.push(outcome.record);
        if let Some(failure) = outcome.failure {
            self.tool_command_failures.push(failure);
        }
        Ok((outcome.stdout_norm, outcome.stderr_norm))
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

    /// Store per-test outcome records; consumed by
    /// [`Self::write_test_outcomes`].
    pub fn set_test_outcomes(&mut self, outcomes: Vec<TestOutcomeRecord>) {
        self.test_outcomes = outcomes;
    }

    /// `true` iff records were captured. Used by the compliance
    /// generator to upgrade A-7 Obj-3/Obj-4 Partial → Met.
    pub fn has_test_outcomes(&self) -> bool {
        !self.test_outcomes.is_empty()
    }

    /// Accessor for the coverage-facing impl in
    /// `builder_coverage.rs`. The sibling module extends
    /// [`EvidenceBuilder`] with `set_coverage_report` +
    /// aggregate getters without pushing this file past the
    /// 500-line workspace limit.
    pub(super) fn coverage_report_ref(&self) -> Option<&crate::CoverageReport> {
        self.coverage_report.as_ref()
    }

    pub(super) fn coverage_report_mut(&mut self) -> &mut Option<crate::CoverageReport> {
        &mut self.coverage_report
    }

    /// Populate `requirement_uids` on each stored
    /// [`TestOutcomeRecord`] by joining against the trace's
    /// [`crate::trace::TestEntry`] list. Call after
    /// [`Self::set_test_outcomes`] and before
    /// [`Self::write_test_outcomes`] so the serialized JSONL
    /// carries the back-links.
    pub fn enrich_test_outcomes_with_llrs(&mut self, test_entries: &[crate::trace::TestEntry]) {
        crate::trace::resolve_llr_backlinks(&mut self.test_outcomes, test_entries);
    }

    /// Serialize records to `tests/test_outcomes.jsonl`. Call
    /// before [`Self::finalize`] so `write_sha256sums` covers
    /// the file.
    pub fn write_test_outcomes(&self) -> Result<Option<PathBuf>, BuilderError> {
        super::outcome_record::write_outcomes_jsonl(&self.bundle_dir, &self.test_outcomes)
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

        // Step 1: Write the deterministic projection of
        // `cargo metadata --format-version 1` into the bundle as
        // `cargo_metadata.json` BEFORE the recipe manifest — the
        // manifest's `locked_graph_hash` binds the canonical
        // resolved-dependency projection this step produces
        // (LLR-141 / LLR-144). The artifact binds the resolved
        // dependency graph and lets verify-time re-run the boundary
        // checks the bundle claimed at generate time (LLR-072).
        // Cert/record bundles always carry it; the development
        // profile writes it only when the boundary policy enables
        // `forbid_build_rs` or `forbid_proc_macros`. Landing it
        // before `write_sha256sums` lets the integrity chain
        // auto-bind it like every other content file.
        let metadata_projection = if matches!(self.config.profile, Profile::Cert | Profile::Record)
            || self.config.boundary_policy.forbid_build_rs
            || self.config.boundary_policy.forbid_proc_macros
        {
            Some(write_cargo_metadata_projection(
                &self.bundle_dir,
                self.config.resolution_policy,
            )?)
        } else {
            None
        };

        // Step 2: Project env.json plus the recorded build inputs
        // onto the canonical recipe manifest and write
        // `deterministic-manifest.json`. The manifest is the
        // committed artifact whose hash becomes the recipe identity
        // (`index.json.recipe_hash`); writing it before SHA256SUMS
        // is assembled means `write_sha256sums` picks it up for
        // free and the integrity chain binds it like any other
        // content file. `inputs_hash` / `command_recipe_hash` are
        // computed over the same canonical serializations
        // `write_inputs` / `write_commands` persist, so the
        // generate-time and verify-time digests agree. `features`
        // records empty: the tool does not set cargo features.
        let env_path = self.bundle_dir.join("env.json");
        let env_bytes = fs::read(&env_path).map_err(|source| BuilderError::Io {
            op: "reading",
            path: env_path.clone(),
            source,
        })?;
        let env_fp: crate::env::EnvFingerprint =
            serde_json::from_slice(&env_bytes).map_err(BuilderError::ParseEnv)?;
        let recipe_inputs = recipe::assemble_recipe_inputs(
            &self.inputs,
            &self.commands,
            metadata_projection.as_ref(),
            self.config.resolution_policy,
        )?;
        let manifest = env_fp.recipe_manifest(&recipe_inputs);
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

        // Step 3: Write SHA256SUMS covering the content layer only.
        // index.json does not exist yet so it is naturally excluded.
        write_sha256sums(&self.bundle_dir, &sha256sums_path)?;

        // Step 4: Compute full content_hash and the recipe_hash.
        let content_hash = crate::hash::sha256_file(&sha256sums_path)?;
        let recipe_hash = crate::hash::sha256_file(&manifest_path)?;

        // Step 5: Build and write index.json (metadata layer).
        let idx = EvidenceIndex {
            schema_version: crate::schema_versions::INDEX.to_string(),
            boundary_schema_version: crate::schema_versions::BOUNDARY.to_string(),
            trace_schema_version: crate::schema_versions::TRACE.to_string(),
            profile: self.config.profile,
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
            bundle_complete: self.tool_command_failures.is_empty(),
            content_hash,
            recipe_hash,
            test_summary: self.test_summary.clone(),
            tool_command_failures: self.tool_command_failures.clone(),
            dal_map: self
                .config
                .dal_map
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
            boundary_policy: self.config.boundary_policy.clone(),
            resolution_policy: self.config.resolution_policy,
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

mod cargo_metadata_artifact;
mod recipe;
use cargo_metadata_artifact::write_cargo_metadata_projection;
