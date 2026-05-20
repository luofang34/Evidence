//! Data types serialized into the `ContextReport` JSON blob.
//!
//! The wire shape mirrors design spec §3.1 exactly. Field order is
//! load-bearing — the report is byte-locked against
//! `crates/cargo-evidence/tests/fixtures/golden_context.json` so any
//! accidental rename or reorder fires the golden test. The supporting
//! structs (`RequirementRef`, `ParentRef`, `TestRef`, `FloorRow`,
//! `BoundarySlice`, `Conventions`) live in dedicated sub-types here so
//! `serde` produces stable nested keys.
//!
//! All map-typed fields use `BTreeMap` and every array of structs is
//! sorted by a stable key (`id` for requirements, `name` for tests)
//! before serialization — see `lookup::build_report`.

use serde::{Deserialize, Serialize};

/// Resolved selector classification — what kind of input the resolver
/// matched. Carries both the raw `input` (what the caller wrote) and
/// the canonical `resolved` form (workspace-relative file path,
/// `[package].name`, or fully-qualified module path).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelectorView {
    /// Classification: `"workspace"`, `"file"`, `"crate"`, or
    /// `"module"`.
    pub kind: String,
    /// The raw input the caller passed, verbatim. Empty for the
    /// workspace overview path.
    pub input: String,
    /// Canonical resolved form. For files: workspace-relative path
    /// with forward slashes. For crates: the `[package].name` from
    /// `Cargo.toml`. For modules: the dotted path
    /// (`evidence_core::trace`). Empty for the workspace overview.
    pub resolved: String,
}

/// One LLR or HLR reference in the report. `layer` distinguishes the
/// origin file (`"llr"` for entries from `llr.toml`, `"hlr"` for
/// entries from `hlr.toml`). The wire shape stays the same so a
/// consumer can render any layer uniformly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequirementRef {
    /// Human-readable ID (e.g. `"LLR-001"`).
    pub id: String,
    /// Machine-stable UUID.
    pub uid: String,
    /// Origin layer (`"hlr"` or `"llr"`).
    pub layer: String,
    /// Title carried by the entry.
    pub title: String,
    /// Description (may be empty for entries that omit it).
    pub description: String,
    /// Implementation modules. Empty for HLRs.
    pub modules: Vec<String>,
    /// Diagnostic codes the entry owns. Empty for HLRs.
    pub emits: Vec<String>,
    /// Parent UIDs this entry traces up to.
    pub traces_to: Vec<String>,
    /// Verification methods declared on the entry.
    pub verification_methods: Vec<String>,
}

/// Parent HLR or SYS reference rolled up from a `RequirementRef`'s
/// `traces_to` list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParentRef {
    /// Human-readable ID.
    pub id: String,
    /// Machine-stable UUID.
    pub uid: String,
    /// Origin layer (`"sys"` or `"hlr"`).
    pub layer: String,
    /// Title carried by the entry.
    pub title: String,
    /// Parent UIDs (empty for SYS rows).
    pub traces_to: Vec<String>,
}

/// Test entry reference attached to one or more LLRs in the report's
/// `requirements` list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TestRef {
    /// Human-readable ID.
    pub id: String,
    /// Machine-stable UUID.
    pub uid: String,
    /// Test title.
    pub name: String,
    /// All resolved selectors (merged from `test_selector` +
    /// `test_selectors`).
    pub selectors: Vec<String>,
    /// Parent LLR UIDs.
    pub traces_to: Vec<String>,
}

/// One row in the floors slice for the selector's crate. Mirrors the
/// `[per_crate.<crate>]` / `[per_crate_ceilings.<crate>]` /
/// `[floors]` shape from `cert/floors.toml`. Workspace-wide rows
/// (`kind = "floor"`) are emitted only for the workspace-overview
/// path; per-crate rows are emitted for file/crate/module selectors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FloorRow {
    /// Dimension name (e.g. `"test_count"`, `"library_panics"`).
    pub dimension: String,
    /// Kind discriminator: `"floor"`, `"per_crate_floor"`, or
    /// `"per_crate_ceiling"`.
    pub kind: String,
    /// Current measured value.
    pub current: u64,
    /// Committed floor or ceiling.
    pub floor: u64,
}

