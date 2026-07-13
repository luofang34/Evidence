# Bundle Assurance Contract — fail-closed generate/verify — design spec

**Date:** 2026-07-13
**Status:** Draft — pending user review
**Issues:** #138 (inputs), #139 (outputs / recipe_hash), #140 (test identity), #141 (trace fail-closed)
**Goal:** Replace the exit-code proxy for bundle completeness with a single
fail-closed **Bundle Assurance Contract**, evaluated by both `generate` and
`verify` from one code path, so that a bundle can never report
`bundle_complete: true` / `GENERATE_OK` while its input, output, test, or
trace evidence is empty, mis-identified, or unresolvable.

This spec follows the brainstorming → writing-plans → implementation flow.
The writing-plans skill consumes this file to produce the per-PR
implementation plan.

Two design decisions are already fixed (user-confirmed 2026-07-13):

- **#140 test capture:** adopt `cargo-nextest` machine-readable output as the
  test-execution source of truth. `nextest` becomes a required tool for
  `generate`.
- **#139 hash rename:** rename `deterministic_hash` → `recipe_hash` and add a
  typed `reproducibility` comparison ({input_digest, recipe_digest,
  output_digest}).

---

## 1. Background

`cargo-evidence` produces certification bundles. Each bundle is a
self-contained directory (`index.json`, `SHA256SUMS`, `env.json`,
`deterministic-manifest.json`, `inputs_hashes.json`, `outputs_hashes.json`,
`commands.json`, `tests/`, trace outputs, compliance reports) whose integrity
is meant to be cryptographically checkable. The tool dogfoods itself: the
`evidence-self-*` CI jobs generate a bundle *of this repository* and verify it.

Four issues, audited on `main` at `f8b0ebe`, report the same shape of failure
from four different subsystems: the generator emits a success terminal for a
bundle that is provably incomplete, and the verifier accepts absence of
evidence as valid evidence.

### The current success decision

`bundle_complete` is computed at exactly one place:

```
crates/evidence-core/src/bundle/builder.rs:459
    bundle_complete: self.tool_command_failures.is_empty(),
```

`generate.rs:347` then keys `GENERATE_OK` vs `GENERATE_FAIL` off the count of
captured commands that exited non-zero — and nothing else. No code inspects
the manifests the generator just wrote. The four issues are four consequences
of that single proxy.

### The four facets (mapped code)

| Issue | Failure | Root cause (file:line) |
|---|---|---|
| **#138** inputs | `inputs_hashes.json = {}` on success | `phases.rs:191` passes bare package names (`evidence-core`) to `git_ls_files`; real paths are `crates/evidence-core` → 0 matches → `Ok(vec![])`. No zero-input check (`verify/paths.rs:26` requires only that the file *exists*). |
| **#139** outputs | `outputs_hashes.json = {}` on success; hash overclaims | `EvidenceBuilder::hash_output()` (`builder.rs:204`) has **zero callers**. `deterministic_hash` (`builder.rs:431`) = SHA-256 of an 11-field env projection (`env/manifest.rs:30-58`); excludes input/output/command/target-triple/feature digests → byte-identical across materially different bundles. |
| **#140** tests | fresh bundle fails immediate verify with 87 `VERIFY_LLR_TEST_SELECTOR_UNRESOLVED` | generate parses `cargo test` **stdout only** (`phases.rs:237-244`); the `Running .../deps/<bin>` headers Cargo needs for binary identity are on **stderr** → every test keyed `__unknown_binary__` (`test_summary.rs:169-176`) → forward join (`trace/test_backlinks.rs`) matches no LLR → all `requirement_uids` empty. Summary counts 601 (`total = passed+failed+ignored+filtered_out`) vs 596 JSONL rows, unreconciled. generate never runs verify before `GENERATE_OK`. |
| **#141** trace | missing/empty trace → `VERIFY_OK` | `read_all_trace_files` returns empty defaults + `warn!` for a missing/empty root (`trace/read.rs:129`); `validate_trace_links_with_policy` treats an empty graph as vacuously `Ok` (`trace/validation.rs:431`); DAL-D disables every gate but `require_uids` (`policy/evidence.rs:80-93`); doctor's only empty guard is `dal >= C` (`doctor/checks.rs:74`). No diagnostic code exists for "not configured / not adopted / empty". |

