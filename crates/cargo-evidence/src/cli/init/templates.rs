//! The managed template set rendered by `cargo evidence init`
//! (LLR-150).
//!
//! Every file init writes — boundary config, profile stubs, floors
//! config, and the five trace files — is rendered here so
//! `cmd_init` iterates one list for both the write path and the
//! `--force` rewrite path. That single list IS the managed
//! template set: `--force` rewrites exactly these paths and
//! nothing else, so user evidence outside them is unreachable.
//!
//! Version strings flow from `evidence_core::schema_versions` and
//! `evidence_core::floors::FLOORS_SCHEMA_VERSION`, never literals —
//! a schema bump stays a one-line change upstream
//! (`schema_versions_locked` greps the source tree for stray
//! `"0.0.x"` strings).
//!
//! Trace templates carry an explicit empty entry array
//! (`requirements = []` / `tests = []`) ahead of the first table
//! header — TOML parses root keys only before any `[table]`
//! header, and the `*File` serde shapes require the field — so a
//! fresh scaffold parses through `read_all_trace_files`
//! immediately. The worked example entry in each file is present
//! only as comment lines: no placeholder parses as a live
//! requirement, and an unmodified scaffold contributes zero
//! entries to any bundle.

use evidence_core::floors::FLOORS_SCHEMA_VERSION;
use evidence_core::schema_versions::{BOUNDARY, TRACE};

/// One managed template: the workspace-relative path init writes
/// (and `--force` rewrites) plus the rendered file body.
pub struct ManagedTemplate {
    /// Workspace-relative destination, forward-slash separated.
    pub path: &'static str,
    /// Rendered file contents.
    pub content: String,
}

/// Render the complete managed template set for a fresh scaffold:
/// `cert/boundary.toml`, the three `cert/profiles/*.toml` stubs,
/// `cert/floors.toml`, and the five
/// `cert/trace/{sys,hlr,llr,tests,derived}.toml` files. Order is
/// stable so human and jsonl output list files deterministically.
pub fn managed_templates() -> Vec<ManagedTemplate> {
    vec![
        ManagedTemplate {
            path: "cert/boundary.toml",
            content: boundary_template(),
        },
        ManagedTemplate {
            path: "cert/floors.toml",
            content: floors_template(),
        },
        ManagedTemplate {
            path: "cert/profiles/dev.toml",
            content: PROFILE_DEV.to_string(),
        },
        ManagedTemplate {
            path: "cert/profiles/cert.toml",
            content: PROFILE_CERT.to_string(),
        },
        ManagedTemplate {
            path: "cert/profiles/record.toml",
            content: PROFILE_RECORD.to_string(),
        },
        ManagedTemplate {
            path: "cert/trace/sys.toml",
            content: sys_template(),
        },
        ManagedTemplate {
            path: "cert/trace/hlr.toml",
            content: hlr_template(),
        },
        ManagedTemplate {
            path: "cert/trace/llr.toml",
            content: llr_template(),
        },
        ManagedTemplate {
            path: "cert/trace/tests.toml",
            content: tests_template(),
        },
        ManagedTemplate {
            path: "cert/trace/derived.toml",
            content: derived_template(),
        },
    ]
}

/// Render the `boundary.toml` template.
///
/// Built at call time rather than stored as a `const` so the
/// `[schema].version` string flows from `schema_versions::BOUNDARY`
/// — no literals to hunt down on a schema bump.
fn boundary_template() -> String {
    format!(
        r#"# Navigate Certification Boundary Configuration
# Schema version: {ver}

[schema]
version = "{ver}"

[scope]
# Crates that are in scope for certification
in_scope = [
    # Add your certifiable crates here
    # "my-crate",
]

# Trace root directories (relative to workspace root)
trace_roots = ["cert/trace"]

# Workspace crates explicitly forbidden as dependencies
explicit_forbidden = []

[policy]
# NOTE: these three flags are reserved for upcoming real enforcement.
# Until each one's cargo-metadata-backed check lands, enabling it
# causes `cargo evidence generate` to refuse the run — the tool will
# not silently produce a bundle stamped cert-ready under a rule it
# never actually checked. Flip to `true` per rule once this release
# notes that rule as enforced.

# Forbid dependencies on out-of-scope workspace crates (enforcement TBD)
no_out_of_scope_deps = false

# Forbid build.rs in boundary crates (enforcement TBD; DO-178C determinism)
forbid_build_rs = false

# Forbid proc-macros in boundary crates (enforcement TBD; DO-178C auditability)
forbid_proc_macros = false

[forbidden_external]
# External crates that are forbidden with reasons
# "crate_name" = "reason"

[dal]
# Default Design Assurance Level for all in-scope crates (A, B, C, or D).
# D is the least stringent. Omit the whole [dal] section for
# unclassified development; cert/record profiles fail closed without
# an explicit default_dal here (POLICY_ASSURANCE_SELECTION_MISSING).
default_dal = "D"

# Per-crate DAL overrides
# [dal.crate_overrides]
# "my-critical-crate" = "A"
# "my-utility-crate" = "C"
"#,
        ver = BOUNDARY
    )
}

