# Compliance Review: cargo-evidence

**Date:** 2026-02-07
**Status:** P0 issues FIXED, P1 items tracked

## Tool Qualification

- Classification: Development tool (§1.6.1) + Verification tool (§1.6.3)
- TQL Assessment: TQL-4 minimum, TQL-3 depending on DAL
- Self-verifying loop concern: tool generates AND verifies its own bundles (§4.2 independence)

## P0 Issues (FIXED)

1. **Timestamp determinism** — index.json excluded from SHA256SUMS; content_hash field added
2. **Cert-mode strictness** — unknown git state, unreadable files, missing tools → hard errors
3. **Platform path normalization** — backslash → forward slash in SHA256SUMS
4. **Bundle overwrite protection** — existing directory causes error
5. **Profile discrimination** — bundle dir prefixed with profile name
6. **Reverse traceability** — Test→LLR→HLR, orphan test detection, HLR→Test roll-up

## P1 Issues (TRACKED)

1. ~~No cryptographic signing~~ → **RESOLVED**: HMAC-SHA256 via `sign_bundle()` + `BUNDLE.sig`
2. **No structured SVR capture** (DO-178C Table A-7) — `TestSummary` captures totals, but per-test results not yet structured
3. ~~No extra-file detection in verify~~ → **RESOLVED**: `verify.rs` walks bundle, flags unexpected files
4. ~~Incomplete SCI/SECI~~ → **PARTIALLY RESOLVED**: `Cargo.lock` hash, `RUSTFLAGS`, `rust-toolchain.toml` now captured; linker version still missing
5. **No derived requirements safety report** (DO-178C §5.2.2)
6. **`engine_git_sha` conflation** — records consumer project SHA, not engine build SHA

## Data Item Coverage

| Data Item | Status |
|-----------|--------|
| SCI (§11.16) | Partial — git + env + `Cargo.lock` hash + `rust-toolchain.toml` captured |
| SECI (§11.15) | Partial — `RUSTFLAGS` captured, missing linker version |
| SVR (§11.14) | Partial — `TestSummary` (totals), per-test not yet structured |
| Trace (§6.3.4) | Full — bidirectional with orphan detection |
| SCM Records | Missing |
| HMAC Signing (§7.2.6) | Implemented |

## ADR-001 Invariant Compliance

| Invariant | Status |
|-----------|--------|
| 1. Non-mutating | PASS — --out-dir required |
| 2. Self-describing | PASS — index.json + SHA256SUMS |
| 3. Deterministic | PASS after P0 fix — content_hash is reproducible |
| 4. Data-driven | PASS — TOML policy files |
| 5. Offline-capable | PASS — no network calls |
| 6. Cross-platform | PASS after P0 fix — forward slash normalization |