### Two facts that make the fix converge

- **`check` already captures tests correctly.** `check.rs:361-372` sets
  `NO_COLOR`/`CARGO_TERM_COLOR=never` and concatenates stdout+stderr before
  parsing. generate diverged from the known-good path.
- **`diff` already has the reproducibility-comparison surface.** `diff.rs:77-80`
  compares both bundles' `inputs_hashes` and `outputs_hashes` — it is fed empty
  maps today. Populate the manifests and the comparison becomes real.

---

## 2. Goals & non-goals

### Goals

- **G1.** One `evidence-core` **Bundle Assurance Contract** — a typed model of
  which evidence claims a bundle asserts, derived from profile + DAL +
  boundary — evaluated by *both* generate and verify from the same code.
- **G2.** `generate` runs the release verifier against the bundle it just wrote
  and refuses `GENERATE_OK` if that verifier would reject it. (#140 AC:
  *"GENERATE_OK is never emitted for a bundle that the same release verifier
  will reject."*)
- **G3.** Every claim fails **closed**: a claim may never be `Satisfied` with
  zero, mis-identified, or unresolvable evidence — on any profile.
- **G4.** Canonical identity: package names resolve to manifest directories;
  Cargo artifacts inventory outputs; tests carry package/binary/harness
  identity; trace has a typed adoption state.
- **G5.** Each of the four issues lands as an independently revertible PR that
  ships its fix **and** its regression guardrail.
- **G6.** The README and `index.json` schema stop overclaiming: each claim's
  documentation is corrected in the PR that earns it.

### Non-goals

- **N1.** Byte-for-byte cross-host *output* reproducibility. Native macOS/Windows
  artifacts will differ from Linux; the contract distinguishes recipe identity
  from output equality rather than asserting the latter across hosts.
- **N2.** Reproducing the full resolved dependency *graph* in the bundle beyond
  the `Cargo.lock` hash already captured (a future enrichment, not required by
  these four issues).
- **N3.** A new agent-facing verb. All behavior routes through the existing
  `generate` / `verify` / `check` / `doctor` / `trace` surfaces.

---

## 3. Architecture — the Bundle Assurance Contract

### 3.1 Library layer (`evidence-core`) — new `assurance` module

New `crates/evidence-core/src/assurance.rs` + `assurance/` per the
`foo.rs` + `foo/` convention (no `mod.rs`):

```rust
/// The evidence a bundle asserts. Which claims are *required* is derived
/// from Profile + Dal + BoundaryConfig.
pub enum EvidenceClaim {
    SourceBaseline,          // #138
    OutputReproducibility,   // #139
    TestExecution,           // #140
    Traceability,            // #141
}

/// The single semantic result shared across CLI / MCP / doctor / generate /
/// verify (#141 AC: "use the same semantic result").
pub enum ClaimVerdict {
    Satisfied,
    NotClaimed,               // profile/DAL does not assert this claim
    NotConfigured,            // evidence source absent, never adopted
    NotAdopted,               // present-but-empty; explicit adoption state
    Empty,                    // claimed but zero evidence
    Invalid(Vec<Diagnostic>), // present but broken
}

pub struct AssuranceContract {
    required: Vec<EvidenceClaim>,   // from profile + dal + boundary
}

pub struct ClaimReport {
    claim: EvidenceClaim,
    verdict: ClaimVerdict,
}
```

`ClaimVerdict::is_fail_for(profile)` centralizes the fail-closed policy:
for any *named assurance claim* (cert/record, or a `check`/`trace --validate`
that requests a claim), every verdict other than `Satisfied`/`NotClaimed`
fails. Development mode maps `NotConfigured`/`NotAdopted`/`Empty` to an
explicit **adoption** terminal (§6), never to `VERIFY_OK`.

Each claim is evaluated by a checker that reads *bundle contents only*, so the
same function runs at generate-time (against the freshly written bundle) and
verify-time.

### 3.2 `generate` — evaluate then re-verify

`crates/cargo-evidence/src/cli/generate.rs` finalize path, after
`finalize_and_sign`:

1. Build the `AssuranceContract` from profile + DAL + boundary.
2. Evaluate every required claim against the written bundle. Fold results into
   `bundle_complete` (replacing the `tool_command_failures.is_empty()`-only
   decision).
3. **Run `verify_bundle` on the written bundle directory.** If it returns any
   error, emit `GENERATE_FAIL` — the generator cannot certify a bundle its own
   verifier rejects.
4. Only if the contract is satisfied *and* verify passes → `GENERATE_OK`.

This step (G2) is the keystone: it mechanically forecloses the entire "generate
succeeds where verify fails" class, and it is what makes each subsequent claim
self-enforcing on the dogfood CI.

### 3.3 `verify` — fail closed

`crates/evidence-core/src/verify/bundle.rs` gains a contract-evaluation pass
that maps `ClaimVerdict` → `VerifyError`. The existing structural checks
(`SHA256SUMS`, extra-file detection, `check_deterministic_manifest`,
`check_llr_test_selectors`, `check_bundle_completeness`) become the *evidence*
that individual claim checkers consume, not independent ad-hoc gates.

### 3.4 MCP layer

`evidence-mcp` shells out to `cargo evidence <verb> --format=jsonl`
(`server.rs:198-233`), so it inherits the unified verdict verbatim. No MCP-side
change beyond surfacing the new terminals/codes in its README.

---

## 4. Per-issue design

### 4.1 #138 — SourceBaseline claim

**Package → directory resolution.** Extend the boundary check's existing
`cargo metadata` deserialization (`boundary_check/metadata.rs:17-25`, currently
`Package { name, id, targets, links }`) to retain `manifest_path` — *share the
primitive* rather than spawning a second `cargo metadata`. Run it
unconditionally at generate (today it runs only when `forbid_build_rs ||
forbid_proc_macros`). For each `in_scope` name:

- resolve name → `manifest_path` → parent dir → path relative to workspace root;
- `git ls-files -- <dir>` (a real pathspec now);
- hash every returned file, recording the *reason in scope*.

Reject, per #138 AC:
- name not present in metadata → **missing package**;
- resolved dir escapes the workspace root → **path escape**;
- zero tracked files for an in-scope unit → **empty scope**.

**Workspace control inputs.** A deterministic, canonically-relative set hashed
exactly once: root `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`,
`cert/**` (boundary, floors, trace, baselines, signing pubkey), any declared
schema/build-script/generated-code inputs. Each carries a recorded reason.
Enumerate with `walkdir::WalkDir(...).follow_links(false)` per the repo walker
convention.

**Data.** `inputs_hashes.json` stays a `BTreeMap<path, sha256>` for
compatibility, but a sibling `inputs_manifest.json` (or an extended record)
carries `{ path, sha256, reason, unit }` so provenance is auditable.

**Guardrail.** Fixture workspace with a package whose crate name ≠ its
directory; an independent enumeration must agree with `inputs_hashes.json`
(#138 AC). Negative fixtures: missing package, path escape, ignored-but-required
generated input, zero-file scope.

### 4.2 #139 — OutputReproducibility claim + recipe_hash rename

**Artifact inventory.** Add a build phase running
`cargo build --workspace --message-format=json` (release/profile as
configured). Parse `compiler-artifact` messages → `{ filenames, target.name,
target.kind, features, profile }`. Call `hash_output` (`builder.rs:204` — first
caller) for each declared deliverable. Provide an explicit declared mechanism in
cert config for non-Cargo outputs. Reject a successful build that *expected*
outputs but captured none (#139 AC).

**recipe_hash.** Rename `deterministic_hash` → `recipe_hash` in
`index.json`, `DeterministicManifest` naming, README, the `determinism-compare`
CI job, baselines, and `cross_time_determinism` / `cross_platform_determinism`
tests. The field's documented claim narrows to *recipe identity*: target
triple, features, profile, locked-dependency hash, toolchain, controlled
environment, and the exact command recipe — an explicit superset of today's 11
fields (add target_triple + features + the command recipe digest).

**Reproducibility comparison.** Add a typed `reproducibility` object to
`index.json` and a three-way verdict to `diff`:

```
reproducibility = {
    input_digest:  sha256 over canonical inputs_hashes,
    recipe_digest: recipe_hash,
    output_digest: sha256 over canonical outputs_hashes,
}
```

`diff` reports three independent verdicts — (a) content integrity, (b) recipe
identity, (c) reproduced-output equality — so cross-host recipe parity can never
be reported as output reproducibility (#139 AC).

**Guardrail.** Fixtures: same-source/same-recipe/same-target compare equal only
when output digests compare equal; changed target / feature / build flag /
dependency lock / build-script input / output byte each produce the expected
distinct finding; zero-expected-and-captured and missing-declared-artifact fail
generation.

### 4.3 #140 — TestExecution claim (cargo-nextest)

**Capture.** Replace the bare `cargo test --workspace` (`phases.rs:235`) with
`cargo nextest run --message-format libtest-json` (nextest's structured output),
consumed for per-test identity. `nextest` becomes a required generate tool —
recorded in the env fingerprint and checked by `doctor` (a missing `nextest`
must be an explicit non-success, not a silent fallback).

**Identity.** Extend `TestOutcomeRecord` (`bundle/outcome_record.rs:120-189`)
with `package`, `binary`, `harness`, and a stable `execution_id`
(`{package}::{binary}::{module_path}::{name}`). The forward join
(`trace/test_backlinks.rs`) matches selectors against the fully-qualified,
binary-scoped path — eliminating both `__unknown_binary__` and the duplicate-
name `BTreeMap` collapse.

**Reconciliation.** Every summary count reconciles to a row or an explicit
category: `total == executed + filtered_out + ignored_without_row + doctest`.
Doctests (nextest does not run them) captured via a separate `cargo test --doc`
pass or recorded as an explicit `doctest` category so counts never silently
diverge (the 601-vs-596 gap).

**Verify-in-generate** (§3.2) closes the loop: the same
`check_llr_test_selectors` that fires 87 findings today runs before
`GENERATE_OK`.

**Guardrail.** Fixtures: duplicate test names in different binaries stay
distinguishable; every declared test selector resolves to ≥1 executed result
with the correct requirement UID or generation fails; summary totals reconcile
by documented categories; negative fixtures for unknown binary/module identity,
dangling selectors, skipped-required tests, stale results, post-generation trace
mutation. A full `generate → verify` loop on this repo's own bundle (absent from
the suite today — every existing parser test feeds a pre-merged fixture string).

### 4.4 #141 — Traceability claim (fail closed)

**Adoption state.** `read_all_trace_files` returns a typed
`TraceAdoption { NotConfigured, NotAdopted, Empty, Invalid, Valid }` instead of
silent empty defaults. Add a diagnostic code for a missing/empty root (none
exists — today it is only a `tracing::warn!`).

**Fail closed.** For any named assurance claim, any adoption state below
`Valid` fails. Development mode emits an explicit **adoption terminal** (§6),
never `VERIFY_OK` over zero evidence (#141 AC: *"No warning stream can terminate
VERIFY_OK when the requested claim has no evidence"*). The DAL-D relaxation
(`policy/evidence.rs`) continues to relax *per-requirement rigor* but may never
satisfy a claim with zero requirements — completeness policy becomes scoped and
typed by node kind/profile, with no profile permitted to satisfy a claim on an
empty graph.

**Derived requirements.** Extend `DerivedEntry` (`trace/entries.rs:262-290`,
today only `rationale` + unused `safety_impact`) with disposition/notification
and **review**, and validate all of them (safety_impact is never read today).

**Unify.** The five divergent wrappers — `trace --validate`
(`cli/trace.rs`), `check` (`cli/check.rs`), `generate`
(`generate/phases/trace_validation.rs`, its own `TraceValidationResult`),
`doctor` (`doctor/checks.rs`, its own `CheckResult`), and bundle `verify` —
consume the one `ClaimVerdict`.

**Guardrail.** Positive minimal graph proving the boundary between "empty" and
"valid". Stable non-success diagnostics for: missing root, empty root, empty
files, zero in-scope requirements, missing required layer, orphan requirement,
unresolved result. Update the tests that intentionally accept an empty DAL-D
graph (`doctor_cmd.rs:107-194` `downstream_dal_d_fixture_passes`,
`derived_trace_validation.rs`) to assert the new adoption terminal instead of
`DOCTOR_OK`/`VERIFY_OK`.

---

## 5. Data-model & schema changes

- `index.json`: `deterministic_hash` → `recipe_hash`; add `reproducibility`
  object; bump `index` schema version.
- `TestOutcomeRecord`: add `package`, `binary`, `harness`, `execution_id`; bump
  the outcomes schema version.
- New `inputs_manifest.json` (or extended input records) carrying
  `{ path, sha256, reason, unit }`.
- Non-empty `outputs_hashes.json` with real Cargo-artifact digests.
- `DerivedEntry`: add disposition/notification/review; validate safety_impact.
- `env.json`: record `nextest` version alongside `rustc`/`cargo`.

All schema bumps go through `schema_versions.rs`; `diagnostic_codes_locked`
and the schema-version tests are the mechanical guards.

---

## 6. New diagnostic codes & terminals

| Code / terminal | Owner | Fires when |
|---|---|---|
| `GENERATE_INCOMPLETE` (terminal) | generate | a required claim is unsatisfied or the post-generate verify rejects |
| `VERIFY_INPUTS_EMPTY` | verify | in-scope unit resolves to zero tracked inputs |
| `VERIFY_INPUT_UNRESOLVED` | verify | in-scope package name has no manifest dir (missing/path-escape) |
| `VERIFY_OUTPUTS_EMPTY` | verify | build expected artifacts but captured none |
| `VERIFY_TEST_IDENTITY_UNKNOWN` | verify | an outcome row lacks binary/module identity |
| `VERIFY_COUNT_UNRECONCILED` | verify | summary total ≠ Σ documented categories |
| `TRACE_NOT_CONFIGURED` / `TRACE_NOT_ADOPTED` / `TRACE_EMPTY` | trace | adoption state below `Valid` for a named claim |
| `VERIFY_ADOPTION` (terminal) | dev-mode | explicit non-success for absence-of-evidence, replaces the fail-open `VERIFY_OK` |

Every new terminal registers in `TERMINAL_CODES`; CLI-layer signals register in
`HAND_EMITTED_CLI_CODES`; `diagnostic_codes_locked` enforces the bijection.

---

## 7. Scope split — one issue per PR

Each PR seeds its SYS/HLR/LLR/TEST chain in the first commit (trace-first),
lands the fix **and** its guardrail, corrects the README claim it earns, and is
independently revertible. The contract + verify-in-generate infra lands *with*
PR-1 (thin); each later PR registers its claim, so the dogfood CI goes green
step by step with no big-bang keystone.

1. **PR-1 · #138 + spine.** `AssuranceContract`/`ClaimVerdict` skeleton +
   verify-in-generate wiring + `SourceBaseline` claim + package→dir resolution +
   control inputs + enumeration & negative fixtures. Inputs become non-empty
   first, so the gate can turn on.
2. **PR-2 · #140.** nextest capture + identity fields + count reconciliation;
   register `TestExecution`. Selectors resolve.
3. **PR-3 · #139.** artifact inventory + `hash_output` + `recipe_hash` rename +
   `reproducibility` object + typed `diff`; register `OutputReproducibility`.
4. **PR-4 · #141.** adoption-state typing + fail-closed + wrapper unification +
   derived-req review; register `Traceability`.

Ordering rationale: verify-in-generate turns the dogfood bundle red until each
content fix lands, so the content fixes precede/accompany their gate; #138 first
because inputs are the cheapest to make non-empty and unblock the gate.

---

## 8. Compatibility & migration

- **`recipe_hash` rename** is a breaking `index.json` change. Ripples:
  `determinism-compare` CI job, committed baselines, the
  `Override-Deterministic-Baseline:` protocol/lint
  (`scripts/deterministic-baseline-override-lint.sh`), README §"content_hash vs
  deterministic_hash". `verify` accepts only the new field; old bundles must be
  regenerated. Call this out in the PR-3 description and the changelog.
- **nextest dependency.** `generate` now requires `cargo-nextest`. CI installs
  it; `doctor` reports its absence as an explicit non-success; the README quick-
  start notes the prerequisite. Downstream adopters inherit the requirement.
- **Fail-closed default** flips previously-green empty-trace / empty-input dev
  runs to explicit non-success. This is the intended behavior change; the
  affected intentional tests are updated in-PR, not deleted.

---

## 9. Risks & mitigations

- **R1 — Dogfood CI red between PRs.** verify-in-generate rejects this repo's
  own bundle until #138/#140/#139 land. *Mitigation:* land content fixes with
  (not after) the gate that needs them; PR-1 registers only `SourceBaseline`.
- **R2 — nextest and doctests.** nextest does not run doctests; naive adoption
  drops them from the count. *Mitigation:* explicit `doctest` category via a
  separate `cargo test --doc` pass; reconciliation test asserts the category.
- **R3 — Over-broad control-input set.** Hashing all of `cert/**` could pull in
  generated or host-specific files, breaking determinism. *Mitigation:* the set
  is declared and git-tracked-only; `follow_links(false)`; an independent
  enumeration fixture pins it.
- **R4 — recipe_hash rename churn.** Wide surface. *Mitigation:* mechanical
  rename in one PR; schema-version bump + `diagnostic_codes_locked` catch
  stragglers.
- **R5 — Scope creep into full dep-graph capture.** Out of scope (N2); the
  `Cargo.lock` hash remains the dependency identity for these four issues.

---

## 10. Open questions to revisit during writing-plans

- **Q1.** `inputs_manifest.json` as a new sibling file vs. extending
  `inputs_hashes.json` entries in place (the latter breaks the plain
  `BTreeMap<path,sha256>` shape `diff` relies on).
- **Q2.** Non-Cargo output declaration format — a `[outputs]` table in
  `cert/boundary.toml` vs. a dedicated `cert/outputs.toml`.
- **Q3.** Exact recipe-digest field set for `recipe_hash` — minimal (target +
  features + lock + toolchain + rustflags) vs. including the full argv recipe of
  every captured command.
- **Q4.** Whether the dev-mode adoption terminal exits non-zero or exits zero
  with a distinct non-`OK` terminal (agents parse the terminal, humans parse the
  exit code) — must not regress the JSONL "exactly one terminal" invariant
  (HLR-001/002).
- **Q5.** Doctest execution identity under nextest — separate `--doc` pass vs.
  recording doctests as an un-selectored category exempt from the
  selector-resolution claim.