/// Render the `floors.toml` template: an honest zero baseline for
/// every workspace-true dimension. The file parses through
/// `FloorsConfig::load_or_missing` as `Loaded` immediately, so
/// doctor's floors check passes (or reports slack on dimensions
/// that measure tool-side constants) instead of firing
/// `DOCTOR_FLOORS_MISSING` on a fresh scaffold. Per-crate tables
/// ship only as commented examples — the `[per_crate.*]` key set
/// must equal `boundary.toml`'s `scope.in_scope`, which is empty
/// on a fresh scaffold, so any live table would misfire the
/// bijection.
fn floors_template() -> String {
    format!(
        r#"# Ratcheting floors for this project ("rigor only goes up").
#
# Each entry is a current measurement taken at commit time; the
# floors gate (`cargo evidence floors`) fails any change where
# `current < committed_floor`. Raising a floor means editing the
# number in a reviewed commit.
#
# Fresh-scaffold baseline: every workspace-true dimension starts
# at 0 — nothing has been adopted yet, and zero is the honest
# floor. As the project adopts a dimension, set its floor to the
# current value printed by `cargo evidence floors`; the gate then
# blocks later regressions. Dimension semantics are documented in
# evidence_core::floors.
#
# `schema_version` pins the file shape; older tools refuse a newer
# version rather than silently skipping unknown fields.

schema_version = {schema_version}

[floors]

# evidence_core::RULES length — every diagnostic code the tool
# can emit. Workspace-wide: the registry lives in one crate.
diagnostic_codes = 0

# evidence_core::TERMINAL_CODES length — hand-emitted jsonl
# terminals (VERIFY_OK / DOCTOR_OK / GENERATE_OK / ...).
terminal_codes = 0

# Per-layer entry counts across the configured trace roots.
trace_sys = 0
trace_hlr = 0
trace_llr = 0
trace_test = 0

# evidence_core::trace::surfaces::KNOWN_SURFACES length.
known_surfaces = 0

# Per-crate dimensions: one table per crate listed in
# cert/boundary.toml's scope.in_scope (doctor enforces the
# bijection, so add these only after declaring in-scope crates).
# Absolute floors ratchet `current >= value`; ceilings ratchet
# down-only dimensions (`current <= value`, e.g. panics).
# Examples:
#
# [per_crate.my-crate]
# test_count = 0
#
# [per_crate_ceilings.my-crate]
# library_panics = 0
"#,
        schema_version = FLOORS_SCHEMA_VERSION
    )
}

const PROFILE_DEV: &str = r#"# Development Profile
# Relaxed checks for local development

[profile]
name = "dev"
description = "Development profile with relaxed checks"

[checks]
require_clean_git = false
require_coverage = false
allow_all_features = true
offline_required = false

[evidence]
include_timestamps = true
strict_hash_validation = false
fail_on_dirty = false
"#;

const PROFILE_CERT: &str = r#"# Certification Profile
# Strict checks for certification builds

[profile]
name = "cert"
description = "Certification profile with strict compliance checks"

[checks]
require_clean_git = true
require_coverage = true
allow_all_features = false
offline_required = true

[evidence]
include_timestamps = false
strict_hash_validation = true
fail_on_dirty = true
"#;

const PROFILE_RECORD: &str = r#"# Recording Profile
# Captures evidence without full enforcement

[profile]
name = "record"
description = "Recording profile for evidence capture"

[checks]
require_clean_git = true
require_coverage = false
allow_all_features = true
offline_required = false

[evidence]
include_timestamps = true
strict_hash_validation = false
fail_on_dirty = true
"#;

/// Shared header for the five trace templates: layer-specific
/// prose first, then the block every file repeats — the field
/// cheat-sheet, the explicit empty entry array (ahead of the
/// first table header so TOML keeps it a root key), and the
/// commented example entry the user edits into their first real
/// requirement.
fn trace_template(
    layer_intro: &str,
    fields_cheatsheet: &str,
    entry_array: &str,
    document_id: &str,
    example: &str,
) -> String {
    format!(
        r#"{layer_intro}
#
{fields_cheatsheet}
#
# This file intentionally holds ZERO live entries: a fresh
# scaffold is adoption-incomplete, which `cargo evidence trace
# --validate` reports as TRACE_EVIDENCE_EMPTY and `cargo evidence
# doctor` as DOCTOR_TRACE_NO_EVIDENCE — the intended pre-adoption
# signal, not a scaffold error.

{entry_array} = []

[schema]
version = "{ver}"

[meta]
document_id = "{document_id}"
revision = "1"

# Example entry — every line commented so placeholder content can
# never parse as a live requirement or enter an evidence bundle.
# To adopt: delete the `{entry_array} = []` line above and add
# your first real entry below (the block below, edited), then run
# `cargo evidence trace --backfill-uuids` to assign a real UUID.
#
{example}
"#,
        ver = TRACE,
    )
}

