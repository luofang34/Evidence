# Tool Qualification — `cargo-evidence`

Companion to `cert/boundary.toml`. An auditor reviewing evidence produced
by `cargo-evidence` reads this document to understand which DO-178C /
DO-330 objectives the tool claims to automate, which objectives it
explicitly does NOT automate (and why), and what stance the project
takes on the automation gaps.

When you run `cargo evidence doctor`, the `DOCTOR_QUALIFICATION_MISSING`
check looks for this file. Severity is Warning at DAL-D (advisory) and
Error at DAL ≥ C. Cert / record profile generation refuses to proceed
at DAL ≥ C without this file present.

## Integrity layers (SYS-001)

A bundle's integrity is verifiable along three layers, each requiring
strictly less trusted material than the next. DO-178C §7 (Software
Configuration Management) requires "protection against unauthorized
changes" but does not mandate any specific cryptographic primitive;
this tool's choice is documented here so an auditor can map the
artifact set onto the SCM expectation.

1. **Content layer.** `SHA256SUMS` lists every content-bearing file
   with its SHA-256 digest. Any party with `sha256sum` confirms every
   file. No key required. This is the layer most directly aligned
   with traditional CC1 baseline integrity (DO-178C §7.2.5).
2. **Metadata layer.** `BUNDLE.sig` is a 64-byte ed25519 detached
   signature, hex-encoded, over the length-prefixed
   `(SHA256SUMS, index.json)` envelope. `index.json` is excluded from
   `SHA256SUMS` because `index.json` records the SHA-256 of
   `SHA256SUMS` (a self-referential cycle); the signature covers it
   instead. The verifier needs the supplier's 32-byte public ed25519
   verifying key.
3. **Provenance layer.** A successful Layer-2 verification implies
   non-repudiation: only the holder of the corresponding private
   signing key could have produced a signature that the public key
   accepts. This satisfies cross-organizational provenance requirements
   that an HMAC-based integrity check cannot — HMAC is symmetric and
   the verifier would need the same secret as the signer (FIPS 198-1).

**Pre-0.1.5 history:** earlier releases used HMAC-SHA256 over the same
envelope and called it a "signature." That was a terminology error;
HMAC is a Message Authentication Code, not a signature. The mechanism
was correct for in-organization integrity but inadequate for cross-
organizational provenance. The 0.1.5 transition replaces the primitive
with ed25519 and keeps the same envelope shape and the same
`BUNDLE.sig` filename (now legitimate).

**Out of scope of this layer:**

- SLSA L2/L3 build-provenance attestations (in-toto / DSSE / sigstore)
  — a planned follow-up; the current signature is metadata-layer only.
- X.509 / PKI / institutional cert chains — the public key is raw and
  distributed by the supplier however they choose.
- Time-of-signing attestation — the signature is computed at bundle
  finalize, on the same host, and bound to that bundle only.

## A-7 Obj-7 (MC/DC) — capability gap

The most visible automation gap is DO-178C Annex A Table A-7 Objective 7
(Modified Condition / Decision Coverage), required at DAL-A. `cargo-
evidence` does **not** measure MC/DC on stable Rust.

**Verbatim gap statement** (rendered into each
`compliance/<crate>.json`'s `objectives["A7-7"].rationale` field so an
auditor sees the same words across every bundle this tool produces):

> MC/DC support is gated on rust-lang/rust#124144 resolution and
> semantics of arXiv:2409.08708 Section 4 sub-pattern table + Section 5
> guard clause specification. Until then, cargo-evidence does not
> attempt to certify MC/DC evidence for Rust code.

The gap is a capability limitation of stable Rust's coverage
instrumentation, not a tool-qualification weakness. Mitigation options
— ordered from strongest to weakest credit:

1. Submit MC/DC evidence from a DO-330-qualified auxiliary tool
   (LDRA, VectorCAST, Rapita). Auditor cross-references this bundle's
   structural-coverage evidence against the MC/DC submission.
2. Target DAL-B or lower (Obj-7 is `NotApplicable`; no gap).
3. Reduce the DAL of the component so MC/DC is not required.

### rustc upstream status

- **rust-lang/rust#124144** — MC/DC tracking issue. Status: active but
  nightly-only, labelled "implementation incomplete."
- **cargo-llvm-cov 0.8.4+** — exposes `--mcdc` flag but inherits the
  nightly-only + sharp-edge constraints. Not credit-eligible at any DAL
  on stable.
- **arXiv 2409.08708** — Zaeske, T.; Albini, P.; Gilcher, J.; Durak, U.
  *Journal of Aerospace Information Systems 22(10), 2025.* Defines
  Rust-specific MC/DC semantics for pattern-match constructs (`let-else`,
  `if let`, `?`, or-patterns) that C-derived MC/DC does not cover.
  Co-author Albini is a rustc contributor; the paper is the conceptual
  spec feeding #124144's pattern-matching design.

### Schema preparation

Even though no MC/DC evidence is emitted today, the
`coverage/coverage_summary.json` wire format reserves shape for future
MC/DC additions:

