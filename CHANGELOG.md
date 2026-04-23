# Changelog

All notable changes to this project are documented here. The format
is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html);
pre-1.0, any minor bump may contain breaking changes.

All three workspace crates (`evidence-core`, `cargo-evidence`,
`evidence-mcp`) share a single version; release entries cover all
three unless noted.

## [0.1.2] — 2026-04-23

### Added

- `evidence-mcp` now probes `cargo evidence --version` at startup
  and prepends `MCP_VERSION_SKEW` (versions disagree) or
  `MCP_VERSION_PROBE_FAILED` (probe couldn't run) to every tool
  response when the CLI it spawned isn't the version this MCP
  server was built against. `RulesToolResponse` gains a
  `warnings: [...]` field carrying these MCP-layer signals
  separately from the `rules[]` manifest. Two new diagnostic
  codes registered in `evidence_core::RULES`.
- `cargo-evidence` and `evidence-mcp` binaries handle `--version`
  and `--help` as direct-invocation flags. Previously the cargo-
  subcommand dispatch form was the only path that accepted
  these, so `evidence-mcp --version` hung on the MCP handshake
  and `cargo-evidence --version` was rejected by clap.

### Docs

- `crates/evidence-mcp/README.md` gains a `claude mcp add
  evidence evidence-mcp` snippet for Claude Code (CLI) alongside
  the existing Claude Desktop JSON config.

### Internal

- Trace entries added for the skew-detection surface: SYS-028
  + HLR-060 + LLR-063 + TEST-063. Floors ratcheted
  correspondingly.

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

[0.1.2]: https://github.com/luofang34/Evidence/releases/tag/v0.1.2
[0.1.1]: https://github.com/luofang34/Evidence/releases/tag/v0.1.1
[0.1.0]: https://github.com/luofang34/Evidence/releases/tag/v0.1.0