/// Boundary policy slice — the per-crate facts an agent editing the
/// crate cares about. Workspace-overview reports populate `in_scope`
/// from the union; per-crate reports populate it from the lookup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoundarySlice {
    /// Whether the resolved crate is in `boundary.toml`'s
    /// `scope.in_scope`. Always `true` for the workspace-overview
    /// path (no specific crate to disqualify).
    pub in_scope: bool,
    /// Workspace crates explicitly forbidden as deps for in-scope
    /// crates. Carried straight through from
    /// `BoundaryConfig.scope.explicit_forbidden`.
    pub forbidden_deps: Vec<String>,
}

/// Conventions block — the layered `CLAUDE.md` pointer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Conventions {
    /// Workspace-relative path to the nearest `CLAUDE.md` (per-crate
    /// when the selector resolves into a crate, root otherwise).
    /// `None` when no `CLAUDE.md` is reachable from the workspace
    /// root.
    pub nearest_claude_md: Option<String>,
}

/// One non-fatal warning attached to the report. The same code names
/// the CLI's hand-emitted JSONL diagnostic so MCP consumers and CLI
/// consumers see the same vocabulary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextWarning {
    /// `CONTEXT_*` diagnostic code.
    pub code: String,
    /// Human one-liner explaining the warning.
    pub message: String,
}

/// Single-blob response — the canonical wire shape of `evidence
/// context`. Field order matches design spec §3.1; the golden fixture
/// pins the byte layout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextReport {
    /// What the resolver classified the selector as.
    pub selector: SelectorView,
    /// Crate this report is scoped to. Empty string for the
    /// workspace-overview path.
    #[serde(rename = "crate")]
    pub crate_name: String,
    /// Effective DAL for the resolved crate (`"A"` / `"B"` / `"C"` /
    /// `"D"`). `"D"` for the workspace overview (the lowest-rigor
    /// safe default).
    pub dal: String,
    /// LLR-level requirements governing the selector. Sorted by
    /// `id`.
    pub requirements: Vec<RequirementRef>,
    /// Parent HLR / SYS roll-up — every distinct UID reached from
    /// `requirements[*].traces_to` plus their parents.
    pub parents: Vec<ParentRef>,
    /// Tests that verify any of the listed requirements. Sorted by
    /// `name`.
    pub tests: Vec<TestRef>,
    /// Diagnostic codes the listed requirements collectively own.
    /// Sorted alphabetically.
    pub diagnostic_codes: Vec<String>,
    /// Floors / ceilings applicable to the resolved crate.
    pub floors: Vec<FloorRow>,
    /// Boundary slice.
    pub boundary: BoundarySlice,
    /// Conventions block.
    pub conventions: Conventions,
    /// Warnings the resolver / lookup attached. Ordered by emission.
    pub warnings: Vec<ContextWarning>,
}

/// Convenience constructor for the workspace-overview shape — every
/// per-selector field empty / default. Callers populate the
/// `selector` view themselves so they own the `kind` discriminator.
impl ContextReport {
    /// Build a `ContextReport` with `kind="workspace"` and the
    /// selector-specific fields at their lowest-information defaults
    /// (`crate_name=""`, `dal="D"`, empty arrays, no nearest
    /// `CLAUDE.md`). Callers can fill in the workspace-level fields
    /// after construction.
    pub fn workspace_default() -> Self {
        Self {
            selector: SelectorView {
                kind: "workspace".to_string(),
                input: String::new(),
                resolved: String::new(),
            },
            crate_name: String::new(),
            dal: "D".to_string(),
            requirements: Vec::new(),
            parents: Vec::new(),
            tests: Vec::new(),
            diagnostic_codes: Vec::new(),
            floors: Vec::new(),
            boundary: BoundarySlice {
                in_scope: true,
                forbidden_deps: Vec::new(),
            },
            conventions: Conventions {
                nearest_claude_md: None,
            },
            warnings: Vec::new(),
        }
    }
}
