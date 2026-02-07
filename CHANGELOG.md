# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-02-07

### Added
- Evidence bundle generation with three-tier profiles (dev/cert/record)
- Bundle verification with SHA-256 integrity checking
- Traceability matrix generation (HLR -> LLR -> Test, bidirectional)
- Reverse trace table (Test -> LLR -> HLR) and end-to-end HLR -> Test roll-up
- Evidence diff command for comparing bundles
- Project initialization (`cargo evidence init`)
- JSON schema validation for all bundle file types (index, env, commands, hashes)
- Deterministic hashing (`content_hash` excludes timestamps via two-layer design)
- Platform path normalization (forward slashes on all OS)
- Cert-mode strict enforcement (hard errors on unknown git state, missing tools)
- Orphan test detection in traceability validation
- Bundle overwrite protection (refuses to write to existing bundle directory)
- Profile-prefixed bundle directories (`cert-20260207-...`, `dev-20260207-...`)
- Environment fingerprinting (rustc, cargo, LLVM version, libc, host OS/arch, Nix detection)
- Boundary configuration (`cert/boundary.toml`) for scoping certifiable crates
- Ownership-aware trace link validation (cross-crate link restrictions)
- Derived requirement tracking with mandatory rationale
- Coverage summary and gap report in traceability matrix output
- JSON output mode (`--json`) for all commands
- Nix flake for reproducible builds
- CI workflow (Linux, macOS, Windows matrix)

### Compliance
- ADR-001: Six evidence engine invariants codified and enforced
- Certification data item coverage: SCI (partial), SECI (partial), trace data (bidirectional)
- Tool qualification level assessment documented (TQL-4 minimum)
- Compliance review with P0 items resolved and P1 items tracked
