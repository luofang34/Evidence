# cargo-evidence

**Certification evidence generation, verification, and traceability for Rust projects.**

[![CI](https://github.com/luofang34/Evidence/actions/workflows/ci.yml/badge.svg)](https://github.com/luofang34/Evidence/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

`cargo-evidence` generates self-describing, offline-verifiable evidence bundles
for safety-critical Rust builds. It captures environment fingerprints,
source/artifact hashes, command logs, and bidirectional traceability matrices.

---

## Project Status

> **Work in progress. Not production-ready. Not yet qualified for use on a
> certification program.**

This repository is an early-stage tool and its on-disk formats, CLI surface,
schema versions, and tool-qualification story are all still moving. Treat
anything below as an implementation sketch, not a contract:

- **No public release yet.** There is no crates.io publication and no stable
  version number. Breaking changes land on `main` without deprecation cycles.
- **Determinism is a design goal, not a proven property.** The tool is built
  around reproducible bundles (see ADR-001 invariants), but end-to-end
  cross-platform bit-for-bit determinism has not been measured and is not
  asserted by the test suite.
- **No DER review.** No auditor, DER, or certification authority has evaluated
  the tool. The "For Auditors" section below documents the intended design,
  not a qualification claim.
- **Schema versions (`0.0.x`) signal pre-1.0 instability.** Treat them as such.

If you are considering this tool on a real program, the honest answer today is:
fork it, read every line, and plan to own the delta.

---

## Platform Support

The CI matrix covers three host platforms; the level of confidence in each
is intentionally different.

| Platform     | Compiles | Unit + integration tests | Nix reproducible build | Cross-host `deterministic_hash` parity | `deterministic_hash` parity under Nix |
|--------------|----------|--------------------------|------------------------|-----------------------------------------|----------------------------------------|
| Linux x86_64 | yes      | yes                      | yes (gated in CI)      | yes (gated in CI)                       | yes (gated in CI)                      |
| macOS (Apple Silicon) | yes | yes                  | works via devShell (gated in CI) | yes (gated in CI)             | yes (gated in CI)                      |
| Windows x86_64 | yes    | yes                      | n/a (no Nix on Windows) | yes (gated in CI)                       | n/a (no Nix on Windows)                |

What's being claimed:

- Every bundle carries **two** SHA-256 hashes in `index.json`:
  `content_hash` (the full SHA-256 of `SHA256SUMS`, covers every recorded
  byte, necessarily host-specific because `env.json` records host identity)
  and `deterministic_hash` (the SHA-256 of a committed
  `deterministic-manifest.json`, which is a projection of `env.json` down
  to toolchain + target + source identity).
- **`deterministic_hash` parity is gated in CI across five flavors**
  (3 native + 2 Nix) via the `determinism-compare` job: every push runs
  the generator on Linux, macOS, and Windows *native* plus Linux and
  macOS *under `nix develop`*, and asserts all five `deterministic_hash`
  values are byte-equal. If any flavor diverges, or if any flavor's
  artifact is missing, the job fails the PR. The Nix flavors dogfood
  the cert-build toolchain resolution path (rust-overlay) against the
  dev path (rustup), so silent drift between dev and cert bundles is
  caught mechanically. See
  [`determinism-compare` in `.github/workflows/ci.yml`](.github/workflows/ci.yml).
- `content_hash` **still differs** per host by design, and that's
  intentional: the full SHA256SUMS-hashed chain records the host
  operating environment so a DO-330 auditor has cryptographically-bound
  provenance for the build environment. The cross-host equality channel
  is `deterministic_hash`.

What this matrix does **not** claim:

- Passing CI tests and matching `deterministic_hash` prove the tool runs
  consistently on that platform. Neither proves a bundle is fit for
  regulatory qualification on that platform — that's a DER's call, not a
  CI signal.
- macOS and Windows are kept green primarily so contributors on those
  hosts can develop the tool. Bundles generated there are still
  **development artifacts** pending qualification review.

**Recommended posture for anyone evaluating the tool:**

- Final cert builds should run on Linux under the provided Nix flake.
- For dev work on macOS / Windows, compare `deterministic_hash` against
  the Linux reference when you need to confirm reproducibility; don't
  compare `content_hash` across hosts (it will always differ).
- Pin a commit SHA, because the formats may still change.

---

## Quick Start

Get your first evidence bundle in under 30 seconds:

```bash
# Install
cargo install cargo-evidence

# Initialize evidence tracking in your project
cargo evidence init

# Generate an evidence bundle — IMPORTANT: --out-dir must point
# OUTSIDE the tracked git tree (see "Choosing --out-dir" below).
cargo evidence generate --out-dir /tmp/evidence

# Verify bundle integrity
cargo evidence verify /tmp/evidence/dev-20260207-*/
```

That is it. The `init` command scaffolds a `cert/` directory with boundary
configuration, profile templates, and example trace files. The `generate`
command produces a complete evidence bundle. The `verify` command checks every
SHA-256 hash in the bundle and confirms structural integrity.

### Choosing `--out-dir`

`--out-dir` should live **outside** your project's tracked git tree (e.g.
`/tmp/evidence`, `$RUNNER_TEMP/evidence` in CI, or a sibling directory to
your repo). Why: `env.json` records `git_dirty`, derived from
`git status --porcelain`. If your bundle directory sits inside the repo
root, the first `generate` leaves an untracked directory there, and every
subsequent `generate` on the same commit observes the tree as dirty —
flipping `git_dirty` from `false` to `true`, rotating `env.json`'s hash in
`SHA256SUMS`, and rotating `content_hash`. Same-commit runs are then no
longer byte-reproducible.

If you need to place the bundle inside the repo (e.g. some CI workflows
find it convenient), add the output directory to `.gitignore` *before*
running `generate` for the first time. `git status --porcelain` respects
`.gitignore`, so ignored directories do not flip `git_dirty`. This repo's
own `.gitignore` already excludes `/evidence/` as a safety net for the
Quick Start example path.

---

## Three-Tier Environment Strategy

`cargo-evidence` uses a three-tier profile system that auto-detects the
appropriate strictness level based on your environment:

| Profile    | When auto-detected                  | Git state    | Missing tools | Timestamps in hash | Use case                         |
|------------|-------------------------------------|--------------|---------------|--------------------|----------------------------------|
| **dev**    | Default (local workstation)         | Dirty OK     | Warnings      | Excluded           | Local development, iteration     |
| **cert**   | Nix shell + CI (`IN_NIX_SHELL` + `CI`) | Must be clean | Hard errors   | Excluded           | Certification builds, DER review |
| **record** | `NAV_RECORD` env var set            | Must be clean | Hard errors   | Excluded           | Hardware-in-the-loop recording   |

**Auto-detection logic:**

1. If `NAV_RECORD` is set --> `record`
2. If `IN_NIX_SHELL` is set AND `CI`/`GITHUB_ACTIONS` is set --> `cert`
3. Otherwise --> `dev`

You can always override with `--profile dev|cert|record`.

**Key differences:**
- `dev` lets you iterate fast -- dirty git trees and missing tools are tolerated.
- `cert` enforces clean git, requires all toolchain components on PATH, and
  produces bundles suitable for certification data item submission.
- `record` is identical to `cert` in strictness but is triggered by a dedicated
  environment variable for hardware test recording workflows.

---

## What's in a Bundle?

Every bundle is a self-contained directory with deterministic content:

```
evidence/cert-20260207-143022Z-a1b2c3d4/
  index.json                   # Bundle metadata, schema versions, content_hash + deterministic_hash
  env.json                     # Environment fingerprint (rustc, cargo, LLVM, libc, OS, tools, …)
  deterministic-manifest.json  # Cross-host-stable projection of env.json (toolchain + target + source)
  inputs_hashes.json           # SHA-256 hashes of all source inputs
  outputs_hashes.json          # SHA-256 hashes of all build outputs
  commands.json                # Recorded command executions with exit codes
  SHA256SUMS                   # Content-layer integrity manifest
  trace/                       # Traceability matrix outputs (Markdown)
    matrix.md                  # HLR <-> LLR <-> Test bidirectional matrix
  tests/                       # Test execution artifacts (stdout/stderr)
```

**Design invariants:**

- `SHA256SUMS` covers every content-layer file, including
  `env.json` **and** `deterministic-manifest.json`. `index.json` is
  metadata-layer and excluded from `SHA256SUMS`, so timestamps do not
  affect either hash.
- `content_hash` in `index.json` is `SHA-256(SHA256SUMS)` — the
  full-fidelity integrity hash. It differs per host because
  `env.json` records host identity (host.os, libc, tool availability).
- `deterministic_hash` in `index.json` is
  `SHA-256(deterministic-manifest.json)` — the cross-host
  reproducibility contract. Two runs on the same commit with the
  same `rust-toolchain.toml` and the same `--target` produce the
  same `deterministic_hash` regardless of host. This is what CI's
  cross-host determinism job gates on.
- The manifest is a **projection** of `env.json`, not a rewrite.
  `verify_bundle` re-projects `env.json` at verification time and
  asserts byte-equality against the committed manifest; tampering
  with either side is caught.
- All paths in `SHA256SUMS` use forward slashes, regardless of OS.
- Bundle directories are prefixed with the profile name to prevent
  accidental submission of `dev` bundles as `cert`.
- Existing bundle directories are never overwritten.

### content_hash vs deterministic_hash — when to compare which

Use `content_hash` when you need to attest that **every recorded byte**
is unchanged. It is the integrity hash in the classical sense and is
what `sha256sum -c SHA256SUMS` inside the bundle will attest to
(subject to the `index.json` exclusion). Comparing `content_hash`
across hosts is meaningless because the bundles legitimately record
different host identities.

Use `deterministic_hash` when you need to confirm that two bundles
**represent the same logical build** (same commit, same toolchain,
same target). It is the cross-host reproducibility channel. Our CI
cross-host job asserts `deterministic_hash` parity across Linux,
macOS, and Windows on every push.

### Cross-time determinism and the `Override-Deterministic-Baseline:` protocol

Cross-host determinism (above) pins reproducibility across N hosts
at a single commit. The complementary CI job `cross-time-determinism`
pins the other axis: every PR's **toolchain projection** (rustc,
cargo, llvm_version, cargo_lock_hash, rust_toolchain_toml, rustflags)
must match the last successful main-branch build — OR the PR must
explicitly acknowledge that it intentionally changed a
reproducibility-affecting input.

