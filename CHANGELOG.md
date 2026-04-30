# Changelog

All notable changes to this project are documented here. The format
is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html);
pre-1.0, any minor bump may contain breaking changes.

All three workspace crates (`evidence-core`, `cargo-evidence`,
`evidence-mcp`) share a single version; release entries cover all
three unless noted.

## [0.1.3] — 2026-04-30

### Added

- **`forbid_build_rs` and `forbid_proc_macros` boundary policies
  now have full enforcement.** Both flags were declared in
  `BoundaryPolicy` since 0.1.0 but were rejected by the generate
  preflight as "unimplemented" — a safety rail to keep bundles
  from silently overclaiming. As of 0.1.3 the rules are enforced
  at three points:
  - **Generate-time detection** in
    `evidence_core::boundary_check::check_no_build_rs` /
    `check_no_proc_macros`. Both shell out to
    `cargo metadata --format-version 1` (no new crate dep) and
    flag any in-scope crate whose `targets[].kind` contains
    `"custom-build"` or `"proc-macro"`. Per-crate scoping is
    preserved — out-of-scope crates with build.rs / proc-macro
    are fine. Codes: `BOUNDARY_FORBIDDEN_BUILD_RS`,
    `BOUNDARY_FORBIDDEN_PROC_MACRO`.
  - **`links =` informational surfacing.** When a
    `BOUNDARY_FORBIDDEN_BUILD_RS` violation fires, the
    diagnostic message includes the package's `links` value
    (when set) — e.g. `crate 'foo' has build.rs (links =
    "libz")`. Auditors see the native-FFI binding without
    re-running `cargo metadata` by hand. Strictly informational;
    no separate violation code.
  - **Verify-time recheck** against a new bundle artifact
    `cargo_metadata.json`. The artifact is a deterministic
    projection (sorted `packages[]` of
    `{ name, targets[].kind, links }`) written into the bundle
    when either flag is enabled at generate time. Verify
    replays the rules against the cached projection — closes
    the window where a build.rs / proc-macro added between
    generate and verify would otherwise pass silently. New
    codes: `BOUNDARY_VERIFY_METADATA_MISSING` (artifact required
    but absent), `VERIFY_BOUNDARY_BUILD_RS_DETECTED`,
    `VERIFY_BOUNDARY_PROC_MACRO_DETECTED`.
- **`EvidenceIndex.boundary_policy`** field. Additive,
  `#[serde(default)]` — old bundles deserialize with all-`false`
  defaults so the verify-time recheck no-ops on legacy bundles.
  Captures the policy flags at generate time so verify can
  replay the contract without consulting the verifier's local
  `boundary.toml`. The new field is omitted from `index.json`
  serialization when the policy is the all-`false` default
  (`skip_serializing_if`), so bundles with no policy claim look
  byte-identical to pre-0.1.3 bundles.
- **`evidence_core::CargoMetadataProjection`** public type +
  `from_raw_metadata` / `from_projection_json` /
  `to_canonical_json` constructors. New module
  `evidence_core::cargo_metadata`. The projection sorts packages
  by name on construction and serializes deterministically — two
  hosts with the same git state produce byte-identical
  artifacts (SYS-003).
- **`VERIFY_RUNTIME_READ_VERIFY_KEY` diagnostic code.** Closes
  an HLR-001 violation: `cargo evidence verify --verify-key
  <path> --format=jsonl` previously short-circuited via
  `Err(anyhow)` when the key file was missing or unreadable,
  dropping the run on the floor without emitting a JSONL
  terminal. The new code fires a structured finding + the
  `VERIFY_ERROR` terminal across all three output formats
  (`--format={human,json,jsonl}`).
- **CI security audit.** New `.github/workflows/audit.yml`
  runs `cargo audit --deny warnings` on PR + push-to-main
  (path-scoped to dep-graph changes), Monday 06:00 UTC cron,
  and `workflow_dispatch`. `--deny warnings` flips
  unmaintained / unsound / yanked advisories from soft to hard
  failures — DAL-A/B projects can't ship cert evidence while a
  known unsound dep sits in the graph.

### Changed

- **MCP `exit_code` contract documented as a non-machine
  signal.** `TOOL_FAILURE_EXIT_CODE = 2` deliberately collides
  with `cargo evidence`'s own `EXIT_VERIFICATION_FAILURE` —
  agents can't distinguish CLI verification failure from MCP
  tool-layer failure by `exit_code` alone. The contract now
  spells out (in the `evidence-mcp` README's
  "Failure-shape contract" section, in the
  `TOOL_FAILURE_EXIT_CODE` doc comment, and in LLR-064) that
  hosts must dispatch on `terminal` (JSONL verbs) or
  `error.code` (rules / diff verbs) — never on `exit_code`.
  Behaviour unchanged; documentation tightened to prevent host
  implementers from defaulting to the simpler-looking
  `exit_code` dispatch.
