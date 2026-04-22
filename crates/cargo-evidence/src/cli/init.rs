//! `cargo evidence init`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use evidence_core::diagnostic::{Diagnostic, Severity};
use evidence_core::schema_versions::{BOUNDARY, TRACE};

use super::args::{EXIT_ERROR, EXIT_SUCCESS, OutputFormat};
use super::output::emit_jsonl;

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
# D is the least stringent (default if omitted).
default_dal = "D"

# Per-crate DAL overrides
# [dal.crate_overrides]
# "my-critical-crate" = "A"
# "my-utility-crate" = "C"
"#,
        ver = BOUNDARY
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

/// `cargo evidence init` handler: scaffold a `cert/` layout
/// (boundary.toml + per-profile stubs) for a fresh project. Refuses
/// to overwrite an existing `cert/` tree unless `force` is set.
pub fn cmd_init(force: bool, format: OutputFormat) -> Result<i32> {
    let jsonl = format == OutputFormat::Jsonl;
    let cert_dir = PathBuf::from("cert");
    let profiles_dir = cert_dir.join("profiles");

    // Check if cert directory exists and not forcing
    if cert_dir.exists() && !force {
        if jsonl {
            emit_jsonl(&Diagnostic {
                code: "INIT_CERT_DIR_EXISTS".to_string(),
                severity: Severity::Error,
                message: "cert/ directory already exists. Use --force to overwrite.".to_string(),
                location: None,
                fix_hint: None,
                subcommand: Some("init".to_string()),
                root_cause_uid: None,
            })?;
            emit_jsonl(&init_terminal(
                "INIT_FAIL",
                Severity::Error,
                "init refused: cert/ exists and --force not set",
            ))?;
        } else {
            eprintln!("error: cert/ directory already exists. Use --force to overwrite.");
        }
        return Ok(EXIT_ERROR);
    }

    // Create directories
    fs::create_dir_all(&profiles_dir)?;
    fs::create_dir_all(cert_dir.join("trace"))?;

    let mut written = 0u64;

    // Write boundary.toml
    let boundary_path = cert_dir.join("boundary.toml");
    if !boundary_path.exists() || force {
        fs::write(&boundary_path, boundary_template())?;
        emit_template_written(jsonl, &boundary_path)?;
        written += 1;
    }

    // Write profile configs
    let profiles = [
        ("dev.toml", PROFILE_DEV),
        ("cert.toml", PROFILE_CERT),
        ("record.toml", PROFILE_RECORD),
    ];

    for (name, content) in profiles {
        let path = profiles_dir.join(name);
        if !path.exists() || force {
            fs::write(&path, content)?;
            emit_template_written(jsonl, &path)?;
            written += 1;
        }
    }

    // Create example trace files (must match struct field names for TOML parsing)
    let hlr_example = format!(
        r#"# High-Level Requirements
#
# Each [[requirements]] entry must include:
#   uid    - unique identifier (e.g. "HLR-001")
#   id     - human-readable slug
#   title  - short description
# Optional fields: owner, description, rationale, sort_key,
#   scope, category, source, verification_methods

[schema]
version = "{TRACE_VERSION}"

[meta]
document_id = "HLR-DOC-001"
revision = "1"

[[requirements]]
uid = "HLR-001"
id = "hlr-example"
title = "Example Requirement"
description = "This is an example high-level requirement."
owner = "team@example.com"
verification_methods = ["test", "review"]
"#,
        TRACE_VERSION = TRACE
    );

    let llr_example = format!(
        r#"# Low-Level Requirements
#
# Each [[requirements]] entry must include:
#   uid         - unique identifier (e.g. "LLR-001")
#   id          - human-readable slug
#   title       - short description
#   traces_to   - list of HLR UIDs this LLR derives from
# Optional fields: owner, description, rationale, sort_key,
#   derived (bool), modules, verification_methods, source

[schema]
version = "{TRACE_VERSION}"

[meta]
document_id = "LLR-DOC-001"
revision = "1"

[[requirements]]
uid = "LLR-001"
id = "llr-example"
title = "Example Implementation Requirement"
description = "This is an example low-level requirement."
owner = "developer@example.com"
traces_to = ["HLR-001"]
verification_methods = ["test"]
"#,
        TRACE_VERSION = TRACE
    );

    let tests_example = format!(
        r#"# Test Cases
#
# Each [[tests]] entry must include:
#   uid        - unique identifier (e.g. "TST-001")
#   id         - human-readable slug
#   title      - short description
#   traces_to  - list of LLR UIDs this test verifies
# Optional fields: owner, description, sort_key, category,
#   test_selector (e.g. "crate::module::test_fn"), source

[schema]
version = "{TRACE_VERSION}"

[meta]
document_id = "TST-DOC-001"
revision = "1"

[[tests]]
uid = "TST-001"
id = "test-example"
title = "Example Test Case"
description = "Verifies that the example LLR is satisfied."
owner = "tester@example.com"
traces_to = ["LLR-001"]
"#,
        TRACE_VERSION = TRACE
    );

    let derived_example = format!(
        r#"# Derived Requirements
#
# Each [[requirements]] entry must include:
#   uid            - unique identifier (e.g. "DRQ-001")
#   id             - human-readable slug
#   title          - short description
#   rationale      - why this requirement was derived
# Optional fields: owner, description, sort_key,
#   safety_impact ("none" | "low" | "medium" | "high"), source

[schema]
version = "{TRACE_VERSION}"

[meta]
document_id = "DRQ-DOC-001"
revision = "1"

[[requirements]]
uid = "DRQ-001"
id = "derived-example"
title = "Example Derived Requirement"
description = "A requirement derived during design or implementation."
owner = "team@example.com"
rationale = "Required for implementation of HLR-001"
safety_impact = "none"
"#,
        TRACE_VERSION = TRACE
    );

    let trace_dir = cert_dir.join("trace");
    let trace_files = [
        ("hlr.toml", hlr_example),
        ("llr.toml", llr_example),
        ("tests.toml", tests_example),
        ("derived.toml", derived_example),
    ];

    for (name, content) in trace_files {
        let path = trace_dir.join(name);
        if !path.exists() || force {
            fs::write(&path, &content)?;
            emit_template_written(jsonl, &path)?;
            written += 1;
        }
    }

    if jsonl {
        emit_jsonl(&init_terminal(
            "INIT_OK",
            Severity::Info,
            &format!("init wrote {} template file(s) under cert/", written),
        ))?;
    } else {
        println!("\nInitialized evidence tracking in cert/");
        println!("\nNext steps:");
        println!("  1. Edit cert/boundary.toml to define in-scope crates");
        println!("  2. Add requirements to cert/trace/ (hlr.toml, llr.toml, tests.toml)");
        println!("  3. Run: cargo evidence generate --out-dir evidence");
    }

    Ok(EXIT_SUCCESS)
}

fn emit_template_written(jsonl: bool, path: &Path) -> Result<()> {
    if jsonl {
        emit_jsonl(&Diagnostic {
            code: "INIT_TEMPLATE_WRITTEN".to_string(),
            severity: Severity::Info,
            message: format!("created {}", path.display()),
            location: Some(evidence_core::Location {
                file: Some(path.to_path_buf()),
                ..evidence_core::Location::default()
            }),
            fix_hint: None,
            subcommand: Some("init".to_string()),
            root_cause_uid: None,
        })?;
    } else {
        println!("created: {:?}", path);
    }
    Ok(())
}

fn init_terminal(code: &'static str, severity: Severity, message: &str) -> Diagnostic {
    Diagnostic {
        code: code.to_string(),
        severity,
        message: message.to_string(),
        location: None,
        fix_hint: None,
        subcommand: Some("init".to_string()),
        root_cause_uid: None,
    }
}