Mechanism: the job downloads the last main-branch `xhost-Linux`
artifact via `gh run download`, extracts `deterministic-manifest.json`,
and hands both manifests to
`scripts/deterministic-baseline-override-lint.sh`. The lint
projects only the six toolchain fields (git_sha / git_branch /
git_dirty / schema_version / profile are excluded — they differ
between commits by construction, not because of drift) and
compares canonicalized JSON.

When the projections match, the job passes silently. When they
differ, the job requires a line of the form

    Override-Deterministic-Baseline: <one-sentence reason>

Accepted locations (checked in order):

1. The PR body (any line in the PR description).
2. Any commit message in the PR's push range — the gate reads
   `github.event.commits[*].message`, so the override line in any
   real-work commit on the branch satisfies the check. This
   matters for **merge-commit workflows** (like this repo's)
   where the merge commit itself typically won't carry the
   override; for **squash-merge workflows** the single head
   commit is the only message and carries it alongside the PR
   body.

Without that line in either place, the job fails with the full
projection diff + the expected override syntax.

Examples of legitimate override reasons:

    Override-Deterministic-Baseline: bumped serde_json to 1.0.130 for CVE-NNNN-NNNN
    Override-Deterministic-Baseline: added -C opt-level=3 to RUSTFLAGS
    Override-Deterministic-Baseline: upgraded rust-toolchain pin to 1.96

If no prior main-branch artifact is available (fresh repo, or 14-day
artifact retention expired), the job logs a warning and passes —
the gate is best-effort, never user-hostile.

**Known limitation**: the live-compare gate detects per-PR drift but
not slow cumulative drift across many individually-justified
overrides. A committed historical-anchor baseline — pinning
`{git_sha → deterministic_hash}` for a curated set of milestone
commits — would close that gap and is tracked as a follow-up.

### Captured Output Normalization

Every file written by `cargo evidence generate` under the capture directory
(`tests/`) has its line endings normalized to LF (`\n`) before being written
to disk and hashed. This applies uniformly to `cargo test` stdout and stderr
on every host.

**Why:** a Windows host running `cargo test` emits output with CRLF
(`\r\n`) line endings; a Linux host emits LF. Without normalization, the
same logical test run on two different hosts would produce different bytes
on disk, different `SHA256SUMS` entries, and therefore different
`content_hash` values — a cross-platform determinism leak that would
defeat the evidence chain the tool is built around.

**What's normalized:** strict `\r\n` pairs collapse to a single `\n`.
Lone `\r` bytes (e.g. cargo's `Compiling foo\r` progress spinners) are
**preserved**, so legitimate carriage-return use is not corrupted. Lone
`\n` bytes pass through unchanged.

**What Windows users should expect:** opening
`tests/cargo_test_stdout.txt` in Notepad may render as one long line.
Use VS Code, Notepad++, or any editor that handles Unix line endings.
This is a deliberate, tool-wide invariant — there is no flag to opt
out, and bundles from all three supported hosts are byte-comparable as
a result.

**What's not normalized:** this rule applies only to captured
subprocess text output. JSON files (`index.json`, `env.json`, `*_hashes.json`,
`commands.json`) and `SHA256SUMS` are written by the tool itself and are
LF-only by construction. Binary outputs recorded into `outputs_hashes`
are hashed as-is — normalization would corrupt them and would not apply.

---

## Commands Reference

### `cargo evidence check` — the agent-facing verb

Agents and humans should call `check` as the default. It auto-detects
whether the argument is a source tree or a bundle and dispatches:

```bash
cargo evidence check                 # source mode on current dir
cargo evidence check .               # same, explicit
cargo evidence check path/to/bundle  # bundle mode (auto-detected via SHA256SUMS)
cargo evidence check --mode=source . # force source mode
cargo evidence check --mode=bundle path/to/bundle  # force bundle mode
cargo evidence --format=jsonl check .              # streaming per-requirement diags
```

In source mode, `check` runs `cargo test --workspace`, parses outcomes,
and emits one `REQ_PASS` / `REQ_GAP` / `REQ_SKIP` diagnostic per
requirement in the discovered trace (`cert/trace/` or `cert/trace/`).
`REQ_GAP` events carry a `FixHint` for mechanically-fixable cases
(missing UUID, empty `traces_to` under policy, dangling
`test_selector`), and derived GAPs at higher layers carry
`root_cause_uid` pointing at the primary failure.

In bundle mode, `check` is a passthrough to `verify` — same wire
shape, same exit codes.

**`verify` remains supported as the low-level primitive** for CI
scripts and bash pipelines that want a stable bundle-only surface
without argument-shape inference. The `evidence-mcp` wrapper exposes
`check` as one of six MCP tools (alongside `rules`, `doctor`,
`floors`, `diff`, and `ping`); it does not expose `verify` because
`check` in bundle mode already delegates to it.

### `cargo evidence generate`

Generate a new evidence bundle.

```bash
cargo evidence generate --out-dir /tmp/evidence
cargo evidence generate --out-dir /tmp/evidence --profile cert
cargo evidence generate --out-dir /tmp/evidence --boundary cert/boundary.toml
cargo evidence generate --out-dir /tmp/evidence --trace-roots cert/trace
cargo evidence generate --out-dir /tmp/evidence --json --quiet
```

| Flag                | Description                                           |
|---------------------|-------------------------------------------------------|
| `--out-dir <DIR>`   | Output directory — must be outside tracked tree (required unless `--write-workspace`) |
| `--profile <PROF>`  | Override auto-detected profile (`dev`/`cert`/`record`) |
| `--boundary <FILE>` | Path to `boundary.toml` (default: `cert/boundary.toml`)|
| `--trace-roots <D>` | Comma-separated trace root directories                 |
| `--write-workspace` | Write to `evidence/` in workspace (xtask integration)  |
| `--quiet`, `-q`     | Suppress non-error output                              |
| `--json`            | Output results as JSON                                 |

### `cargo evidence verify`

Verify an existing evidence bundle.

```bash
cargo evidence verify /tmp/evidence/cert-20260207-143022Z-a1b2c3d4/
cargo evidence verify /tmp/evidence/cert-20260207-*/  --strict
cargo evidence verify /tmp/evidence/cert-20260207-*/  --json
```

Checks performed:
1. Bundle directory exists and contains all required files
2. `index.json` parses correctly; bundle-completeness cross-check:
   - `bundle_complete` is `true` iff `tool_command_failures == []` (tamper signal: `VERIFY_BUNDLE_INCOMPLETELY_CLAIMED`);
   - on `cert` / `record` profile, `tool_command_failures` must be empty (`VERIFY_TOOL_COMMANDS_FAILED_SILENTLY`);
   - on `dev` profile, `bundle_complete: false` is allowed and surfaces as `VERIFY_BUNDLE_INCOMPLETE` (Warning, non-blocking) so snapshots of half-broken local builds remain inspectable.
3. All trace outputs referenced in the index exist
4. Every entry in `SHA256SUMS` matches the actual file hash
5. `index.json` is NOT listed in `SHA256SUMS` (metadata-layer invariant)
6. `content_hash` matches the SHA-256 of `SHA256SUMS`

Exit codes: `0` = pass, `1` = error, `2` = verification failure.

When `cargo test` (or any captured subprocess) exits non-zero during `generate`, the builder records a `ToolCommandFailure { command_name, exit_code, stderr_tail }` entry on the bundle's `index.json`, and `bundle_complete` flips to `false`. On `cert` / `record` profile this also propagates as a non-zero exit from `generate` itself (`EXIT_VERIFICATION_FAILURE`, 2), so automation sees the signal without parsing the bundle.

### `cargo evidence diff`

Compare two evidence bundles.

```bash
cargo evidence diff ./evidence/bundle-a ./evidence/bundle-b
cargo evidence diff ./evidence/bundle-a ./evidence/bundle-b --json
```

Shows added, removed, and changed files in both input and output hash sets,
plus metadata changes (profile, git SHA, branch, dirty state).

### `cargo evidence init`

Initialize evidence tracking for a new project.

```bash
cargo evidence init
cargo evidence init --force   # overwrite existing cert/ directory
```

Creates:
- `cert/boundary.toml` -- certification boundary configuration
- `cert/profiles/dev.toml`, `cert.toml`, `record.toml` -- profile configs
- `cert/trace/hlr.toml`, `llr.toml` -- example trace files

### `cargo evidence schema show`

Print a JSON schema to stdout.

```bash
cargo evidence schema show index
cargo evidence schema show env
cargo evidence schema show commands
cargo evidence schema show hashes
```

### `cargo evidence schema validate`

Validate a JSON file against its schema.

```bash
cargo evidence schema validate ./evidence/bundle/index.json
cargo evidence schema validate ./evidence/bundle/env.json
```

Auto-detects the schema type from the filename or file content.

### `cargo evidence floors` — the ratchet

Enforce "rigor only goes up" across every dimension the tool
ratchets. Reads `cert/floors.toml`, measures the current state, and
reports per-dimension pass/fail:

```bash
cargo evidence floors         # human table, exit 0 if all ✓
cargo evidence floors --json  # machine-readable, deterministic
```

Dimensions currently tracked: diagnostic code count, terminal code
count, per-layer trace entry counts (SYS/HLR/LLR/Test), `#[test]`
fn count, library panics. Adding a dimension is a PR that lands the
measurement helper in `evidence_core::floors` and the initial floor in
`cert/floors.toml`; CI keeps the floor from falling.

**Lowering a floor** requires a `Lower-Floor: <dimension> <reason>`
line in the PR body (or direct-push commit message). Without it,
`scripts/floors-lower-lint.sh` fires in CI with
`FLOORS_LOWERED_WITHOUT_JUSTIFICATION`. The friction is intentional:
the ratchet only moves up.

**Squash-merge caveat.** GitHub's default "squash and merge"
button DROPS the original PR body unless the committer hand-copies
it into the squash commit message. If your PR lowers a floor, paste
the `Lower-Floor:` line into the squash commit's extended
description before merging — otherwise a post-merge dogfood run of
the lint on the `main` branch would fail against the squashed
commit's body. Projects that use merge-commits or rebase-and-merge
preserve the PR body and are unaffected.

**Using `floors` in your own project (no manual setup required for
the default case).** If your project has no `cert/floors.toml`, the
subcommand emits a friendly "not configured" info line on stderr and
exits 0 — non-adopters aren't forced into the gate. To opt in, drop
a minimal `cert/floors.toml` in your repo:

```toml
# cert/floors.toml — pin whichever dimensions matter for your project
[floors]
test_count = 42          # `#[test]` fn count across crates/
diagnostic_codes = 10    # evidence_core::RULES.len() if you use it

[delta_ceilings]
# Reserved for delta-based gates (new dead-code allows, new library
# panics). Parsed today, enforced via a follow-up.
```

Only the dimensions you list are enforced — missing ones are
skipped, not assumed-zero. Point at a custom path via
`cargo evidence floors --config path/to/floors.toml` if your layout
differs. Measurement helpers that need workspace subdirs (`cert/trace/`,
`crates/*/src/`) gracefully degrade to 0 when the dirs are absent,
so a single-crate project without a `cert/trace/` directory can
still enforce `test_count` and `diagnostic_codes` without
configuring the other dimensions.

### `cargo evidence rules` — what can the tool say?

Dump every diagnostic code the tool can emit as a deterministic
JSON array (for agents / MCP) or a human-readable table:

```bash
cargo evidence rules --json  # machine-readable, stable shape
cargo evidence rules         # human table
```

Each entry carries `code`, `severity`, `domain`, `has_fix_hint`, and
`terminal`. This is the self-describe endpoint `evidence-mcp`
consumes; every code here is (a) backed by a
`DiagnosticCode::code()` impl or by the `TERMINAL_CODES` /
`HAND_EMITTED_CLI_CODES` sets, and (b)
claimed by at least one LLR's `emits` list in
`cert/trace/llr.toml`. Four bijection invariants in
`diagnostic_codes_locked` fail CI if those relationships ever drift
— adding a code without updating `RULES` or writing an owning LLR
is not possible silently.

The `--json` wire shape is byte-locked against a committed fixture
at `crates/cargo-evidence/tests/fixtures/golden_rules.json`.
Intentional regeneration: `tools/regen-golden-fixtures.sh`.

---

## For Auditors

This section is for certification auditors and Designated Engineering
Representatives (DERs) evaluating `cargo-evidence` as a tool qualification
candidate.

### Certification Data Item Coverage

| Data Item                                  | Status  | Notes                                      |
|--------------------------------------------|---------|---------------------------------------------|
| Software Configuration Index (SCI)         | Partial | Git SHA + env fingerprint captured           |
| Software Environment Config Index (SECI)   | Partial | rustc, cargo, LLVM, libc, OS; missing Cargo.lock hash, RUSTFLAGS |
| Software Verification Results (SVR)        | Partial | Per-test `{name, module_path, passed, ignored, failure_message?}` in `tests/test_outcomes.jsonl`; duration missing (libtest stable limitation) |
| Traceability Data                          | Yes     | Bidirectional HLR <-> LLR <-> Test           |
| SCM Records                                | No      | Not yet implemented                          |

### Tool Qualification Level

- **Classification:** Development tool + Verification tool.
- **Qualification status:** Not qualified. The tool has not undergone DO-330
  qualification; no Tool Qualification Plan, Tool Qualification Data, or
  independent assessor review exists. The TQL ceiling discussed in older
  internal notes (TQL-3 / TQL-5) was an aspirational target, not a current
  claim — it is retired here to avoid downstream confusion. The minimum
  viable qualification package (PSAC + SVVP + SVR + SCM Plan + SQA Plan +
  Qualification Report, signed by an independent DER) does not exist; a
  template-shaped placeholder for projects to fill in is in the 1.0 backlog
  (`cert/DO-330-TEMPLATE/`).
- **No independent-verification path today.** The tool generates AND verifies
  its own bundles — they share the same binary. `sha256sum -c SHA256SUMS` from
  a separate utility re-checks *integrity* of the recorded hashes, but it
  cannot answer the auditor's actual question: *did the tool-under-qualification
  record the correct hashes in the first place?* That answer requires either
  (a) re-running this tool with the same inputs and comparing the resulting
  `content_hash` byte-for-byte (which is what the cross-host CI gate does
  internally — see `.github/workflows/ci.yml`), or (b) a separately-developed
  verifier with its own qualification story. Option (b) does not exist yet
  and is in the 1.0 backlog.

### MC/DC coverage at DAL-A — currently unavailable

DO-178C DAL-A requires Modified Condition/Decision Coverage at the source-
code level. Stable Rust does not currently expose MC/DC instrumentation:
the `-Zcoverage-options=mcdc` nightly flag was removed upstream by
[rust-lang/rust#144999](https://github.com/rust-lang/rust/pull/144999)
(merged 2025-08-08). The tracking issue
[rust-lang/rust#124144](https://github.com/rust-lang/rust/issues/124144)
remains open with no active reimplementation.

**Practical consequences for projects using this tool today:**

- A DAL-A project running `cargo evidence generate --profile cert` will
  produce a bundle whose `compliance/<crate>.json` reports DO-178C objective
  A7-10 (MC/DC coverage) as `NotMet`. The bundle's terminal can still be
  `VERIFY_OK` because branch coverage was met. **A careful auditor reads the
  A7-10 line and rejects the submission; a careless one signs off.** This
  asymmetry is a known sharp edge — see the 0.2 backlog item
  *"DAL-A fail-loud on missing MC/DC."*
- Projects pursuing actual DAL-A certification today need an auxiliary
  qualified MC/DC tool (LDRA, VectorCAST, Rapita) and must record its
  output by reference in their own qualification submission. This tool's
  bundle does not yet have a schema hook for that reference; that's also
  in the 0.2 backlog.

The `CoverageLevel::Mcdc` enum variant + the `decisions: Vec<DecisionCoverage>`
and `conditions: Vec<ConditionCoverage>` per-file vectors in the bundle's
coverage schema are forward-looking placeholders so the wire format can absorb
rustc's future re-implementation without a breaking schema bump. They emit
empty arrays today.

### ADR-001: Evidence Engine Invariants

The six invariants that govern this tool's design:

| # | Invariant        | Status | How enforced                                              |
|---|------------------|--------|-----------------------------------------------------------|
| 1 | Non-mutating     | PASS   | `--out-dir` required; never modifies source tree           |
| 2 | Self-describing  | PASS   | `index.json` + `SHA256SUMS` in every bundle                |
| 3 | Deterministic    | PASS   | `content_hash` excludes timestamps; BTreeMap ordering; two back-to-back runs on the same commit produce identical `content_hash` (gated by the dogfood `Evidence (self)` CI job) |
| 4 | Data-driven      | PASS   | TOML policy files in `cert/`                               |
| 5 | Offline-capable  | PASS   | Zero network calls; all operations are local              |
| 6 | Cross-platform   | PASS   | `deterministic_hash` parity across Linux/macOS/Windows is gated by the `evidence-cross-host` CI job; `content_hash` differs by host by design (it binds `env.json`'s host identity fields) |

### Known Limitations (P1 Items)

These items are tracked and not yet resolved:

1. **No derived requirements safety report** -- derived LLRs are validated but no
   summary report is generated for safety analysis.

Previously tracked items now resolved:

- ~~No structured SVR capture~~ → per-test outcome atoms in
  `tests/test_outcomes.jsonl` via the enriched libtest parser
  (captures panic/assertion text from `---- <test> stdout ----`
  failure blocks). A-7 Obj-3/Obj-4 upgrade from Partial → Met
  when present + aggregate `tests_passed == true`.
- ~~No cryptographic signing~~ → HMAC-SHA256 via `sign_bundle()` + `BUNDLE.sig`
- ~~No extra-file detection~~ → `verify.rs` walks bundle and flags unexpected files
- ~~Incomplete SCI/SECI~~ → `Cargo.lock` hash, `RUSTFLAGS`, `rust-toolchain.toml` captured
- ~~`engine_git_sha` conflation / `"unknown"` fallback~~ → `build.rs` captures the engine's own
  SHA via `EVIDENCE_ENGINE_GIT_SHA` env override (CI publish path uses `${GITHUB_SHA}`) with
  `git rev-parse HEAD` as the second choice; a `release-v<version>` string is embedded when
  neither is available (crates.io tarball builds). `engine_build_source` in `index.json`
  records which branch fired, and `verify` rejects cert/record bundles whose provenance is
  `"release"` or `"unknown"`.



---

## Library Usage

The `evidence-core` crate can be used as a library for custom integration:

```rust
use evidence_core::{
    EvidenceBuilder, EvidenceBuildConfig, verify_bundle, VerifyResult,
    EnvFingerprint, Profile,
};
use std::path::PathBuf;

// Build configuration
let config = EvidenceBuildConfig {
    output_root: PathBuf::from("./evidence"),
    profile: "dev".to_string(),
    in_scope_crates: vec!["my-crate".to_string()],
    trace_roots: vec!["cert/trace".to_string()],
    require_clean_git: false,
    fail_on_dirty: false,
};

// Create a builder and generate a bundle
let builder = EvidenceBuilder::new(config)?;
builder.write_inputs()?;
builder.write_outputs()?;
builder.write_commands()?;
let bundle_path = builder.finalize("0.0.1", "0.0.3", vec![])?;

// Verify a bundle
match verify_bundle(&bundle_path)? {
    VerifyResult::Pass => println!("Bundle verified."),
    VerifyResult::Fail(reason) => eprintln!("Verification failed: {}", reason),
    VerifyResult::Skipped(reason) => println!("Skipped: {}", reason),
}

// Capture environment fingerprint
let env = EnvFingerprint::capture("dev", false)?;
println!("rustc: {}", env.rustc);
println!("LLVM: {:?}", env.llvm_version);
```

### Traceability

```rust
use evidence_core::trace::{read_all_trace_files, validate_trace_links, generate_traceability_matrix};

// Read trace files from cert/trace/
let (hlr, llr, tests) = read_all_trace_files("cert/trace")?;

// Validate all links (UID format, ownership, derived rationale, orphans)
validate_trace_links(&hlr.requirements, &llr.requirements, &tests.tests)?;

// Generate a Markdown traceability matrix
let matrix = generate_traceability_matrix(&hlr, &llr, &tests, "TM-001")?;
std::fs::write("trace/matrix.md", matrix)?;
```

---

## Reproducible Builds with Nix

A `flake.nix` is provided for fully reproducible build environments:

```bash
nix develop          # Enter the development shell
cargo evidence generate --out-dir ./evidence --profile cert
```

The Nix flake pins the exact Rust toolchain, ensuring that `cert` profile
bundles are reproducible across machines.

---

## Contributing

1. Fork the repository and create a feature branch.
2. **First-time setup**: run `./tools/install-hooks.sh` once. This points the repo at `.githooks/` so the `pre-push` hook runs `scripts/local-ci.sh` automatically.
3. Before pushing, run `./scripts/local-ci.sh`. It mirrors every cargo gate run by `.github/workflows/ci.yml` — including the full `RUSTDOCFLAGS` doc gate (`-D rustdoc::broken_intra_doc_links -D warnings`). Subset commands (`cargo test` or `cargo clippy` alone) are not a substitute; the `local_ci_mirrors_workflow` integration test pins the script's coverage against CI.
4. Run `cargo evidence generate --out-dir /tmp/evidence` to verify the tool works end-to-end.
5. Submit a pull request with a clear description of the change.

### Project Structure

```
crates/
  evidence-core/     # Core library (types, trace, verify, compliance)
  cargo-evidence/    # Cargo subcommand binary
  evidence-mcp/      # MCP (Model Context Protocol) server binary
schemas/             # JSON schemas for bundle files
cert/                # Certification configuration (boundary, profiles, floors)
cert/trace/          # SYS / HLR / LLR / Test chain (this project's own trace)
tools/               # Repo utilities (install-hooks.sh, regen-golden-fixtures.sh)
scripts/             # CI mirror (local-ci.sh)
```

---

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