/// SYS is the System layer above HLR — the four-layer trace
/// chain cargo-evidence itself uses (SYS/HLR/LLR/TEST).
/// Downstream projects without a SYS layer can't satisfy the
/// DAL-A-and-up `require_hlr_sys_trace` policy gate; shipping
/// the template keeps the shape consistent with the tool's
/// own dogfood.
fn sys_template() -> String {
    trace_template(
        r#"# System Requirements
#
# The top of the DO-178C §5.1 trace chain: system-level
# assumptions the software is supposed to enforce. Each HLR
# below should trace to at least one SYS entry at DAL-A/B."#,
        r#"# Each [[requirements]] entry must include:
#   uid    - machine-stable UUID; write any placeholder (e.g.
#            "SYS-001") and let `cargo evidence trace
#            --backfill-uuids` assign a real v4 UUID in place
#   id     - human-readable slug
#   title  - short description
# Optional fields: owner, description, rationale,
#   verification_methods, source."#,
        "requirements",
        "SYS-DOC-001",
        r#"# [[requirements]]
# uid = "SYS-001"
# id = "sys-example"
# title = "Example System Requirement"
# description = "This is an example system-level requirement."
# owner = "team@example.com"
# verification_methods = ["review"]"#,
    )
}

fn hlr_template() -> String {
    trace_template(
        r#"# High-Level Requirements
#
# System-level behavior decomposed into verifiable statements;
# each LLR below derives from at least one HLR."#,
        r#"# Each [[requirements]] entry must include:
#   uid    - machine-stable UUID (see sys.toml for the backfill
#            workflow)
#   id     - human-readable slug
#   title  - short description
# Optional fields: owner, description, rationale, sort_key,
#   scope, category, source, verification_methods, traces_to
#   (SYS UIDs this HLR derives from)"#,
        "requirements",
        "HLR-DOC-001",
        r#"# [[requirements]]
# uid = "HLR-001"
# id = "hlr-example"
# title = "Example Requirement"
# description = "This is an example high-level requirement."
# owner = "team@example.com"
# verification_methods = ["test", "review"]"#,
    )
}

fn llr_template() -> String {
    trace_template(
        r#"# Low-Level Requirements
#
# Implementation-level statements; each LLR derives from at
# least one HLR and is verified by at least one test."#,
        r#"# Each [[requirements]] entry must include:
#   uid         - machine-stable UUID (see sys.toml for the
#                 backfill workflow)
#   id          - human-readable slug
#   title       - short description
#   traces_to   - list of HLR UIDs this LLR derives from
# Optional fields: owner, description, rationale, sort_key,
#   derived (bool), modules, verification_methods, source"#,
        "requirements",
        "LLR-DOC-001",
        r#"# [[requirements]]
# uid = "LLR-001"
# id = "llr-example"
# title = "Example Implementation Requirement"
# description = "This is an example low-level requirement."
# owner = "team@example.com"
# traces_to = ["HLR-001"]
# verification_methods = ["test"]"#,
    )
}

fn tests_template() -> String {
    trace_template(
        r#"# Test Cases
#
# Each test verifies at least one LLR; `test_selector` points at
# a real #[test] fn so renames surface as validation findings."#,
        r#"# Each [[tests]] entry must include:
#   uid        - machine-stable UUID (see sys.toml for the
#                backfill workflow)
#   id         - human-readable slug
#   title      - short description
#   traces_to  - list of LLR UIDs this test verifies
# Optional fields: owner, description, sort_key, category,
#   test_selector (e.g. "crate::module::test_fn"), source"#,
        "tests",
        "TST-DOC-001",
        r#"# [[tests]]
# uid = "TST-001"
# id = "test-example"
# title = "Example Test Case"
# description = "Verifies that the example LLR is satisfied."
# owner = "team@example.com"
# traces_to = ["LLR-001"]"#,
    )
}

fn derived_template() -> String {
    trace_template(
        r#"# Derived Requirements
#
# Requirements that emerge during design or implementation
# rather than flowing down from an HLR; each carries its own
# rationale and safety impact."#,
        r#"# Each [[requirements]] entry must include:
#   uid            - machine-stable UUID (see sys.toml for the
#                    backfill workflow)
#   id             - human-readable slug
#   title          - short description
#   rationale      - why this requirement was derived
# Optional fields: owner, description, sort_key,
#   safety_impact ("none" | "low" | "medium" | "high"), source"#,
        "requirements",
        "DRQ-DOC-001",
        r#"# [[requirements]]
# uid = "DRQ-001"
# id = "derived-example"
# title = "Example Derived Requirement"
# description = "A requirement derived during design or implementation."
# owner = "team@example.com"
# rationale = "Required for implementation of HLR-001"
# safety_impact = "none""#,
    )
}