- **`evaluate_thresholds` (renamed from `threshold_violations`)
  + `aggregate_lines_percent` / `aggregate_branches_percent`
  moved to `evidence_core::coverage`** for test discoverability.
  Previously buried in `cargo_evidence::cli::generate::coverage_phase`,
  these are now reachable via `cargo test --workspace
  threshold_integration_*` without needing `--all-targets` or
  `--bins` (Cargo's binary-unit-test convention was filtering
  the regression tests out).
- **`BoundaryPolicy`** gains `Default` + `PartialEq` derives.
  Public API; downstream code matching on the struct is
  unaffected.

### Fixed

- **`cargo evidence verify --verify-key` JSONL terminal
  contract.** See
  `VERIFY_RUNTIME_READ_VERIFY_KEY` under Added. Previously
  violated HLR-001 on key-read I/O failure; now emits exactly
  one terminal as Schema Rule 1 requires.

### Internal

- New `evidence_core::cargo_metadata` module + sibling tests.
- New `evidence_core::coverage::thresholds` module owning the
  threshold-evaluation logic.
- `bundle/builder.rs` split into `builder.rs` +
  `builder/config.rs` + `builder/cargo_metadata_artifact.rs`
  (workspace 500-line limit).
- `verify/errors.rs` Display impl extracted to
  `verify/errors_display.rs`.
- `verify/bundle.rs` gains `check_boundary_recheck`
  function (LLR-072).
- Schema versions stay at `"0.0.1"` per pre-1.0 convention —
  every additive bundle/wire-format change rewrites the
  documentation in place.
- Trace entries: HLR-064, HLR-065 (boundary determinism vs
  auditability split); LLR-070, LLR-071 (per-policy detection);
  LLR-072 (verify-time recheck artifact); TEST-072 through
  TEST-079.
- Floors ratcheted: `diagnostic_codes` 143 → 150,
  `trace_hlr` 63 → 65, `trace_llr` 68 → 72,
  `trace_test` 71 → 77, `per_crate.evidence-core.test_count`
  314 → 351, `per_crate.cargo-evidence.test_count` 149 → 133
  (net relocation: coverage threshold tests moved to
  evidence-core; workspace test mass conserved).
- `cert/boundary.toml` documents why this workspace can't
  enable `forbid_build_rs = true`: `evidence-core/build.rs`
  is load-bearing for engine-provenance via
  `EVIDENCE_ENGINE_GIT_SHA` (SYS-017). Other projects without
  equivalent constraints should set both flags to `true` at
  DAL-A/B.

## [0.1.2] — 2026-04-24

### Added

- **Three new MCP verbs.** `evidence-mcp` now exposes six tools
  total (up from three):
  - `evidence_ping` — cheap liveness + version-skew probe. No
    subprocess spawn per call; reads the cached `VersionSkew`
    captured at server startup. Use as a reachability check
    before invoking expensive verbs.
  - `evidence_floors` — query the ratchet-gate state. Wraps
    `cargo evidence floors --format=jsonl`; streams
    `FLOORS_DIMENSION_OK` / `FLOORS_BELOW_MIN` per dimension
    and terminates with `FLOORS_OK` / `FLOORS_FAIL`.
  - `evidence_diff` — compare two on-disk bundles. Wraps
    `cargo evidence diff <a> <b> --json`; returns the raw
    structured delta blob (inputs, outputs, metadata, env) as
    a single JSON document.
- **Structured MCP-layer failures.** Subprocess failures in any
  MCP tool (cargo not on `PATH`, spawn error, timeout, malformed
  JSONL output) now surface as a well-formed `JsonlToolResponse`
  or `RulesToolResponse` carrying `exit_code == 2` plus a single
  structured diagnostic with an `MCP_*` code. Previously these
  went back as a free-form rmcp `Err(String)` that agents
  couldn't pattern-match on. Five new codes in
  `evidence_core::RULES` under `Domain::Mcp`:
  `MCP_CARGO_NOT_FOUND`, `MCP_MALFORMED_JSONL`, `MCP_NO_OUTPUT`,
  `MCP_SUBPROCESS_SPAWN_FAILED`, `MCP_SUBPROCESS_TIMEOUT`.
