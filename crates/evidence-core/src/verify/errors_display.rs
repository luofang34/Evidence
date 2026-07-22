//! `Display` impl for [`VerifyError`]. Pulled out of the parent
//! `errors.rs` so the leaf error file stays under the workspace
//! 500-line limit. The `match` is exhaustive — adding a variant
//! to `VerifyError` without a matching arm here fails compilation.

use super::errors::VerifyError;

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerifyError::UnexpectedFile(file) => write!(f, "unexpected file: {}", file),
            VerifyError::SignatureInvalid => write!(f, "ed25519 signature verification failed"),
            VerifyError::HashMismatch {
                file,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "hash mismatch for {}: expected {}, got {}",
                    file, expected, actual
                )
            }
            VerifyError::MissingHashedFile(file) => {
                write!(f, "file in SHA256SUMS not found: {}", file)
            }
            VerifyError::ContentHashMismatch {
                index_hash,
                actual_hash,
            } => {
                write!(
                    f,
                    "content_hash mismatch: index={}, actual={}",
                    index_hash, actual_hash
                )
            }
            VerifyError::UnsafePath(path) => {
                write!(f, "unsafe path in bundle: {}", path)
            }
            VerifyError::FormatError {
                field,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "invalid format for {}: expected {}, got {}",
                    field, expected, actual
                )
            }
            VerifyError::CrossFileInconsistency {
                field,
                index_value,
                env_value,
            } => {
                write!(
                    f,
                    "env.json vs index.json mismatch for {}: index={}, env={}",
                    field, index_value, env_value
                )
            }
            VerifyError::DeterministicHashMismatch {
                index_hash,
                actual_hash,
            } => {
                write!(
                    f,
                    "recipe_hash mismatch: index={}, actual={}",
                    index_hash, actual_hash
                )
            }
            VerifyError::ManifestProjectionDrift { detail } => {
                write!(
                    f,
                    "deterministic-manifest.json is not a valid projection of the bundle's recipe content: {}",
                    detail
                )
            }
            VerifyError::TraceOutputNotHashed(path) => {
                write!(
                    f,
                    "trace_outputs entry '{}' is not listed in SHA256SUMS",
                    path
                )
            }
            VerifyError::TestSummaryMismatch {
                field,
                index_value,
                parsed_value,
            } => {
                write!(
                    f,
                    "test_summary.{} disagrees with re-parsed stdout: index={}, parsed={}",
                    field, index_value, parsed_value
                )
            }
            VerifyError::DalMapMismatch {
                crate_name,
                index_value,
                compliance_value,
            } => {
                write!(
                    f,
                    "dal_map[{}] disagrees with compliance/{}.json: index={}, compliance={}",
                    crate_name, crate_name, index_value, compliance_value
                )
            }
            VerifyError::DalMapOrphan { crate_name, detail } => {
                write!(f, "dal_map orphan for '{}': {}", crate_name, detail)
            }
            VerifyError::PrereleaseToolDetected {
                profile,
                engine_crate_version,
            } => {
                write!(
                    f,
                    "bundle was produced by a pre-release build \
                     (engine_crate_version = {:?}); cert/record bundles from \
                     pre-release builds are not valid audit evidence \
                     (profile = {})",
                    engine_crate_version, profile
                )
            }
            VerifyError::BundleIncompletelyClaimed { failure_count } => {
                write!(
                    f,
                    "bundle_complete=true but {} captured-subprocess failure(s) \
                     are recorded in tool_command_failures; the bundle was \
                     hand-edited or produced by a broken generator",
                    failure_count
                )
            }
            VerifyError::ToolCommandsFailedSilently { profile, commands } => {
                write!(
                    f,
                    "profile='{}' bundle carries {} recorded captured-subprocess \
                     failure(s) ({}); cert/record bundles must have \
                     tool_command_failures == []",
                    profile,
                    commands.len(),
                    commands.join(", ")
                )
            }
            VerifyError::LlrTestSelectorUnresolved { llr_uid, llr_id } => {
                write!(
                    f,
                    "LLR {llr_id} (uid={llr_uid}) declares test verification but no \
                     tests/test_outcomes.jsonl record lists this uid in \
                     requirement_uids; the test_selector chain is dangling"
                )
            }
            VerifyError::TestSummaryAbsentOnFailedRun { command_name } => {
                write!(
                    f,
                    "tool_command_failures records '{}' but index.json.test_summary \
                     is absent — the two fields must be consistent",
                    command_name
                )
            }
            VerifyError::BoundaryVerifyMetadataMissing => {
                write!(
                    f,
                    "index.json.boundary_policy claims forbid_build_rs / \
                     forbid_proc_macros enforcement but the bundle is missing \
                     cargo_metadata.json — the verify-time recheck cannot replay \
                     the policy"
                )
            }
            VerifyError::BoundaryVerifyForbiddenBuildRs { details } => {
                write!(f, "verify-time recheck: in-scope build.rs found: {details}")
            }
            VerifyError::BoundaryVerifyForbiddenProcMacro { details } => {
                write!(
                    f,
                    "verify-time recheck: in-scope proc-macro found: {details}"
                )
            }
            VerifyError::SourceBaselineEmpty => {
                write!(
                    f,
                    "inputs_hashes.json is empty — the bundle records no source \
                     baseline; a bundle cannot support a source-baseline or \
                     reproducibility claim with zero recorded inputs"
                )
            }
            VerifyError::TestIdentityUnknown { test } => {
                write!(
                    f,
                    "test outcome '{test}' lost its binary identity \
                     (__unknown_binary__); LLR test_selectors cannot resolve to a \
                     test whose executing binary is unknown"
                )
            }
            VerifyError::OutputManifestEmpty => {
                write!(
                    f,
                    "outputs_hashes.json is empty but the bundle recorded a test \
                     summary — the build produced deliverables that were never \
                     attested"
                )
            }
            VerifyError::TraceEvidenceNotAdopted { profile } => {
                write!(
                    f,
                    "profile='{profile}' bundle ships no trace requirement files — \
                     trace evidence was never adopted, so the bundle's \
                     traceability claim has zero backing"
                )
            }
            VerifyError::TraceEvidenceEmpty { profile } => {
                write!(
                    f,
                    "profile='{profile}' bundle ships trace files with zero \
                     requirements — an empty trace set is an adoption state, \
                     not evidence"
                )
            }
            VerifyError::OnlineResolutionBundle { profile } => {
                write!(
                    f,
                    "profile='{profile}' bundle records resolution_policy=online_opt_in — \
                     its dependency graph was resolved with network access, so it \
                     cannot back a cert/record claim"
                )
            }
        }
    }
}
