//! Curated crate-root exports.

pub use crate::boundary_check::{
    BoundaryCheckError, check_dal_a_mcdc_evidence, check_no_build_rs, check_no_out_of_scope_deps,
    check_no_proc_macros,
};
pub use crate::bundle::{
    ArtifactError, EvidenceBuildConfig, EvidenceBuilder, EvidenceIndex, InputEntry, InputReason,
    InputScopeError, OutputArtifact, ResolvedUnit, SigningError, TestSummary, ToolCommandFailure,
    WORKSPACE_CONTROL_PATHSPECS, assemble_input_plan, build_input_plan_blocking,
    generate_signing_key, inventory_outputs_blocking, parse_cargo_test_output_detailed,
    parse_nextest_libtest_json, parse_workspace_artifacts, read_signing_key, read_verifying_key,
    resolve_in_scope_units, sign_bundle, verify_bundle_signature, write_signing_key,
    write_verifying_key,
};
pub use crate::compliance::{
    Applicability, ComplianceReport, ComplianceSummary, CrateEvidence, OBJECTIVES, ObjectiveStatus,
    ObjectiveStatusKind, generate_compliance_report,
};
pub use crate::coverage::{
    CoverageLevel, CoverageReport, CoverageThresholdViolation, evaluate_thresholds,
    parse_llvm_cov_export,
};
pub use crate::diagnostic::{Diagnostic, DiagnosticCode, Location, Severity, TERMINAL_CODES};
pub use crate::env::{EnvFingerprint, Host};
pub use crate::floors::{FloorsConfig, current_measurements};
pub use crate::git::{GitSnapshot, RealGitProvider};
pub use crate::policy::{
    AuxiliaryMcdcTool, BoundaryConfig, BoundaryInputs, BoundaryPolicy, Dal, DalConfig,
    EvidencePolicy, Profile, TracePolicy, load_trace_roots,
};
pub use crate::rules::{Domain, RULES, RuleEntry};
pub use crate::trace::{
    DerivedEntry, DerivedFile, HlrEntry, HlrFile, LlrEntry, LlrFile, TestEntry, TestsFile,
    generate_traceability_matrix, read_all_trace_files, validate_trace_links,
    validate_trace_links_with_policy,
};
pub use crate::traits::GitProvider;
pub use crate::verify::{VerifyError, VerifyResult, verify_bundle, verify_bundle_with_key};
pub use ed25519_dalek::{Signature, SigningKey, VerifyingKey};

#[doc(hidden)]
pub use crate::boundary_check::{BoundaryViolation, BuildRsViolation, ProcMacroViolation};
#[doc(hidden)]
pub use crate::bundle::{CommandRecord, parse_cargo_test_output};
#[doc(hidden)]
pub use crate::cargo_metadata::{
    CargoMetadataProjection, PackageProjection, ProjectionError, TargetProjection,
    check_build_rs_in_projection, check_proc_macros_in_projection,
};
#[doc(hidden)]
pub use crate::coverage::{
    BranchCoverage, ConditionCoverage, DecisionCoverage, FileMeasurement, LineCoverage,
    LlvmCovParseError, Measurement, aggregate_branches_percent, aggregate_lines_percent,
};
#[doc(hidden)]
pub use crate::diagnostic::FixHint;
#[doc(hidden)]
pub use crate::env::DeterministicManifest;
#[doc(hidden)]
pub use crate::floors::LoadOutcome;
#[doc(hidden)]
pub use crate::git::{check_shallow_clone, is_dirty_or_unknown};
#[doc(hidden)]
pub use crate::hash::{sha256, sha256_file};
#[doc(hidden)]
pub use crate::policy::DalCoverageThresholds;
#[doc(hidden)]
pub use crate::rules::{
    HAND_EMITTED_CLI_CODES, HAND_EMITTED_MCP_CODES, RESERVED_UNCLAIMED_CODES, rules_json,
};
#[doc(hidden)]
pub use crate::trace::{
    Schema, TraceFiles, TraceMeta, assign_valid_uuids_derived, assign_valid_uuids_hlr,
    assign_valid_uuids_llr, assign_valid_uuids_test, backfill_uuids, read_toml,
};
#[doc(hidden)]
pub use crate::util::normalize_bundle_path;