- `Measurement.level` enum includes `pattern_decision` and `mcdc` values.
- `FileMeasurement` has `decisions: Vec<DecisionCoverage>` and
  `conditions: Vec<ConditionCoverage>` vectors.

A future PR adding rustc-stable MC/DC measurement populates those
existing fields without breaking downstream consumers that pattern-match
on `level == "statement"` or `level == "branch"`.

`FileMeasurement` also carries a `branches: Option<BranchCoverage {
covered, total }>` field sibling to `lines`, populated on
`level == "branch"` and `None` on `level == "statement"`. This is
the structural source for the A-7 Obj-6 dimension's *engineering
gate* — aggregation over a Branch measurement reads
`branches`, never `lines`, so the gate cannot be
accidentally satisfied by line coverage. The gate is an
engineering quality check, not a compliance determination:
passing it never closes the decision-coverage objective (see the
credit table below).

## Rust-specific pattern-decision semantics

The paper above (Section 4.3) defines 13 sub-pattern refutability
classes that a Rust MC/DC implementation must account for. Reproduced
here as the reference table our implementation conforms to when rustc
stabilizes support:

| # | Pattern                     | Refutability                              |
|---|-----------------------------|-------------------------------------------|
| 1 | Wildcard `_`                | Irrefutable                               |
| 2 | Identifier binding `x`      | Irrefutable                               |
| 3 | Literal `42`, `"s"`, `true` | Refutable (matches one value)             |
| 4 | Range `0..=9`               | Refutable (matches a set of values)       |
| 5 | Reference `&p`              | Delegated to `p`'s refutability           |
| 6 | Struct `S { a, .. }`        | Refutable iff any sub-pattern refutable   |
| 7 | Tuple `(a, b)`              | Refutable iff any sub-pattern refutable   |
| 8 | Slice `[a, b, ..]`          | Refutable (length + element constraints)  |
| 9 | Path (variant) `E::A`       | Refutable iff enum has >1 variant         |
| 10 | Or `a \| b`                | Delegated to constituent patterns         |
| 11 | Guard `p if cond`           | Refutable (condition adds a decision)     |
| 12 | Constant expr `FOO`         | Refutable (only true `const` qualifies)   |
| 13 | Rest `..`                   | Irrefutable within its context            |

A Rust MC/DC implementation treats each refutable sub-pattern as a
condition within its enclosing decision (a `match` arm or `if let`
head). The test suite must demonstrate that every condition
independently affects the decision outcome.

## `const` exemption clarification

Paper Section 5.3 notes a subtlety relative to C-derived MC/DC:

> Immutable let bindings and `static` are NOT constant due to interior
> mutability.

C-style MC/DC tools typically short-circuit `const`-qualified conditions
out of the evaluation. In Rust only true `const` items and associated
`const` items qualify. An immutable `let` binding is **not**
exempt:

```rust
let x = 5;
if x < 10 {  // x must count as a condition, not const-exempt
    ...
}
```

An auditor accepting a future MC/DC submission against this tool should
reject any claim that extends the C-style const exemption to immutable
`let` bindings.

## What `cargo-evidence` DOES qualify

| DO-178C Objective              | Tool credit                      | Evidence artifact                          |
|--------------------------------|----------------------------------|--------------------------------------------|
| A-3 Obj-6 (HLR traceability)   | Mechanical when trace present    | `trace/matrix.md`, `trace/hlr.toml`        |
| A-4 Obj-6 (LLR traceability)   | Mechanical when trace present    | `trace/llr.toml`, `trace/matrix.md`        |
| A-6 Obj-5 (source-to-LLR)      | Partial (needs `test_selectors`) | `trace/llr.toml`                           |
| A-7 Obj-3 (test cases)         | Mechanical + per-test artifact   | `tests/test_outcomes.jsonl`                |
| A-7 Obj-5 (statement coverage) | Metric + engineering gate only — `Met` requires a recorded analysis/disposition of uncovered structure | `coverage/coverage_summary.json`           |
| A-7 Obj-6 (decision coverage)  | Approximation via LLVM branches — caps at `ManualReviewRequired`, never mechanically `Met` | `coverage/coverage_summary.json`, `lcov.info` |

Coverage percentages are **engineering metrics, not compliance
evidence**. The metrics inform the objective; what closes it is
documented analysis/disposition of uncovered structure, tool
identity and qualification evidence, and manual/independent review —
each recorded as a separate evidence kind. Approximate (LLVM branch)
or otherwise unqualified coverage evidence yields
`ManualReviewRequired`, never `Met`.

Objectives not in this table default to `ManualReviewRequired`: the
tool emits the bundle but the human reviewer must confirm the
objective. Tables A-3, A-4, A-5, A-6 bodies largely live here.

## Versioning

This document is versioned alongside `cargo-evidence`. When the tool
gains new qualification credit (e.g., a stable-Rust MC/DC measurement),
the gap statement updates in place and the `DOCTOR_QUALIFICATION_MISSING`
check re-reads this file on every `doctor` / `generate` run.

Downstream projects that fork or supplement this file should retain the
verbatim A-7 Obj-7 gap-statement block above so auditors see consistent
language across submissions that use `cargo-evidence`.