- **`HAND_EMITTED_MCP_CODES`** public constant in
  `evidence_core`, parallel to `HAND_EMITTED_CLI_CODES` and
  disjoint from it. Each set audits against its own source tree;
  `MCP_VERSION_PROBE_FAILED` / `MCP_VERSION_SKEW` /
  `MCP_WORKSPACE_FALLBACK` migrate out of the CLI list into the
  new MCP list.
- **`RulesToolResponse.error: Option<Value>`** field. `None` on
  success; on tool-layer failure carries the structured `MCP_*`
  diagnostic.
- **`EVIDENCE_MCP_TIMEOUT_SECS`** environment variable tunes the
  per-spawn subprocess cap. Default 600 s, clamped to
  `[60, 7200]`. Read on every call, so an operator can retune
  without restarting the server; out-of-range values clamp with
  a `tracing::warn!`, unparseable values fall back to the
  default.
- **Version-skew probe.** On startup `evidence-mcp` probes
  `cargo evidence --version` and caches the result. Tool
  responses prepend `MCP_VERSION_SKEW` (versions disagree) or
  `MCP_VERSION_PROBE_FAILED` (probe couldn't run) when the
  CLI's version doesn't match the MCP server's. The cached
  outcome is surfaced directly by `evidence_ping`.
  `RulesToolResponse` gains a `warnings: [...]` field carrying
  these MCP-layer signals separately from the `rules[]`
  manifest.
- **`cargo-evidence` and `evidence-mcp` binaries handle
  `--version` / `--help`** as direct-invocation flags.
  Previously `evidence-mcp --version` hung on the MCP
  handshake and `cargo-evidence --version` was rejected by
  clap because the cargo-subcommand dispatch form was the only
  path that accepted these.

### Changed

- **Synthesized parse terminals renamed for prefix alignment.**
  `MALFORMED_JSONL` → `MCP_MALFORMED_JSONL`; `NO_OUTPUT` →
  `MCP_NO_OUTPUT`. Both are MCP-layer signals (not CLI-emitted),
  so the `MCP_` prefix now reflects domain ownership.
- **`EnvFilter::from_env_lossy`** in the MCP binary's
  tracing init (replaces `try_from_default_env().unwrap_or_else
  (…)`). Preserves valid directives when `RUST_LOG` has a syntax
  error elsewhere in the string, and honors empty `RUST_LOG=""`
  (the Nix-sandbox-clears-RUST_LOG case) without dropping to the
  silent fallback.
- **`evidence_check` tool description** explicitly flags
  side-effect scope: `--mode=source` executes the workspace's
  tests (writes files, binds sockets, mutates env, spawns
  processes); `--mode=bundle` is inspection-only.
- **`VersionSkew::Matched(String)`** (internal enum) carries the
  probed CLI version string. The byte-equality invariant is now
  expressed at the type — consumers read `cli_version` from the
  variant rather than substituting the MCP version (which would
  silently misreport if the match check ever relaxed).

### Fixed

- **`cargo-evidence` binary** now supported `--version` on direct
  invocation (not only via `cargo evidence --version` dispatch).

### Docs

- `crates/evidence-mcp/README.md` gains a `claude mcp add
  evidence evidence-mcp` snippet for Claude Code (CLI) alongside
  the existing Claude Desktop JSON config, a `Configuration`
  table documenting `EVIDENCE_MCP_TIMEOUT_SECS` and `RUST_LOG`,
  and an expanded tools table covering all six verbs with
  inspection-vs-execution annotations.

### Internal

- Trace entries added for the full MCP-layer expansion:
  SYS-028, HLR-060 through HLR-063, LLR-063 through LLR-068,
  TEST-063 through TEST-071. Floors ratcheted correspondingly
  (`diagnostic_codes`: 136 → 143, `trace_hlr`: 59 → 63,
  `trace_llr`: 62 → 68, `trace_test`: 62 → 71,
  `per_crate.evidence-mcp.test_count`: 11 → 37).
- `crates/evidence-mcp/src/server.rs` streaming-verb handlers
  (`evidence_doctor`, `evidence_check`, `evidence_floors`) share
  a single `Server::run_streaming_verb` helper owning the
  `resolve_workspace` → `run_evidence` → `parse_jsonl` → prepend-
  fallback → prepend-skew pipeline. New verbs of the same shape
  route through the helper by construction, so skipping the
  skew-signal prepend is no longer possible.
- `rules.rs` split: `HAND_EMITTED_CLI_CODES` /
  `HAND_EMITTED_MCP_CODES` / `RESERVED_UNCLAIMED_CODES` move
  into `rules/hand_emitted.rs` to keep the facade under the
  workspace 500-line file limit.
- `server.rs` split: response-builder helpers move into
  `server/responses.rs` (pure functions over `VersionSkew` /
  `RunError` / `WorkspaceResolution`, unit-tested in isolation).
- Meta-bijection tests for `HAND_EMITTED_MCP_CODES`
  (registry ⇔ `crates/evidence-mcp/src`) in both directions.
- `RunError::code(&self) -> &'static str` pins each subprocess-
  wrapper variant to its `MCP_*` code at the type site; the
  response-building helper reads the code off the enum rather
  than matching at each call-site.
- Golden fixture `crates/cargo-evidence/tests/fixtures/golden_rules.json`
  regenerated for the 143-code manifest.

## [0.1.1] — 2026-04-23

### Added

- `evidence-mcp` published to crates.io for the first time. The
  crate's tree was present in the 0.1.0 workspace but not uploaded.
- `evidence-mcp` returns its own identity (`name: "evidence-mcp"`,
  `version: env!("CARGO_PKG_VERSION")`) in the MCP `initialize`
  handshake. Previously the default `rmcp` framework identity was
  advertised, which made the server indistinguishable from any other
  rmcp-built MCP server.
- `cargo evidence trace --validate` human output now uses the
  `[✓]` / `[⚠]` / `[✗]` glyph convention that `check`, `doctor`, and
  `floors` use. Each `LinkError` variant prints a one-line typed
  entry; the terminal line is `TRACE_OK` or `TRACE_FAIL`.

### Fixed

- Branch-coverage threshold check now reads branch counts, not line
  counts. Pre-fix, a project at 95% lines / 50% branches passed the
  DAL-B `branch ≥ 85%` gate spuriously. `FileMeasurement` gains an
  `Option<BranchCoverage>` field sibling to `lines`; the parser
  populates it from the `summary.branches.{covered,count}` pair on
  Branch-level measurements. Aggregation splits into
  `aggregate_lines_percent` / `aggregate_branches_percent` so the
  per-level dispatch is unmissable at every call-site.
- `cargo evidence doctor` now derives its trace-policy DAL as the
  maximum across per-crate overrides, not just `default_dal`. A
  project with `default_dal = "D"` and one crate overridden to
  DAL-A previously ran DAL-D rules in doctor; the auditor saw green
  at the lowest configured rigor while the DAL-A crate's stricter
  checks were silently skipped. `load_default_dal` renamed to
  `load_max_dal` to match.
- `compliance/<crate>.json` A3-6 / A4-6 statuses now reflect the
  actual outcome of `validate_trace_links_phase`, not a hardcoded
  `true`. Non-strict (dev) profile warn-and-continue produces
  `Partial` instead of `Met`; strict profile (cert / record)
  short-circuits before compliance reports are written at all.

### Changed

- `compliance/status.rs` `determine_a7_status` (137 lines) split
  into per-objective helpers (`a7_1_2_hlr_testing`,
  `a7_3_4_llr_testing`, `a7_5_target_compatibility`,
  `a7_6_hlr_test_coverage`, `a7_7_llr_test_coverage`,
  `a7_8_statement_coverage`, `a7_9_decision_coverage`,
  `a7_10_mcdc_coverage`). No behaviour change. Each helper has its
  own normal / robustness / BVA unit tests.
- `evidence-mcp/src/lib.rs` split into `lib.rs` (facade, 26 lines),
  `server.rs`, `workspace.rs` to satisfy the workspace ≤100-line
  `lib.rs` rule.

### Internal

- Trace entries added for the above: SYS-024..027, HLR-056..059,
  LLR-056..062, TEST-056..062. Floors ratcheted correspondingly.
- `coverage/coverage_summary.json` wire format gains
  `per_file[].branches: { covered, total }` on Branch-level
  measurements. Additive change; old bundles deserialize with the
  field absent (reads as `None`).

## [0.1.0] — 2026-02 .. 2026-04

Initial public release on crates.io for `evidence-core` and
`cargo-evidence`. Release-arc milestones are summarized in the
project README (section `Release cadence`) and in the git log —
a per-PR enumeration was not maintained for the 0.1.0 arc. Future
releases will use this file.

[0.1.3]: https://github.com/luofang34/Evidence/releases/tag/v0.1.3
[0.1.2]: https://github.com/luofang34/Evidence/releases/tag/v0.1.2
[0.1.1]: https://github.com/luofang34/Evidence/releases/tag/v0.1.1
[0.1.0]: https://github.com/luofang34/Evidence/releases/tag/v0.1.0
