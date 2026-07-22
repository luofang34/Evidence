<!-- GENERATED FILE. DO NOT EDIT.
     Source of truth: cert/trace/*.toml
     Regenerate: cargo xtask trace
-->

# Traceability Matrix

<!-- Source: cert/trace roots (see project.toml trace.roots) -->
**Document ID:** TOOL-HLR

## Schema & Provenance

- **HLR:** schema=0.0.1, document=TOOL-HLR, rev=1.0
- **LLR:** schema=0.0.1, document=TOOL-LLR, rev=1.0
- **Tests:** schema=0.0.1, document=TOOL-TESTS, rev=1.0

## HLR to LLR Traceability

| HLR ID | HLR Title | LLR IDs |
|--------|-----------|--------|
| HLR-001 | Every --format=jsonl run emits exactly one terminal event | LLR-001, LLR-069 |
| HLR-002 | stdout under --format=jsonl is strict JSONL | LLR-002 |
| HLR-003 | Diagnostic codes are locked: unique + regex + exhaustive | LLR-003 |
| HLR-004 | Terminal suffixes are reserved | LLR-004 |
| HLR-005 | FixHint is forward-compatible | LLR-005 |
| HLR-006 | Capture atomic git snapshot at start | LLR-006 |
| HLR-007 | Capture deterministic environment fingerprint | LLR-007 |
| HLR-008 | Write SHA256SUMS with deterministic ordering | LLR-008 |
| HLR-009 | Emit deterministic-manifest.json recipe projection | LLR-009 |
| HLR-010 | Finalize re-checks git SHA (TOCTOU guard) | LLR-010 |
| HLR-011 | Verify detects hash mismatch for every hashed file | LLR-011 |
| HLR-012 | Verify enforces env.json ↔ index.json consistency | LLR-012 |
| HLR-013 | Verify enforces DAL-map ↔ compliance consistency | LLR-013 |
| HLR-014 | Verify enforces trace_outputs are in SHA256SUMS | LLR-014 |
| HLR-015 | Strict-mode verify requires an ed25519 signature | LLR-015 |
| HLR-016 | Exit codes map to terminal events | LLR-016 |
| HLR-017 | --format resolution folds legacy --json | LLR-017 |
| HLR-018 | schema show diagnostic prints the embedded source | LLR-018 |
| HLR-019 | trace --validate enforces cross-tier links | LLR-019 |
| HLR-020 | Dispatch guards unwired --format=jsonl subcommands | LLR-020 |
| HLR-021 | Policy gate rejects HLR with empty traces_to | LLR-021 |
| HLR-022 | Test-selector resolution catches dangling pointers | LLR-022 |
| HLR-023 | Default --trace-roots discovery | LLR-023 |
| HLR-024 | CI self-check on enforcement flags | LLR-024 |
| HLR-025 | check auto-detects argument shape | LLR-025 |
| HLR-026 | check emits one diagnostic per requirement | LLR-026 |
| HLR-027 | Every REQ_GAP carries a mechanical FixHint where one exists | LLR-027 |
| HLR-028 | Test results come from captured workspace stdout | LLR-028 |
| HLR-029 | RULES is the single source of truth for the diagnostic vocabulary | LLR-029 |
| HLR-030 | RULES <-> source DiagnosticCode bijection is machine-enforced | LLR-030 |
| HLR-031 | Every code is claimed by an LLR via LLR.emits | LLR-031 |
| HLR-032 | TERMINAL_CODES <-> RULES.terminal=true is machine-enforced | LLR-032 |
| HLR-033 | Committed JSONL fixtures byte-lock the verify and check wire shapes | LLR-033 |
| HLR-034 | Tool's own compliance report stays green under its own generator | LLR-034 |
| HLR-035 | cert/floors.toml is the single source of truth for ratcheted measurements | LLR-035 |
| HLR-036 | CI enforces floors and ceilings on every push | LLR-036 |
| HLR-037 | Lowering a committed floor requires explicit written justification | LLR-037 |
| HLR-038 | HLR declares lateral surface of user-visible behaviors | LLR-038 |
| HLR-039 | TestEntry expresses N:M test-to-requirement mapping | LLR-039 |
| HLR-040 | Derived LLRs require written rationale | LLR-040 |
| HLR-041 | TraceValidationError::Link carries a typed sub-error enum | LLR-041 |
| HLR-042 | `trace --validate` emits one JSONL event per Link-phase sub-error | LLR-042 |
| HLR-043 | Every Link-phase sub-rule is listed in RULES with its own code | LLR-043 |
| HLR-044 | CI asserts no new rot-prone marker lands in .rs sources | LLR-044 |
| HLR-045 | Cross-time determinism is enforced by comparing every PR's recipe toolchain projection to the last successful main-branch build | LLR-045 |
| HLR-046 | CI gate asserts every narrative trace-ID reference resolves to a real trace entry | LLR-046 |
| HLR-047 | CI gate fails on any hand-rolled recursive fs::read_dir walker outside a reviewer-visible allowlist | LLR-047 |
| HLR-048 | cargo evidence doctor audits downstream rigor via a checklist of typed diagnostic codes | LLR-048 |
| HLR-049 | Pre-release builds embed a tool_prerelease flag in env.json and verify refuses such bundles under cert/record | LLR-049 |
| HLR-050 | Thin MCP server wraps check, rules, and doctor via subprocess over stdio | LLR-050, LLR-064, LLR-065 |
| HLR-051 | Per-test outcome records written to tests/test_outcomes.jsonl with failure-message capture from libtest stdout | LLR-051 |
| HLR-052 | Bundle records per-test requirement_uids; verify asserts every LLR is test-verified | LLR-052 |
| HLR-053 | generate --coverage flag invokes cargo-llvm-cov and writes a typed coverage report into the bundle | LLR-053, LLR-056 |
| HLR-054 | MCP tool handlers reject unknown fields + emit fallback signal when workspace_path is omitted | LLR-054 |
| HLR-055 | cmd_trace + cmd_generate pass derived.toml requirements into Link-phase validation | LLR-055 |
| HLR-056 | generate compares per-level coverage aggregate against the DAL engineering gates | LLR-057, LLR-058, LLR-059 |
| HLR-057 | cli::doctor derives its trace DAL via load_max_dal, not default_dal | LLR-060 |
| HLR-058 | generate threads trace_validation_passed bool from Phase 6 to write_compliance_reports | LLR-061 |
| HLR-059 | evidence-mcp ServerHandler returns {name: evidence-mcp, version: CARGO_PKG_VERSION} in get_info | LLR-062 |
| HLR-060 | evidence-mcp probes cargo evidence --version at startup and prepends MCP_VERSION_SKEW / MCP_VERSION_PROBE_FAILED | LLR-063 |
| HLR-061 | MCP exposes evidence_ping as a cheap liveness + version-skew probe that does not spawn a subprocess | LLR-066 |
| HLR-062 | MCP exposes evidence_floors so agents can query the ratchet-gate state | LLR-067 |
| HLR-063 | MCP exposes evidence_diff so agents can compare two bundles | LLR-068 |
| HLR-064 | Boundary policy can forbid build scripts in in-scope crates | LLR-070, LLR-072 |
| HLR-065 | Boundary policy can forbid proc-macros in in-scope crates | LLR-071, LLR-072 |
| HLR-066 | DAL-A in-scope crate without auxiliary MC/DC tool reference fails cert/record generate | LLR-073 |
| HLR-067 | MCP tool responses carry an explicit success boolean | LLR-074 |
| HLR-068 | Direct cargo-evidence --help invocation lists subcommands, not a redirect stub | LLR-075 |
| HLR-069 | Repository contains no editor-duplicate artifacts (` N.<ext>` filenames) | LLR-076 |
| HLR-070 | cargo evidence keygen: explicit create + rotate | LLR-077 |
| HLR-071 | Generate refuses on signing.pub anchor mismatch | LLR-078 |
| HLR-072 | Repo demonstrates lean-layered CLAUDE.md doctrine | LLR-079 |
| HLR-073 | cargo evidence context CLI verb returns per-module trace slice | LLR-080, LLR-081, LLR-082, LLR-083 |
| HLR-074 | evidence_context MCP tool returns per-module trace slice | LLR-084, LLR-085, LLR-086 |
| HLR-075 | cargo evidence init --with-agent-context scaffolds downstream CLAUDE.md | LLR-090 |
| HLR-076 | In-scope package names resolve to manifest directories; workspace-control inputs are captured; empty or unresolved scope fails closed | LLR-091, LLR-092 |
| HLR-077 | Generate captures tests via nextest libtest-json-plus preserving per-binary identity; verify fails closed on lost identity | LLR-093, LLR-094, LLR-097 |
| HLR-078 | Generate inventories workspace compiler artifacts and hashes each deliverable; verify fails closed on empty outputs when a build ran | LLR-095, LLR-096 |
| HLR-079 | corpus.toml is a strict, layout-agnostic index of linked graph files | LLR-100 |
| HLR-080 | Corpus graph enforces uid identity and resolvable typed edges | LLR-098 |
| HLR-081 | Legacy trace documents load as graph nodes at exact parity | LLR-099 |
| HLR-082 | Traceability matrix rows and relationships are derived from the canonical corpus graph | LLR-101 |
| HLR-083 | Trace floor dimensions are derived from the canonical corpus graph | LLR-102 |
| HLR-084 | Assurance bijections are derived from canonical corpus graph mappings | LLR-103 |
| HLR-085 | Requirement gap reports are canonical corpus graph queries | LLR-104 |
| HLR-086 | One shared trace-evidence evaluation classifies adoption states and every consumer fails closed | LLR-105, LLR-107 |
| HLR-087 | Derived requirements carry disposition and review completeness enforced under policy | LLR-106 |
| HLR-088 | Coverage verdicts map engineering metrics to honest A-7 statuses with separate disposition evidence | LLR-108 |
| HLR-089 | Cert and record evaluation fails closed without an explicit assurance selection; reports bind a versioned standards pack | LLR-109, LLR-110 |
| HLR-090 | Review approval binds a versioned canonical requirement-content projection with a stable typed digest | LLR-111, LLR-112 |
| HLR-091 | Native and legacy requirements retain review-sensitive content and expose the same projection | LLR-113 |
| HLR-092 | Review decision records load through a strict fail-closed schema bound to requirement uid and content digest | LLR-114, LLR-116 |
| HLR-093 | Review nodes carry typed review and supersession edges validated as deterministic chains | LLR-115 |
| HLR-094 | Lifecycle evaluation derives one deterministic state per requirement from effective digest-bound review heads | LLR-117 |
| HLR-095 | Lifecycle state is an evaluated-only view reported deterministically by requirement uid | LLR-118 |
| HLR-096 | Approval enforcement is an explicit caller-named policy with no default and no assurance-level inference | LLR-119 |
| HLR-097 | Explicit enforcement gates test verifies edges and implementation claims to approved requirements with distinct typed diagnostics | LLR-120, LLR-121 |
| HLR-098 | Proposal records carry exactly two representable actions in a strict schema-gated fail-closed TOML schema | LLR-122 |
| HLR-099 | Proposal append is store-minted, fail-closed, non-overwriting, and confined to a validated proposal root | LLR-123, LLR-124 |
| HLR-100 | Source revision records load through a strict fail-closed schema with typed material state and exact capture combinations | LLR-125 |
| HLR-101 | Source revision nodes are corpus graph identity loaded through the activated sources index kind before requirements and reviews | LLR-126, LLR-127 |
| HLR-102 | Source revisions own an optional supersedes link validated as one single acyclic chain per document key | LLR-128, LLR-129, LLR-130 |
| HLR-103 | Source baseline transitions are pure UID-preserving immutable-superset comparisons with distinct typed failures | LLR-132 |
| HLR-104 | Effective source heads are a deterministic derived view keyed by document key | LLR-131 |
| HLR-105 | The sources lock is a strict versioned canonical TOML inventory of effective source heads | LLR-133, LLR-134 |
| HLR-106 | Committed sources-lock validation applies three ordered exact gates with typed failures and never mutates the workspace | LLR-135 |
| HLR-107 | Each effective source revision verifies to a deterministic typed state with vendored byte verification beneath the fixed payload root | LLR-136, LLR-137 |
| HLR-108 | Batch source verification gates on global graph and lock prerequisites, then reports one sorted finding per effective head without mutation or network access | LLR-138 |
| HLR-109 | Generate applies one locked/offline resolution policy to every cargo subprocess; online resolution is a development-only opt-in that cert/record refuses | LLR-139, LLR-140 |
| HLR-110 | The bundle binds the resolved dependency graph and records the resolution policy; verification rejects an online-resolution cert/record bundle | LLR-141, LLR-142 |
| HLR-111 | The bundle binds a canonical recipe manifest and records its SHA-256 as index.json.recipe_hash | LLR-143, LLR-144, LLR-145 |
| HLR-112 | Reproduced-output comparison reports typed findings over input, recipe, and output digest planes | LLR-146 |

## LLR to Test Traceability

| LLR ID | LLR Title | Test IDs |
|--------|-----------|----------|
| LLR-001 | cmd_verify_jsonl emits a terminal on every exit path | TEST-001 |
| LLR-002 | emit_jsonl flushes stdout per event | TEST-002 |
| LLR-003 | DiagnosticCode impls use exhaustive match self | TEST-003 |
| LLR-004 | TERMINAL_CODES is the source of truth for terminals | TEST-004 |
| LLR-005 | FixHint::Other catches unknown kind via #[serde(other)] | TEST-005 |
| LLR-006 | GitSnapshot::capture queries git once at builder construction | TEST-006 |
| LLR-007 | env_fingerprint records toolchain + host identity | TEST-007 |
| LLR-008 | write_sha256sums sorts and normalizes paths | TEST-008 |
| LLR-009 | RecipeManifest projects the canonical recipe identity | TEST-009, TEST-160 |
| LLR-010 | EvidenceBuilder::finalize re-reads git SHA | TEST-010 |
| LLR-011 | verify_hash_list rehashes every SHA256SUMS entry | TEST-011 |
| LLR-012 | check_env_vs_index cross-checks duplicated fields | TEST-012 |
| LLR-013 | check_dal_map cross-checks index.json ↔ compliance/ | TEST-013 |
| LLR-014 | check_trace_outputs_hashed enforces trace-file hashing | TEST-014 |
| LLR-015 | cmd_verify_jsonl strict-mode ed25519 signature guard | TEST-015 |
| LLR-016 | terminal_{ok,fail,error} construct fixed-code diagnostics | TEST-016 |
| LLR-017 | OutputFormat::resolve folds --json + --format precedence | TEST-017 |
| LLR-018 | cmd_schema_show writes embedded schema source to stdout | TEST-018 |
| LLR-019 | validate_trace_links_with_policy is two-phase | TEST-019 |
| LLR-020 | dispatch guard emits CLI_SUBCOMMAND_ERROR terminal | TEST-020 |
| LLR-021 | validate_trace_links_with_policy honors require_hlr_sys_trace | TEST-021 |
| LLR-022 | selector_check::resolve_test_selectors greps source tree | TEST-022 |
| LLR-023 | default_trace_roots picks cert/trace or cert/trace | TEST-023 |
| LLR-024 | ci_self_check greps committed ci.yml for both flags | TEST-024 |
| LLR-025 | cmd_check dispatches on --mode and argument shape | TEST-025, TEST-028 |
| LLR-026 | build_requirement_report emits one diag per entry | TEST-027 |
| LLR-027 | fix_hint_for_gap picks the right FixHint variant per sub-case | TEST-026 |
| LLR-028 | parse_cargo_test_output builds a per-test outcome map | TEST-106 |
| LLR-029 | RULES const + rules subcommand + rules_json() | TEST-029 |
| LLR-030 | Locked-codes invariant: RULES <-> source DiagnosticCode walk | TEST-030 |
| LLR-031 | Locked-codes invariant: every code is claimed by >=1 LLR.emits | TEST-031 |
| LLR-032 | Locked-codes invariant: RULES.terminal matches TERMINAL_CODES | TEST-032 |
| LLR-033 | Byte-diff test pins verify + check wire fixtures | TEST-033 |
| LLR-034 | Self-compliance baseline diff test | TEST-034 |
| LLR-035 | evidence_core::floors measurement helpers + cert/floors.toml parser | TEST-035 |
| LLR-036 | cmd_floors runs the gate + JSONL support | TEST-036 |
| LLR-037 | floors-lower-lint refuses silent decreases via a PR-body grep | TEST-037 |
| LLR-038 | HlrEntry.surfaces + KNOWN_SURFACES bijection | TEST-038 |
| LLR-039 | StringOrVec deserializer for test_selectors | TEST-039 |
| LLR-040 | validate_derived_has_rationale Link-phase rule | TEST-040 |
| LLR-041 | LinkError enum with per-variant DiagnosticCode impl | TEST-041 |
| LLR-042 | cmd_trace stream-emits one Diagnostic per LinkError | TEST-042 |
| LLR-043 | RULES registers every LinkError variant code; bijection closed | TEST-043 |
| LLR-044 | rot_prone_markers_locked integration test | TEST-044 |
| LLR-045 | cross-time-determinism CI job + baseline-override lint | TEST-045 |
| LLR-046 | trace_id_refs_locked integration test | TEST-046 |
| LLR-047 | walker_usage_locked integration test | TEST-047 |
| LLR-048 | cmd_doctor subcommand implementation | TEST-048 |
| LLR-049 | Pre-release detection at build time + verify-side refusal | TEST-049 |
| LLR-050 | evidence-mcp crate exposes three rmcp tools over stdio | TEST-050, TEST-069 |
| LLR-051 | Extend libtest parser with failure-block capture; emit TestOutcomeRecord stream | TEST-051 |
| LLR-052 | resolve_llr_backlinks enriches TestOutcomeRecord + check_llr_test_selectors asserts reverse | TEST-052 |
| LLR-053 | coverage subprocess + llvm-cov JSON parser + bundle write | TEST-053 |
| LLR-054 | schema deny_unknown_fields + MCP_WORKSPACE_FALLBACK synthetic diagnostic | TEST-054 |
| LLR-055 | Wire derived entries at cli/trace.rs + cli/generate/phases.rs | TEST-055 |
| LLR-056 | parse_llvm_cov_export populates FileMeasurement.branches structurally | TEST-056 |
| LLR-057 | aggregate_lines_percent sums per_file[].lines over a Statement-level Measurement | TEST-057 |
| LLR-058 | aggregate_branches_percent sums per_file[].branches over a Branch-level Measurement | TEST-058 |
| LLR-059 | threshold_violations dispatches to level-appropriate aggregator; strict < comparison | TEST-059 |
| LLR-060 | load_max_dal: max over dal_map values, fall back to default_dal on empty in_scope, D on load failure | TEST-060 |
| LLR-061 | validate_trace_links_phase returns TraceValidationResult; write_compliance_reports takes trace_validation_passed: bool | TEST-061 |
| LLR-062 | tool_handler attribute passes name = evidence-mcp; lib.rs is facade-only | TEST-062, TEST-118 |
| LLR-063 | detect_with_probe + probe_cli_version + skew_diagnostic | TEST-063 |
| LLR-064 | MCP tool-layer failures surface as structured diagnostics carrying typed MCP_* codes | TEST-064, TEST-071 |
| LLR-065 | EVIDENCE_MCP_TIMEOUT_SECS tunes the per-spawn subprocess cap | TEST-065 |
| LLR-066 | evidence_ping tool returns cached VersionSkew + mcp/cli version strings | TEST-066 |
| LLR-067 | evidence_floors handler wraps cargo evidence floors --format=jsonl | TEST-067, TEST-070 |
| LLR-068 | evidence_diff handler wraps cargo evidence diff --json | TEST-068 |
| LLR-069 | verify --verify-key I/O failure emits VERIFY_RUNTIME_READ_VERIFY_KEY + VERIFY_ERROR terminal | TEST-072 |
| LLR-070 | check_no_build_rs flags in-scope crates with kind=custom-build targets | TEST-075, TEST-076, TEST-078 |
| LLR-071 | check_no_proc_macros flags in-scope crates with kind=proc-macro targets | TEST-075, TEST-077, TEST-078 |
| LLR-072 | cargo_metadata.json bundle artifact + verify-time recheck of forbid_build_rs / forbid_proc_macros | TEST-079 |
| LLR-073 | check_dal_a_mcdc_evidence + enforce_dal_qualification gate | TEST-080 |
| LLR-074 | ToolResponse.success boolean derivation in evidence-mcp | TEST-081 |
| LLR-075 | main.rs --help intercept reuses clap's render tree | TEST-082 |
| LLR-076 | editor_duplicates_locked walks the repo and fires on ` N.<ext>` filenames | TEST-083 |
| LLR-077 | cmd_keygen create + rotate dispatch | TEST-084 |
| LLR-078 | finalize::check_pubkey_anchor refuses on mismatch | TEST-085 |
| LLR-079 | layered_claude_md_doctrine enforces per-crate CLAUDE.md cap + scope | TEST-086 |
| LLR-080 | context::resolver classifies selectors with File>Crate>Module priority | TEST-087 |
| LLR-081 | context::lookup composes ContextReport from trace/boundary/floors/CLAUDE.md | TEST-088 |
| LLR-082 | cli::context wires CLI verb to context_for + emits CONTEXT_* terminals | TEST-107 |
| LLR-083 | context content codes register in RULES and gate the golden wire shape | TEST-089, TEST-090 |
| LLR-084 | evidence_context handler wraps cargo evidence context --json | TEST-091, TEST-092, TEST-093 |
| LLR-085 | ContextRequest / ContextToolResponse define the evidence_context wire shape | TEST-094 |
| LLR-086 | context_roundtrip integration test pins the MCP wire shape against the CLI | TEST-108 |
| LLR-090 | write_agent_context_files emits root CLAUDE.md + .claude/settings.json | TEST-097 |
| LLR-091 | package-name -> manifest-dir resolution + control-input planning + generate-time fail-closed | TEST-098, TEST-099, TEST-100, TEST-101, TEST-104, TEST-105 |
| LLR-092 | verify fails closed on an empty source baseline | TEST-102, TEST-103 |
| LLR-093 | nextest libtest-json-plus parser preserves per-test identity | TEST-109, TEST-110, TEST-116 |
| LLR-094 | verify fails closed on lost test-execution identity | TEST-111 |
| LLR-095 | compiler-artifact inventory hashes workspace deliverables into outputs_hashes.json | TEST-112, TEST-113, TEST-117 |
| LLR-096 | verify fails closed on an empty output manifest when a build ran | TEST-114 |
| LLR-097 | generator closure runs the release verifier before emitting GENERATE_OK | TEST-115 |
| LLR-098 | corpus::graph enforces identity and canonical typed edges | TEST-120, TEST-122 |
| LLR-099 | corpus::legacy adapts the four-file trace into graph nodes | TEST-121 |
| LLR-100 | corpus::index parses and resolves the corpus.toml file index | TEST-119, TEST-122 |
| LLR-101 | matrix view projects canonical corpus nodes and edges into the shared renderer | TEST-123 |
| LLR-102 | floors counts validated corpus nodes by requirement layer and test kind | TEST-124 |
| LLR-103 | assurance view indexes graph-derived surface and diagnostic claimants | TEST-125 |
| LLR-104 | requirement report projects canonical graph relationships and statuses | TEST-126 |
| LLR-105 | evaluate_trace_evidence classifies trace roots into one semantic adoption state | TEST-127 |
| LLR-106 | derived completeness policy gates disposition, review, and safety impact | TEST-128 |
| LLR-107 | doctor and bundle verify fail closed on missing or empty trace evidence | TEST-128 |
| LLR-108 | coverage_verdict truth table and the coverage_disposition evidence field | TEST-129 |
| LLR-109 | AssuranceSelection resolution and the fail-closed named-claim gate | TEST-130 |
| LLR-110 | Compliance reports bind the versioned DO-178C standards pack and name the assurance level | TEST-130 |
| LLR-111 | corpus::review_content projects the v1 normative content and encodes canonical bytes | TEST-131, TEST-132 |
| LLR-112 | corpus::digest types the lowercase SHA-256 review-content digest and fails closed on malformed input | TEST-131 |
| LLR-113 | Requirement nodes and both loaders retain the review-sensitive content fields | TEST-132 |
| LLR-114 | corpus::review_records parses strict review files and validates every record field closed | TEST-133 |
| LLR-115 | The corpus graph types review nodes and edges and validates supersession chains deterministically | TEST-133, TEST-134 |
| LLR-116 | The corpus index resolves review files through the same deterministic mechanism and loads requirements first | TEST-134 |
| LLR-117 | corpus::lifecycle evaluates the lifecycle truth table over effective review heads as a pure function | TEST-135 |
| LLR-118 | evaluate_all_lifecycles reports every requirement in uid order over deterministic graph review accessors | TEST-136 |
| LLR-119 | corpus::approval_boundary types make the enforcement policy explicit with no default and no silent weakening | TEST-137 |
| LLR-120 | validate_approval_boundary scans verifies edges and metadata claims against evaluated lifecycles in deterministic order | TEST-137 |
| LLR-121 | the approval boundary fails closed over zero reviews and never grandfathers legacy graphs | TEST-138 |
| LLR-122 | corpus::proposal schema types represent exactly two actions and fail closed on every other shape | TEST-139 |
| LLR-123 | ProposalStore validates its root, mints all identities, writes exclusively, and reads back fail-closed | TEST-139, TEST-140 |
| LLR-124 | revision guards enforce candidate lifecycle and optimistic concurrency with typed ProposalError context | TEST-140 |
| LLR-125 | corpus::source::records parses strict source-revision files and validates every record field closed | TEST-141 |
| LLR-126 | corpus::graph::nodes carries the source-revision node kind with per-kind identity and canonical iteration | TEST-142 |
| LLR-127 | corpus::index resolves the sources kind and loads sources before requirements and reviews | TEST-143 |
| LLR-128 | corpus::source::records projects the optional supersedes field into one owned source Supersedes edge | TEST-144 |
| LLR-129 | corpus::graph::validation permits SourceRevision Supersedes SourceRevision endpoints and nothing else | TEST-145 |
| LLR-130 | corpus::source::lineage validates single-chain document lineage inside CorpusGraph::validate | TEST-146 |
| LLR-131 | corpus::source::lineage derives effective source heads as a sorted document-key map | TEST-147 |
| LLR-132 | corpus::source::lineage compares source baselines as a pure immutable-superset transition | TEST-148 |
| LLR-133 | corpus::source::lock derives the lock value from effective source heads as a pure projection | TEST-149 |
| LLR-134 | corpus::source::lock renders one canonical byte template and parses strict lock bytes | TEST-150, TEST-152 |
| LLR-135 | corpus::source::lock validates a committed lock through three ordered gates and reads lock files without mutation | TEST-151 |
| LLR-136 | corpus::source::verify reports one typed verification state per effective head capture mode | TEST-153 |
| LLR-137 | corpus::source::verify resolves vendored payloads beneath the fixed sources root and hashes raw bytes with typed payload errors | TEST-154 |
| LLR-138 | corpus::source::verify gates the batch on the committed-lock gates and isolates sorted per-head findings | TEST-155 |
| LLR-139 | policy::resolution::ResolutionPolicy gates locked_offline vs online_opt_in on profile and owns the online-resolution refusal | TEST-156 |
| LLR-140 | every generate-time cargo subprocess carries the resolution policy flags; locked/offline failures map to BUNDLE_LOCKED_GRAPH_UNAVAILABLE | TEST-157 |
| LLR-141 | cargo_metadata.json binds the resolved dependency graph and is written for every cert/record bundle | TEST-158 |
| LLR-142 | EvidenceIndex.resolution_policy records the policy; verify rejects an online-resolution cert/record bundle | TEST-159 |
| LLR-143 | EvidenceIndex.recipe_hash renames deterministic_hash behind a read-time serde alias | TEST-161 |
| LLR-144 | EvidenceBuilder::finalize computes the recipe inputs and writes the canonical recipe manifest | TEST-162 |
| LLR-145 | Verify re-checks recipe_hash and re-projects the recipe manifest from bundle content | TEST-162 |
| LLR-146 | compare_reproduction classifies two bundles into typed reproduction findings | TEST-163 |

## Reverse Trace: Test to LLR to HLR

| Test ID | LLR IDs | HLR IDs |
|---------|---------|--------|
| TEST-001 | LLR-001 | HLR-001 |
| TEST-002 | LLR-002 | HLR-002 |
| TEST-003 | LLR-003 | HLR-003 |
| TEST-004 | LLR-004 | HLR-004 |
| TEST-005 | LLR-005 | HLR-005 |
| TEST-006 | LLR-006 | HLR-006 |
| TEST-007 | LLR-007 | HLR-007 |
| TEST-008 | LLR-008 | HLR-008 |
| TEST-009 | LLR-009 | HLR-009 |
| TEST-010 | LLR-010 | HLR-010 |
| TEST-011 | LLR-011 | HLR-011 |
| TEST-012 | LLR-012 | HLR-012 |
| TEST-013 | LLR-013 | HLR-013 |
| TEST-014 | LLR-014 | HLR-014 |
| TEST-015 | LLR-015 | HLR-015 |
| TEST-016 | LLR-016 | HLR-016 |
| TEST-017 | LLR-017 | HLR-017 |
| TEST-018 | LLR-018 | HLR-018 |
| TEST-019 | LLR-019 | HLR-019 |
| TEST-020 | LLR-020 | HLR-020 |
| TEST-021 | LLR-021 | HLR-021 |
| TEST-022 | LLR-022 | HLR-022 |
| TEST-023 | LLR-023 | HLR-023 |
| TEST-024 | LLR-024 | HLR-024 |
| TEST-025 | LLR-025 | HLR-025 |
| TEST-026 | LLR-027 | HLR-027 |
| TEST-027 | LLR-026 | HLR-026 |
| TEST-028 | LLR-025 | HLR-025 |
| TEST-029 | LLR-029 | HLR-029 |
| TEST-030 | LLR-030 | HLR-030 |
| TEST-031 | LLR-031 | HLR-031 |
| TEST-032 | LLR-032 | HLR-032 |
| TEST-033 | LLR-033 | HLR-033 |
| TEST-034 | LLR-034 | HLR-034 |
| TEST-035 | LLR-035 | HLR-035 |
| TEST-036 | LLR-036 | HLR-036 |
| TEST-037 | LLR-037 | HLR-037 |
| TEST-038 | LLR-038 | HLR-038 |
| TEST-039 | LLR-039 | HLR-039 |
| TEST-040 | LLR-040 | HLR-040 |
| TEST-041 | LLR-041 | HLR-041 |
| TEST-042 | LLR-042 | HLR-042 |
| TEST-043 | LLR-043 | HLR-043 |
| TEST-044 | LLR-044 | HLR-044 |
| TEST-045 | LLR-045 | HLR-045 |
| TEST-046 | LLR-046 | HLR-046 |
| TEST-047 | LLR-047 | HLR-047 |
| TEST-048 | LLR-048 | HLR-048 |
| TEST-049 | LLR-049 | HLR-049 |
| TEST-050 | LLR-050 | HLR-050 |
| TEST-051 | LLR-051 | HLR-051 |
| TEST-052 | LLR-052 | HLR-052 |
| TEST-053 | LLR-053 | HLR-053 |
| TEST-054 | LLR-054 | HLR-054 |
| TEST-055 | LLR-055 | HLR-055 |
| TEST-056 | LLR-056 | HLR-053 |
| TEST-057 | LLR-057 | HLR-056 |
| TEST-058 | LLR-058 | HLR-056 |
| TEST-059 | LLR-059 | HLR-056 |
| TEST-060 | LLR-060 | HLR-057 |
| TEST-061 | LLR-061 | HLR-058 |
| TEST-062 | LLR-062 | HLR-059 |
| TEST-063 | LLR-063 | HLR-060 |
| TEST-064 | LLR-064 | HLR-050 |
| TEST-065 | LLR-065 | HLR-050 |
| TEST-066 | LLR-066 | HLR-061 |
| TEST-067 | LLR-067 | HLR-062 |
| TEST-068 | LLR-068 | HLR-063 |
| TEST-069 | LLR-050 | HLR-050 |
| TEST-070 | LLR-067 | HLR-062 |
| TEST-071 | LLR-064 | HLR-050 |
| TEST-072 | LLR-069 | HLR-001 |
| TEST-075 | LLR-070, LLR-071 | HLR-064, HLR-065 |
| TEST-076 | LLR-070 | HLR-064 |
| TEST-077 | LLR-071 | HLR-065 |
| TEST-078 | LLR-070, LLR-071 | HLR-064, HLR-065 |
| TEST-079 | LLR-072 | HLR-064, HLR-065 |
| TEST-080 | LLR-073 | HLR-066 |
| TEST-081 | LLR-074 | HLR-067 |
| TEST-082 | LLR-075 | HLR-068 |
| TEST-083 | LLR-076 | HLR-069 |
| TEST-084 | LLR-077 | HLR-070 |
| TEST-085 | LLR-078 | HLR-071 |
| TEST-086 | LLR-079 | HLR-072 |
| TEST-087 | LLR-080 | HLR-073 |
| TEST-088 | LLR-081 | HLR-073 |
| TEST-089 | LLR-083 | HLR-073 |
| TEST-090 | LLR-083 | HLR-073 |
| TEST-091 | LLR-084 | HLR-074 |
| TEST-092 | LLR-084 | HLR-074 |
| TEST-093 | LLR-084 | HLR-074 |
| TEST-094 | LLR-085 | HLR-074 |
| TEST-097 | LLR-090 | HLR-075 |
| TEST-098 | LLR-091 | HLR-076 |
| TEST-099 | LLR-091 | HLR-076 |
| TEST-100 | LLR-091 | HLR-076 |
| TEST-101 | LLR-091 | HLR-076 |
| TEST-102 | LLR-092 | HLR-076 |
| TEST-103 | LLR-092 | HLR-076 |
| TEST-104 | LLR-091 | HLR-076 |
| TEST-105 | LLR-091 | HLR-076 |
| TEST-106 | LLR-028 | HLR-028 |
| TEST-107 | LLR-082 | HLR-073 |
| TEST-108 | LLR-086 | HLR-074 |
| TEST-109 | LLR-093 | HLR-077 |
| TEST-110 | LLR-093 | HLR-077 |
| TEST-111 | LLR-094 | HLR-077 |
| TEST-112 | LLR-095 | HLR-078 |
| TEST-113 | LLR-095 | HLR-078 |
| TEST-114 | LLR-096 | HLR-078 |
| TEST-115 | LLR-097 | HLR-077 |
| TEST-116 | LLR-093 | HLR-077 |
| TEST-117 | LLR-095 | HLR-078 |
| TEST-118 | LLR-062 | HLR-059 |
| TEST-119 | LLR-100 | HLR-079 |
| TEST-120 | LLR-098 | HLR-080 |
| TEST-121 | LLR-099 | HLR-081 |
| TEST-122 | LLR-098, LLR-100 | HLR-079, HLR-080 |
| TEST-123 | LLR-101 | HLR-082 |
| TEST-124 | LLR-102 | HLR-083 |
| TEST-125 | LLR-103 | HLR-084 |
| TEST-126 | LLR-104 | HLR-085 |
| TEST-127 | LLR-105 | HLR-086 |
| TEST-128 | LLR-106, LLR-107 | HLR-086, HLR-087 |
| TEST-129 | LLR-108 | HLR-088 |
| TEST-130 | LLR-109, LLR-110 | HLR-089 |
| TEST-131 | LLR-111, LLR-112 | HLR-090 |
| TEST-132 | LLR-111, LLR-113 | HLR-090, HLR-091 |
| TEST-133 | LLR-114, LLR-115 | HLR-092, HLR-093 |
| TEST-134 | LLR-115, LLR-116 | HLR-092, HLR-093 |
| TEST-135 | LLR-117 | HLR-094 |
| TEST-136 | LLR-118 | HLR-095 |
| TEST-137 | LLR-119, LLR-120 | HLR-096, HLR-097 |
| TEST-138 | LLR-121 | HLR-097 |
| TEST-139 | LLR-122, LLR-123 | HLR-098, HLR-099 |
| TEST-140 | LLR-123, LLR-124 | HLR-099 |
| TEST-141 | LLR-125 | HLR-100 |
| TEST-142 | LLR-126 | HLR-101 |
| TEST-143 | LLR-127 | HLR-101 |
| TEST-144 | LLR-128 | HLR-102 |
| TEST-145 | LLR-129 | HLR-102 |
| TEST-146 | LLR-130 | HLR-102 |
| TEST-147 | LLR-131 | HLR-104 |
| TEST-148 | LLR-132 | HLR-103 |
| TEST-149 | LLR-133 | HLR-105 |
| TEST-150 | LLR-134 | HLR-105 |
| TEST-151 | LLR-135 | HLR-106 |
| TEST-152 | LLR-134 | HLR-105 |
| TEST-153 | LLR-136 | HLR-107 |
| TEST-154 | LLR-137 | HLR-107 |
| TEST-155 | LLR-138 | HLR-108 |
| TEST-156 | LLR-139 | HLR-109 |
| TEST-157 | LLR-140 | HLR-109 |
| TEST-158 | LLR-141 | HLR-110 |
| TEST-159 | LLR-142 | HLR-110 |
| TEST-160 | LLR-009 | HLR-009 |
| TEST-161 | LLR-143 | HLR-111 |
| TEST-162 | LLR-144, LLR-145 | HLR-111 |
| TEST-163 | LLR-146 | HLR-112 |

## Annotations

- HLR HLR-001: scope=component
- HLR HLR-002: scope=component
- HLR HLR-003: scope=component
- HLR HLR-004: scope=component
- HLR HLR-005: scope=component
- HLR HLR-006: scope=component
- HLR HLR-007: scope=component
- HLR HLR-008: scope=component
- HLR HLR-009: scope=component
- HLR HLR-010: scope=component
- HLR HLR-011: scope=component
- HLR HLR-012: scope=component
- HLR HLR-013: scope=component
- HLR HLR-014: scope=component
- HLR HLR-015: scope=component
- HLR HLR-016: scope=component
- HLR HLR-017: scope=component
- HLR HLR-018: scope=component
- HLR HLR-019: scope=component
- HLR HLR-020: scope=component
- HLR HLR-021: scope=component
- HLR HLR-022: scope=component
- HLR HLR-023: scope=component
- HLR HLR-024: scope=component
- HLR HLR-025: scope=component
- HLR HLR-026: scope=component
- HLR HLR-027: scope=component
- HLR HLR-028: scope=component
- HLR HLR-029: scope=component
- HLR HLR-030: scope=component
- HLR HLR-031: scope=component
- HLR HLR-032: scope=component
- HLR HLR-033: scope=component
- HLR HLR-034: scope=component
- HLR HLR-035: scope=component
- HLR HLR-036: scope=component
- HLR HLR-037: scope=component
- HLR HLR-038: scope=component
- HLR HLR-039: scope=component
- HLR HLR-040: scope=component
- HLR HLR-041: scope=component
- HLR HLR-042: scope=component
- HLR HLR-043: scope=component
- HLR HLR-044: scope=component
- HLR HLR-045: scope=component
- HLR HLR-046: scope=component
- HLR HLR-047: scope=component
- HLR HLR-048: scope=component
- HLR HLR-049: scope=component
- HLR HLR-050: scope=component
- HLR HLR-051: scope=component
- HLR HLR-052: scope=component
- HLR HLR-053: scope=component
- HLR HLR-054: scope=component
- HLR HLR-055: scope=component
- HLR HLR-056: scope=component
- HLR HLR-057: scope=component
- HLR HLR-058: scope=component
- HLR HLR-059: scope=component
- HLR HLR-060: scope=component
- HLR HLR-061: scope=component
- HLR HLR-062: scope=component
- HLR HLR-063: scope=component
- HLR HLR-064: scope=component
- HLR HLR-065: scope=component
- HLR HLR-066: scope=component
- HLR HLR-067: scope=component
- HLR HLR-068: scope=component
- HLR HLR-069: scope=component
- HLR HLR-070: scope=component
- HLR HLR-071: scope=component
- HLR HLR-072: scope=component
- HLR HLR-073: scope=component
- HLR HLR-074: scope=component
- HLR HLR-075: scope=component
- HLR HLR-076: scope=component
- HLR HLR-077: scope=component
- HLR HLR-078: scope=component
- HLR HLR-079: scope=component
- HLR HLR-080: scope=component
- HLR HLR-081: scope=component
- HLR HLR-082: scope=component
- HLR HLR-083: scope=component
- HLR HLR-084: scope=component
- HLR HLR-085: scope=component
- HLR HLR-086: scope=component
- HLR HLR-087: scope=component
- HLR HLR-088: scope=component
- HLR HLR-089: scope=component
- HLR HLR-090: scope=component
- HLR HLR-091: scope=component
- HLR HLR-092: scope=component
- HLR HLR-093: scope=component
- HLR HLR-094: scope=component
- HLR HLR-095: scope=component
- HLR HLR-096: scope=component
- HLR HLR-097: scope=component
- HLR HLR-098: scope=component
- HLR HLR-099: scope=component
- HLR HLR-100: scope=component
- HLR HLR-101: scope=component
- HLR HLR-102: scope=component
- HLR HLR-103: scope=component
- HLR HLR-104: scope=component
- HLR HLR-105: scope=component
- HLR HLR-106: scope=component
- HLR HLR-107: scope=component
- HLR HLR-108: scope=component
- HLR HLR-109: scope=component
- HLR HLR-110: scope=component
- HLR HLR-111: scope=component
- HLR HLR-112: scope=component
- LLR LLR-001: modules=[cargo_evidence::cli::verify::cmd_verify_jsonl]
- LLR LLR-002: modules=[cargo_evidence::cli::output::emit_jsonl]
- LLR LLR-003: modules=[evidence_core::diagnostic::DiagnosticCode, evidence_core::verify::errors::VerifyError, evidence_core::hash::HashError]
- LLR LLR-004: modules=[evidence_core::diagnostic::TERMINAL_CODES]
- LLR LLR-005: modules=[evidence_core::diagnostic::FixHint]
- LLR LLR-006: modules=[evidence_core::git::GitSnapshot::capture]
- LLR LLR-007: modules=[evidence_core::env::capture::env_fingerprint]
- LLR LLR-008: modules=[evidence_core::hash::write_sha256sums]
- LLR LLR-009: modules=[evidence_core::env::manifest::RecipeManifest, evidence_core::env::fingerprint::EnvFingerprint::recipe_manifest]
- LLR LLR-010: modules=[evidence_core::bundle::EvidenceBuilder::finalize]
- LLR LLR-011: modules=[evidence_core::verify::bundle::verify_bundle_with_key, evidence_core::verify::completeness::check_bundle_completeness]
- LLR LLR-012: modules=[evidence_core::verify::cross_file::check_env_vs_index]
- LLR LLR-013: modules=[evidence_core::verify::consistency::check_dal_map]
- LLR LLR-014: modules=[evidence_core::verify::consistency::check_trace_outputs_hashed]
- LLR LLR-015: modules=[cargo_evidence::cli::verify::cmd_verify_jsonl]
- LLR LLR-016: modules=[cargo_evidence::cli::verify::terminal_ok, cargo_evidence::cli::verify::terminal_fail, cargo_evidence::cli::verify::terminal_error]
- LLR LLR-017: modules=[cargo_evidence::cli::args::OutputFormat::resolve]
- LLR LLR-018: modules=[cargo_evidence::cli::schema::cmd_schema_show, evidence_core::schema::Schema::source]
- LLR LLR-019: modules=[evidence_core::trace::validation::validate_trace_links_with_policy]
- LLR LLR-020: modules=[cargo_evidence::main::emit_unsupported_jsonl_terminal]
- LLR LLR-021: modules=[evidence_core::policy::TracePolicy, evidence_core::trace::validation::validate_trace_links_with_policy]
- LLR LLR-022: modules=[evidence_core::trace::selector_check::resolve_test_selectors, evidence_core::trace::validation::TraceValidationError::SelectorUnresolved]
- LLR LLR-023: modules=[cargo_evidence::cli::trace::default_trace_roots]
- LLR LLR-024: modules=[evidence_core::tests::ci_self_check]
- LLR LLR-025: modules=[cargo_evidence::cli::args::CheckMode, cargo_evidence::cli::check::cmd_check]
- LLR LLR-026: modules=[evidence_core::trace::requirement_report::build_requirement_report, evidence_core::trace::requirement_report::RequirementStatus]
- LLR LLR-027: modules=[evidence_core::trace::requirement_report::fix_hint_for_gap]
- LLR LLR-028: modules=[evidence_core::bundle::testing::parse_cargo_test_output, evidence_core::bundle::testing::TestOutcome]
- LLR LLR-029: modules=[evidence_core::rules::RULES, evidence_core::rules::RuleEntry, evidence_core::rules::Domain, evidence_core::rules::rules_json, cargo_evidence::cli::rules::cmd_rules]
- LLR LLR-030: modules=[diagnostic_codes_locked::rules_contains_every_code, diagnostic_codes_locked::every_rules_entry_is_implemented]
- LLR LLR-031: modules=[evidence_core::trace::entries::LlrEntry::emits, diagnostic_codes_locked::every_code_is_claimed_by_an_llr]
- LLR LLR-032: modules=[diagnostic_codes_locked::rules_terminal_set_matches_terminal_codes]
- LLR LLR-033: modules=[golden_fixtures::verify_golden_matches, golden_fixtures::check_source_golden_matches]
- LLR LLR-034: modules=[self_compliance_baseline::baseline_matches_current_generation]
- LLR LLR-035: modules=[evidence_core::floors::current_measurements, evidence_core::floors::count_rules, evidence_core::floors::count_terminals, evidence_core::floors::count_trace_per_layer, evidence_core::floors::count_tests, evidence_core::floors::count_library_panics, evidence_core::floors::FloorsConfig]
- LLR LLR-036: modules=[cargo_evidence::cli::floors::cmd_floors]
- LLR LLR-037: modules=[scripts/floors-lower-lint.sh]
- LLR LLR-038: modules=[evidence_core::trace::entries::HlrEntry::surfaces, evidence_core::trace::surfaces::KNOWN_SURFACES, evidence_core::trace::validation::validate_hlr_surfaces]
- LLR LLR-039: modules=[evidence_core::trace::entries::TestEntry::test_selectors, evidence_core::trace::entries::deserialize_string_or_vec]
- LLR LLR-040: modules=[evidence_core::trace::validation]
- LLR LLR-041: modules=[evidence_core::trace::validation::link_errors::LinkError, evidence_core::trace::validation::TraceValidationError]
- LLR LLR-042: modules=[cargo_evidence::cli::trace::cmd_trace]
- LLR LLR-043: modules=[evidence_core::rules::RULES]
- LLR LLR-044: modules=[evidence_core::rot_prone_markers_locked]
- LLR LLR-045: modules=[scripts::cross_time_determinism, scripts::deterministic_baseline_override_lint]
- LLR LLR-046: modules=[evidence_core::trace_id_refs_locked]
- LLR LLR-047: modules=[evidence_core::walker_usage_locked]
- LLR LLR-048: modules=[cargo_evidence::cli::doctor]
- LLR LLR-049: modules=[evidence_core::env::capture, evidence_core::verify::bundle, evidence_core::verify::errors, cargo_evidence::cli::verify, cargo_evidence::cli::generate::phases]
- LLR LLR-050: modules=[evidence_mcp::lib, evidence_mcp::subprocess]
- LLR LLR-051: modules=[evidence_core::bundle::test_summary, evidence_core::tests::outcome_record, evidence_core::bundle::builder]
- LLR LLR-052: modules=[evidence_core::trace::test_backlinks::resolve_llr_backlinks, evidence_core::bundle::outcome_record::TestOutcomeRecord, evidence_core::verify::llr_selectors::check_llr_test_selectors, cargo_evidence::cli::generate::test_outcomes::enrich_and_write_test_outcomes]
- LLR LLR-053: modules=[evidence_core::coverage::report::CoverageReport, evidence_core::coverage::llvm_cov_json::parse_llvm_cov_export, cargo_evidence::cli::generate::coverage_phase::run_coverage_phase]
- LLR LLR-054: modules=[evidence_mcp::schema::CheckRequest, evidence_mcp::schema::DoctorRequest, evidence_mcp::schema::RulesRequest, evidence_mcp::resolve_workspace, evidence_mcp::emit_workspace_fallback_diagnostic]
- LLR LLR-055: modules=[cargo_evidence::cli::trace::cmd_trace, cargo_evidence::cli::generate::phases::validate_trace_links_phase]
- LLR LLR-056: modules=[evidence_core::coverage::report::BranchCoverage]
- LLR LLR-057: modules=[cargo_evidence::cli::generate::coverage_phase::aggregate_lines_percent, evidence_core::bundle::builder_coverage::aggregate_lines]
- LLR LLR-058: modules=[cargo_evidence::cli::generate::coverage_phase::aggregate_branches_percent, evidence_core::bundle::builder_coverage::aggregate_branches]
- LLR LLR-059: modules=[cargo_evidence::cli::generate::coverage_phase::threshold_violations]
- LLR LLR-060: modules=[cargo_evidence::cli::doctor::checks::load_max_dal]
- LLR LLR-061: modules=[cargo_evidence::cli::generate::phases::trace_validation::TraceValidationResult, cargo_evidence::cli::generate::phases::trace_validation::validate_trace_links_phase, cargo_evidence::cli::generate::phases::write_compliance_reports]
- LLR LLR-062: modules=[evidence_mcp::server::Server, evidence_mcp::workspace::resolve_workspace]
- LLR LLR-063: modules=[evidence_mcp::version_probe::VersionSkew, evidence_mcp::version_probe::detect_with_probe, evidence_mcp::version_probe::probe_cli_version, evidence_mcp::version_probe::skew_diagnostic]
- LLR LLR-064: modules=[evidence_mcp::subprocess, evidence_mcp::server, evidence_mcp::schema]
- LLR LLR-065: modules=[evidence_mcp::subprocess]
- LLR LLR-066: modules=[evidence_mcp::server, evidence_mcp::schema]
- LLR LLR-067: modules=[evidence_mcp::server, evidence_mcp::schema]
- LLR LLR-068: modules=[evidence_mcp::server, evidence_mcp::schema]
- LLR LLR-069: modules=[evidence_core::verify::bundle, cargo_evidence::cli::verify]
- LLR LLR-070: modules=[evidence_core::boundary_check::check_no_build_rs]
- LLR LLR-071: modules=[evidence_core::boundary_check::check_no_proc_macros]
- LLR LLR-072: modules=[evidence_core::cargo_metadata::CargoMetadataProjection, evidence_core::bundle::builder::EvidenceBuilder::finalize, evidence_core::verify::bundle::verify_bundle_with_key]
- LLR LLR-073: modules=[evidence_core::boundary_check::check_dal_a_mcdc_evidence, evidence_core::policy::dal::AuxiliaryMcdcTool, cargo_evidence::cli::generate::policy::enforce_dal_qualification]
- LLR LLR-074: modules=[evidence_mcp::schema::JsonlToolResponse, evidence_mcp::schema::RulesToolResponse, evidence_mcp::schema::DiffToolResponse]
- LLR LLR-075: modules=[cargo_evidence::main]
- LLR LLR-076: modules=[evidence_core::tests::editor_duplicates_locked]
- LLR LLR-077: modules=[cargo_evidence::cli::keygen::cmd_keygen]
- LLR LLR-078: modules=[cargo_evidence::cli::generate::finalize::check_pubkey_anchor]
- LLR LLR-079: modules=[layered_claude_md_doctrine::every_workspace_crate_has_lean_layered_claude_md]
- LLR LLR-080: modules=[evidence_core::context::resolver, evidence_core::context::resolve_selector]
- LLR LLR-081: modules=[evidence_core::context::lookup, evidence_core::context::context_for, evidence_core::context::report]
- LLR LLR-082: modules=[cargo_evidence::cli::context, cargo_evidence::cli::context::cmd_context]
- LLR LLR-083: modules=[evidence_core::context::lookup, evidence_core::context::error, cargo_evidence::cli::context]
- LLR LLR-084: modules=[evidence_mcp::server, evidence_mcp::server::Server::evidence_context]
- LLR LLR-085: modules=[evidence_mcp::schema]
- LLR LLR-086: modules=[evidence_mcp::server::Server::evidence_context]
- LLR LLR-090: modules=[cargo_evidence::cli::init::agent_context::write_agent_context_files, cargo_evidence::cli::init::cmd_init]
- LLR LLR-091: modules=[evidence_core::bundle::input_scope::resolve_in_scope_units, evidence_core::bundle::input_scope::assemble_input_plan, evidence_core::bundle::input_scope::build_input_plan_blocking, cargo_evidence::cli::generate::phases::hash_in_scope_sources]
- LLR LLR-092: modules=[evidence_core::verify::source_baseline::check_source_baseline]
- LLR LLR-093: modules=[evidence_core::bundle::nextest::parse_nextest_libtest_json, cargo_evidence::cli::generate::phases::run_tests_and_capture]
- LLR LLR-094: modules=[evidence_core::verify::test_identity::check_test_identity]
- LLR LLR-095: modules=[evidence_core::bundle::cargo_artifacts::parse_workspace_artifacts, evidence_core::bundle::cargo_artifacts::inventory_outputs_blocking, cargo_evidence::cli::generate::phases::inventory_and_hash_outputs]
- LLR LLR-096: modules=[evidence_core::verify::output_manifest::check_output_manifest]
- LLR LLR-097: modules=[cargo_evidence::cli::generate::generator_closure, cargo_evidence::cli::generate::verify_error_blocks_generate]
- LLR LLR-098: modules=[evidence_core::corpus::graph]
- LLR LLR-099: modules=[evidence_core::corpus::legacy]
- LLR LLR-100: modules=[evidence_core::corpus::index]
- LLR LLR-101: modules=[evidence_core::trace::matrix::generate_corpus_traceability_matrix, evidence_core::trace::matrix::view, cargo_evidence::cli::generate::phases::copy_trace_and_build_matrix]
- LLR LLR-102: modules=[evidence_core::floors::count_trace_per_layer]
- LLR LLR-103: modules=[evidence_core::trace::assurance::AssuranceBijections, evidence_core::trace::validation::validate_trace_links_with_policy]
- LLR LLR-104: modules=[evidence_core::trace::requirement_report::build_corpus_requirement_report, evidence_core::trace::requirement_report::view]
- LLR LLR-105: modules=[evidence_core::trace::evidence_state]
- LLR LLR-106: modules=[evidence_core::trace::validation::validate_trace_links_with_policy, evidence_core::trace::entries::DerivedEntry, evidence_core::policy::evidence::TracePolicy]
- LLR LLR-107: modules=[cargo_evidence::cli::doctor::checks::check_trace, evidence_core::verify::trace_evidence]
- LLR LLR-108: modules=[evidence_core::compliance::coverage_verdict, evidence_core::compliance::status, evidence_core::compliance::report::CrateEvidence]
- LLR LLR-109: modules=[evidence_core::policy::assurance, evidence_core::policy::boundary::BoundaryConfig, evidence_core::policy::dal::DalConfig, cargo_evidence::cli::generate::policy]
- LLR LLR-110: modules=[evidence_core::policy::standards, evidence_core::compliance::generator, evidence_core::compliance::report::ComplianceReport]
- LLR LLR-111: modules=[evidence_core::corpus::review_content]
- LLR LLR-112: modules=[evidence_core::corpus::digest]
- LLR LLR-113: modules=[evidence_core::corpus::graph::RequirementNode, evidence_core::corpus::records, evidence_core::corpus::legacy]
- LLR LLR-114: modules=[evidence_core::corpus::review_records]
- LLR LLR-115: modules=[evidence_core::corpus::graph]
- LLR LLR-116: modules=[evidence_core::corpus::index]
- LLR LLR-117: modules=[evidence_core::corpus::lifecycle]
- LLR LLR-118: modules=[evidence_core::corpus::lifecycle, evidence_core::corpus::graph]
- LLR LLR-119: modules=[evidence_core::corpus::approval_boundary]
- LLR LLR-120: modules=[evidence_core::corpus::approval_boundary]
- LLR LLR-121: modules=[evidence_core::corpus::approval_boundary]
- LLR LLR-122: modules=[evidence_core::corpus::proposal]
- LLR LLR-123: modules=[evidence_core::corpus::proposal]
- LLR LLR-124: modules=[evidence_core::corpus::proposal]
- LLR LLR-125: modules=[evidence_core::corpus::source::records]
- LLR LLR-126: modules=[evidence_core::corpus::graph::nodes]
- LLR LLR-127: modules=[evidence_core::corpus::index]
- LLR LLR-128: modules=[evidence_core::corpus::source::records]
- LLR LLR-129: modules=[evidence_core::corpus::graph::validation]
- LLR LLR-130: modules=[evidence_core::corpus::source::lineage, evidence_core::corpus::graph]
- LLR LLR-131: modules=[evidence_core::corpus::source::lineage]
- LLR LLR-132: modules=[evidence_core::corpus::source::lineage]
- LLR LLR-133: modules=[evidence_core::corpus::source::lock]
- LLR LLR-134: modules=[evidence_core::corpus::source::lock]
- LLR LLR-135: modules=[evidence_core::corpus::source::lock, evidence_core::corpus::source::error]
- LLR LLR-136: modules=[evidence_core::corpus::source::verify]
- LLR LLR-137: modules=[evidence_core::corpus::source::verify]
- LLR LLR-138: modules=[evidence_core::corpus::source::verify]
- LLR LLR-139: modules=[evidence_core::policy::resolution::ResolutionPolicy, evidence_core::policy::resolution::ResolutionPolicyError, cargo_evidence::cli::generate::cmd_generate]
- LLR LLR-140: modules=[evidence_core::boundary_check::run_cargo_metadata, evidence_core::bundle::input_scope::build_input_plan_blocking, evidence_core::bundle::cargo_artifacts::inventory_outputs_blocking, evidence_core::bundle::builder::write_cargo_metadata_projection, cargo_evidence::cli::generate::phases::run_tests_and_capture, cargo_evidence::cli::generate::coverage_phase::run_coverage_phase]
- LLR LLR-141: modules=[evidence_core::cargo_metadata::CargoMetadataProjection, evidence_core::bundle::builder::EvidenceBuilder::finalize]
- LLR LLR-142: modules=[evidence_core::bundle::index::EvidenceIndex, evidence_core::verify::resolution_policy::check_resolution_policy, evidence_core::verify::bundle::verify_bundle_with_key]
- LLR LLR-143: modules=[evidence_core::bundle::index::EvidenceIndex]
- LLR LLR-144: modules=[evidence_core::bundle::builder::EvidenceBuilder::finalize]
- LLR LLR-145: modules=[evidence_core::verify::bundle::verify_bundle_with_key]
- LLR LLR-146: modules=[evidence_core::verify::reproduction::compare_reproduction]
- TEST TEST-001: selector=verify_jsonl::verify_ok_terminates_with_verify_ok_and_exit_zero
- TEST TEST-002: selector=verify_jsonl::verify_jsonl_stdout_is_strict_jsonl_only
- TEST TEST-003: selector=diagnostic_codes_locked::diagnostic_codes_locked
- TEST TEST-004: selector=verify_jsonl::verify_runtime_error_ends_with_verify_error_terminal
- TEST TEST-005: selector=evidence_core::diagnostic::tests::fix_hint_unknown_kind_falls_back_to_other
- TEST TEST-006: selector=git_and_hashes::test_git_snapshot_with_mock_provider
- TEST TEST-007: selector=git_and_hashes::test_cert_mode_strict_errors_missing_git
- TEST TEST-008: selector=cross_platform_determinism::sha256sums_contents_are_cross_platform_deterministic
- TEST TEST-009: selector=cross_platform_determinism::content_hash_is_cross_platform_deterministic
- TEST TEST-010: selector=bundle_lifecycle::test_toctou_detection
- TEST TEST-011: selector=bundle_content::test_tampering_detection
- TEST TEST-012: selector=verify_consistency::test_verify_detects_env_index_profile_mismatch
- TEST TEST-013: selector=verify_consistency::test_verify_rejects_git_source_with_nonhex_sha
- TEST TEST-014: selector=verify_consistency::test_verify_rejects_phantom_trace_output_not_in_sha256sums
- TEST TEST-015: selector=cli::test_init_template_does_not_trip_policy_gate
- TEST TEST-016: selector=verify_jsonl::verify_finding_emits_terminal_fail_and_exit_two
- TEST TEST-017: selector=cli::test_verify_json_nonexistent
- TEST TEST-018: selector=cli::test_schema_show_index
- TEST TEST-019: selector=trace_sys_layer::sys_hlr_llr_test_chain_validates
- TEST TEST-020: selector=verify_jsonl::unwired_diff_jsonl_is_rejected
- TEST TEST-021: selector=trace_sys_layer::require_hlr_sys_trace_rejects_empty_traces_to
- TEST TEST-022: selector=trace_sys_layer::selector_check_flags_dangling_selector
- TEST TEST-023: selector=trace_discovery::trace_defaults_to_tool_trace_when_flag_absent
- TEST TEST-024: selector=ci_self_check::ci_yaml_has_enforcement_flags
- TEST TEST-025: selector=check_source_tree::check_source_mode_on_clean_workspace
- TEST TEST-026: selector=check_source_tree::req_gap_on_blank_traces_to_has_fixhint
- TEST TEST-027: selector=check_source_tree::derived_gaps_carry_root_cause_uid
- TEST TEST-028: selector=check_bundle_mode::check_bundle_mode_matches_verify
- TEST TEST-029: selector=rules_cmd::rules_json_matches_rules_json_helper
- TEST TEST-030: selector=diagnostic_codes_locked::rules_contains_every_code
- TEST TEST-031: selector=diagnostic_codes_locked::every_code_is_claimed_by_an_llr
- TEST TEST-032: selector=diagnostic_codes_locked::rules_terminal_set_matches_terminal_codes
- TEST TEST-033: selector=golden_fixtures::golden_rules_json_byte_diff
- TEST TEST-034: selector=self_compliance_baseline::baseline_matches_current_generation
- TEST TEST-035: selector=evidence_core::floors::tests::current_measurements_satisfy_committed_floors
- TEST TEST-036: selector=floors_gate::floors_gate_fires_on_below_min_floor
- TEST TEST-037: selector=floors_lower_lint::refuses_decrease_without_justification_line
- TEST TEST-039: selector=trace_decomposition::test_selectors_deserializes_both_shapes
- TEST TEST-040: selector=derived_trace_validation::derived_missing_rationale_fires_at_dal_a
- TEST TEST-098: selector=evidence_core::bundle::input_scope::tests::resolves_package_name_to_manifest_dir
- TEST TEST-099: selector=evidence_core::bundle::input_scope::tests::missing_package_fails_closed
- TEST TEST-100: selector=evidence_core::bundle::input_scope::tests::manifest_outside_workspace_is_path_escape
- TEST TEST-101: selector=evidence_core::bundle::input_scope::tests::empty_unit_fails_closed
- TEST TEST-102: selector=evidence_core::verify::source_baseline::tests::empty_object_is_rejected
- TEST TEST-103: selector=evidence_core::verify::source_baseline::tests::non_empty_object_passes
- TEST TEST-104: selector=evidence_core::bundle::input_scope::tests::plan_agrees_with_independent_git_enumeration_in_a_temp_worktree
- TEST TEST-105: selector=evidence_core::bundle::input_scope::tests::required_input_presence_is_checked
- TEST TEST-106: selector=evidence_core::bundle::test_summary::tests::test_parse_cargo_test_output_failed
- TEST TEST-107: selector=cli_context::context_human_mode_workspace_overview_exits_zero
- TEST TEST-108: selector=context_roundtrip::evidence_context_file_selector_pulls_requirements
- TEST TEST-111: selector=evidence_core::verify::test_identity::tests::unknown_binary_record_is_rejected
- TEST TEST-113: selector=evidence_core::bundle::cargo_artifacts::tests::excludes_build_scripts_and_non_member_deps
- TEST TEST-118: selector=mcp_initialize_surface::initialize_protocol_negotiation_is_pinned
- TEST TEST-121: selector=evidence_core::corpus::tests::legacy_parity::legacy_parity_on_own_trace
- TEST TEST-122: selector=evidence_core::corpus::tests::graph_layout::layout_and_edge_order_produce_identical_graph
- TEST TEST-123: selector=trace_matrix::corpus_matrix_matches_legacy_and_is_input_order_independent

## End-to-End: HLR to Test Roll-Up

| HLR ID | HLR Title | Test IDs (via LLR) |
|--------|-----------|--------------------|
| HLR-001 | Every --format=jsonl run emits exactly one terminal event | TEST-001, TEST-072 |
| HLR-002 | stdout under --format=jsonl is strict JSONL | TEST-002 |
| HLR-003 | Diagnostic codes are locked: unique + regex + exhaustive | TEST-003 |
| HLR-004 | Terminal suffixes are reserved | TEST-004 |
| HLR-005 | FixHint is forward-compatible | TEST-005 |
| HLR-006 | Capture atomic git snapshot at start | TEST-006 |
| HLR-007 | Capture deterministic environment fingerprint | TEST-007 |
| HLR-008 | Write SHA256SUMS with deterministic ordering | TEST-008 |
| HLR-009 | Emit deterministic-manifest.json recipe projection | TEST-009, TEST-160 |
| HLR-010 | Finalize re-checks git SHA (TOCTOU guard) | TEST-010 |
| HLR-011 | Verify detects hash mismatch for every hashed file | TEST-011 |
| HLR-012 | Verify enforces env.json ↔ index.json consistency | TEST-012 |
| HLR-013 | Verify enforces DAL-map ↔ compliance consistency | TEST-013 |
| HLR-014 | Verify enforces trace_outputs are in SHA256SUMS | TEST-014 |
| HLR-015 | Strict-mode verify requires an ed25519 signature | TEST-015 |
| HLR-016 | Exit codes map to terminal events | TEST-016 |
| HLR-017 | --format resolution folds legacy --json | TEST-017 |
| HLR-018 | schema show diagnostic prints the embedded source | TEST-018 |
| HLR-019 | trace --validate enforces cross-tier links | TEST-019 |
| HLR-020 | Dispatch guards unwired --format=jsonl subcommands | TEST-020 |
| HLR-021 | Policy gate rejects HLR with empty traces_to | TEST-021 |
| HLR-022 | Test-selector resolution catches dangling pointers | TEST-022 |
| HLR-023 | Default --trace-roots discovery | TEST-023 |
| HLR-024 | CI self-check on enforcement flags | TEST-024 |
| HLR-025 | check auto-detects argument shape | TEST-025, TEST-028 |
| HLR-026 | check emits one diagnostic per requirement | TEST-027 |
| HLR-027 | Every REQ_GAP carries a mechanical FixHint where one exists | TEST-026 |
| HLR-028 | Test results come from captured workspace stdout | TEST-106 |
| HLR-029 | RULES is the single source of truth for the diagnostic vocabulary | TEST-029 |
| HLR-030 | RULES <-> source DiagnosticCode bijection is machine-enforced | TEST-030 |
| HLR-031 | Every code is claimed by an LLR via LLR.emits | TEST-031 |
| HLR-032 | TERMINAL_CODES <-> RULES.terminal=true is machine-enforced | TEST-032 |
| HLR-033 | Committed JSONL fixtures byte-lock the verify and check wire shapes | TEST-033 |
| HLR-034 | Tool's own compliance report stays green under its own generator | TEST-034 |
| HLR-035 | cert/floors.toml is the single source of truth for ratcheted measurements | TEST-035 |
| HLR-036 | CI enforces floors and ceilings on every push | TEST-036 |
| HLR-037 | Lowering a committed floor requires explicit written justification | TEST-037 |
| HLR-038 | HLR declares lateral surface of user-visible behaviors | TEST-038 |
| HLR-039 | TestEntry expresses N:M test-to-requirement mapping | TEST-039 |
| HLR-040 | Derived LLRs require written rationale | TEST-040 |
| HLR-041 | TraceValidationError::Link carries a typed sub-error enum | TEST-041 |
| HLR-042 | `trace --validate` emits one JSONL event per Link-phase sub-error | TEST-042 |
| HLR-043 | Every Link-phase sub-rule is listed in RULES with its own code | TEST-043 |
| HLR-044 | CI asserts no new rot-prone marker lands in .rs sources | TEST-044 |
| HLR-045 | Cross-time determinism is enforced by comparing every PR's recipe toolchain projection to the last successful main-branch build | TEST-045 |
| HLR-046 | CI gate asserts every narrative trace-ID reference resolves to a real trace entry | TEST-046 |
| HLR-047 | CI gate fails on any hand-rolled recursive fs::read_dir walker outside a reviewer-visible allowlist | TEST-047 |
| HLR-048 | cargo evidence doctor audits downstream rigor via a checklist of typed diagnostic codes | TEST-048 |
| HLR-049 | Pre-release builds embed a tool_prerelease flag in env.json and verify refuses such bundles under cert/record | TEST-049 |
| HLR-050 | Thin MCP server wraps check, rules, and doctor via subprocess over stdio | TEST-050, TEST-064, TEST-065, TEST-069, TEST-071 |
| HLR-051 | Per-test outcome records written to tests/test_outcomes.jsonl with failure-message capture from libtest stdout | TEST-051 |
| HLR-052 | Bundle records per-test requirement_uids; verify asserts every LLR is test-verified | TEST-052 |
| HLR-053 | generate --coverage flag invokes cargo-llvm-cov and writes a typed coverage report into the bundle | TEST-053, TEST-056 |
| HLR-054 | MCP tool handlers reject unknown fields + emit fallback signal when workspace_path is omitted | TEST-054 |
| HLR-055 | cmd_trace + cmd_generate pass derived.toml requirements into Link-phase validation | TEST-055 |
| HLR-056 | generate compares per-level coverage aggregate against the DAL engineering gates | TEST-057, TEST-058, TEST-059 |
| HLR-057 | cli::doctor derives its trace DAL via load_max_dal, not default_dal | TEST-060 |
| HLR-058 | generate threads trace_validation_passed bool from Phase 6 to write_compliance_reports | TEST-061 |
| HLR-059 | evidence-mcp ServerHandler returns {name: evidence-mcp, version: CARGO_PKG_VERSION} in get_info | TEST-062, TEST-118 |
| HLR-060 | evidence-mcp probes cargo evidence --version at startup and prepends MCP_VERSION_SKEW / MCP_VERSION_PROBE_FAILED | TEST-063 |
| HLR-061 | MCP exposes evidence_ping as a cheap liveness + version-skew probe that does not spawn a subprocess | TEST-066 |
| HLR-062 | MCP exposes evidence_floors so agents can query the ratchet-gate state | TEST-067, TEST-070 |
| HLR-063 | MCP exposes evidence_diff so agents can compare two bundles | TEST-068 |
| HLR-064 | Boundary policy can forbid build scripts in in-scope crates | TEST-075, TEST-076, TEST-078, TEST-079 |
| HLR-065 | Boundary policy can forbid proc-macros in in-scope crates | TEST-075, TEST-077, TEST-078, TEST-079 |
| HLR-066 | DAL-A in-scope crate without auxiliary MC/DC tool reference fails cert/record generate | TEST-080 |
| HLR-067 | MCP tool responses carry an explicit success boolean | TEST-081 |
| HLR-068 | Direct cargo-evidence --help invocation lists subcommands, not a redirect stub | TEST-082 |
| HLR-069 | Repository contains no editor-duplicate artifacts (` N.<ext>` filenames) | TEST-083 |
| HLR-070 | cargo evidence keygen: explicit create + rotate | TEST-084 |
| HLR-071 | Generate refuses on signing.pub anchor mismatch | TEST-085 |
| HLR-072 | Repo demonstrates lean-layered CLAUDE.md doctrine | TEST-086 |
| HLR-073 | cargo evidence context CLI verb returns per-module trace slice | TEST-087, TEST-088, TEST-089, TEST-090, TEST-107 |
| HLR-074 | evidence_context MCP tool returns per-module trace slice | TEST-091, TEST-092, TEST-093, TEST-094, TEST-108 |
| HLR-075 | cargo evidence init --with-agent-context scaffolds downstream CLAUDE.md | TEST-097 |
| HLR-076 | In-scope package names resolve to manifest directories; workspace-control inputs are captured; empty or unresolved scope fails closed | TEST-098, TEST-099, TEST-100, TEST-101, TEST-102, TEST-103, TEST-104, TEST-105 |
| HLR-077 | Generate captures tests via nextest libtest-json-plus preserving per-binary identity; verify fails closed on lost identity | TEST-109, TEST-110, TEST-111, TEST-115, TEST-116 |
| HLR-078 | Generate inventories workspace compiler artifacts and hashes each deliverable; verify fails closed on empty outputs when a build ran | TEST-112, TEST-113, TEST-114, TEST-117 |
| HLR-079 | corpus.toml is a strict, layout-agnostic index of linked graph files | TEST-119, TEST-122 |
| HLR-080 | Corpus graph enforces uid identity and resolvable typed edges | TEST-120, TEST-122 |
| HLR-081 | Legacy trace documents load as graph nodes at exact parity | TEST-121 |
| HLR-082 | Traceability matrix rows and relationships are derived from the canonical corpus graph | TEST-123 |
| HLR-083 | Trace floor dimensions are derived from the canonical corpus graph | TEST-124 |
| HLR-084 | Assurance bijections are derived from canonical corpus graph mappings | TEST-125 |
| HLR-085 | Requirement gap reports are canonical corpus graph queries | TEST-126 |
| HLR-086 | One shared trace-evidence evaluation classifies adoption states and every consumer fails closed | TEST-127, TEST-128 |
| HLR-087 | Derived requirements carry disposition and review completeness enforced under policy | TEST-128 |
| HLR-088 | Coverage verdicts map engineering metrics to honest A-7 statuses with separate disposition evidence | TEST-129 |
| HLR-089 | Cert and record evaluation fails closed without an explicit assurance selection; reports bind a versioned standards pack | TEST-130 |
| HLR-090 | Review approval binds a versioned canonical requirement-content projection with a stable typed digest | TEST-131, TEST-132 |
| HLR-091 | Native and legacy requirements retain review-sensitive content and expose the same projection | TEST-132 |
| HLR-092 | Review decision records load through a strict fail-closed schema bound to requirement uid and content digest | TEST-133, TEST-134 |
| HLR-093 | Review nodes carry typed review and supersession edges validated as deterministic chains | TEST-133, TEST-134 |
| HLR-094 | Lifecycle evaluation derives one deterministic state per requirement from effective digest-bound review heads | TEST-135 |
| HLR-095 | Lifecycle state is an evaluated-only view reported deterministically by requirement uid | TEST-136 |
| HLR-096 | Approval enforcement is an explicit caller-named policy with no default and no assurance-level inference | TEST-137 |
| HLR-097 | Explicit enforcement gates test verifies edges and implementation claims to approved requirements with distinct typed diagnostics | TEST-137, TEST-138 |
| HLR-098 | Proposal records carry exactly two representable actions in a strict schema-gated fail-closed TOML schema | TEST-139 |
| HLR-099 | Proposal append is store-minted, fail-closed, non-overwriting, and confined to a validated proposal root | TEST-139, TEST-140 |
| HLR-100 | Source revision records load through a strict fail-closed schema with typed material state and exact capture combinations | TEST-141 |
| HLR-101 | Source revision nodes are corpus graph identity loaded through the activated sources index kind before requirements and reviews | TEST-142, TEST-143 |
| HLR-102 | Source revisions own an optional supersedes link validated as one single acyclic chain per document key | TEST-144, TEST-145, TEST-146 |
| HLR-103 | Source baseline transitions are pure UID-preserving immutable-superset comparisons with distinct typed failures | TEST-148 |
| HLR-104 | Effective source heads are a deterministic derived view keyed by document key | TEST-147 |
| HLR-105 | The sources lock is a strict versioned canonical TOML inventory of effective source heads | TEST-149, TEST-150, TEST-152 |
| HLR-106 | Committed sources-lock validation applies three ordered exact gates with typed failures and never mutates the workspace | TEST-151 |
| HLR-107 | Each effective source revision verifies to a deterministic typed state with vendored byte verification beneath the fixed payload root | TEST-153, TEST-154 |
| HLR-108 | Batch source verification gates on global graph and lock prerequisites, then reports one sorted finding per effective head without mutation or network access | TEST-155 |
| HLR-109 | Generate applies one locked/offline resolution policy to every cargo subprocess; online resolution is a development-only opt-in that cert/record refuses | TEST-156, TEST-157 |
| HLR-110 | The bundle binds the resolved dependency graph and records the resolution policy; verification rejects an online-resolution cert/record bundle | TEST-158, TEST-159 |
| HLR-111 | The bundle binds a canonical recipe manifest and records its SHA-256 as index.json.recipe_hash | TEST-161, TEST-162 |
| HLR-112 | Reproduced-output comparison reports typed findings over input, recipe, and output digest planes | TEST-163 |

## Coverage Summary

- **HLR count:** 112
- **LLR count:** 143
- **Test count:** 159
- **HLR without LLR:** 0
- **LLR without Test:** 0
- **Orphan tests (no LLR link):** 0

