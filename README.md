# cargo-evidence

**Certification evidence generation, verification, and traceability for Rust projects.**

[![Crates.io](https://img.shields.io/crates/v/cargo-evidence.svg)](https://crates.io/crates/cargo-evidence)
[![CI](https://github.com/user/evidence/actions/workflows/ci.yml/badge.svg)](https://github.com/user/evidence/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/cargo-evidence.svg)](LICENSE-MIT)

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
requirement in the discovered trace (`tool/trace/` or `cert/trace/`).
`REQ_GAP` events carry a `FixHint` for mechanically-fixable cases
(missing UUID, empty `traces_to` under policy, dangling
`test_selector`), and derived GAPs at higher layers carry
`root_cause_uid` pointing at the primary failure.

In bundle mode, `check` is a passthrough to `verify` — same wire
shape, same exit codes.

**`verify` remains supported as the low-level primitive** for CI
scripts and bash pipelines that want a stable bundle-only surface
without argument-shape inference. MCP (planned) wraps `check`, not
`verify`: one agent verb, one MCP tool.

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
2. `index.json` parses correctly and `bundle_complete` is `true`
3. All trace outputs referenced in the index exist
4. Every entry in `SHA256SUMS` matches the actual file hash
5. `index.json` is NOT listed in `SHA256SUMS` (metadata-layer invariant)
6. `content_hash` matches the SHA-256 of `SHA256SUMS`

Exit codes: `0` = pass, `1` = error, `2` = verification failure.

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
measurement helper in `evidence::floors` and the initial floor in
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
diagnostic_codes = 10    # evidence::RULES.len() if you use it

[delta_ceilings]
# Reserved for delta-based gates (new dead-code allows, new library
# panics). Parsed today, enforced via a follow-up.
```

Only the dimensions you list are enforced — missing ones are
skipped, not assumed-zero. Point at a custom path via
`cargo evidence floors --config path/to/floors.toml` if your layout
differs. Measurement helpers that need workspace subdirs (`tool/trace/`,
`crates/*/src/`) gracefully degrade to 0 when the dirs are absent,
so a single-crate project without a `tool/trace/` directory can
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
`terminal`. This is the self-describe endpoint MCP (PR #50) wraps;
every code here is (a) backed by a `DiagnosticCode::code()` impl or
by the `TERMINAL_CODES` / `HAND_EMITTED_CLI_CODES` sets, and (b)
claimed by at least one LLR's `emits` list in
`tool/trace/llr.toml`. Four bijection invariants in
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
| Software Verification Results (SVR)        | Minimal | Exit codes only; no structured pass/fail     |
| Traceability Data                          | Yes     | Bidirectional HLR <-> LLR <-> Test           |
| SCM Records                                | No      | Not yet implemented                          |

### Tool Qualification Level

- **Classification:** Development tool + Verification tool
- **TQL assessment:** TQL-4 minimum, TQL-3 depending on DAL
- **Self-verification concern:** The tool generates AND verifies its own bundles.
  For independent verification, a separate tool or manual SHA-256 check of
  `SHA256SUMS` is recommended.

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

1. **No structured SVR capture** -- exit codes and `TestSummary` are recorded but
   per-test-case structured verification results are not.
2. **No derived requirements safety report** -- derived LLRs are validated but no
   summary report is generated for safety analysis.

Previously tracked items now resolved:

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

The `evidence` crate can be used as a library for custom integration:

```rust
use evidence::{
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
use evidence::trace::{read_all_trace_files, validate_trace_links, generate_traceability_matrix};

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
2. Ensure `cargo test --workspace` passes.
3. Ensure `cargo clippy --workspace -- -D warnings` is clean.
4. Run `cargo evidence generate --out-dir /tmp/evidence` to verify the tool works end-to-end.
5. Submit a pull request with a clear description of the change.

### Project Structure

```
crates/
  evidence/          # Core library (evidence crate)
  cargo-evidence/    # Cargo subcommand binary
schemas/             # JSON schemas for bundle files
cert/                # Certification configuration (boundary, profiles, trace)
docs/                # Compliance documentation
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
