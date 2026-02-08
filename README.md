# cargo-evidence

**Certification evidence generation, verification, and traceability for Rust projects.**

[![Crates.io](https://img.shields.io/crates/v/cargo-evidence.svg)](https://crates.io/crates/cargo-evidence)
[![CI](https://github.com/user/evidence/actions/workflows/ci.yml/badge.svg)](https://github.com/user/evidence/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/cargo-evidence.svg)](LICENSE-MIT)

`cargo-evidence` generates self-describing, deterministic, and offline-verifiable
evidence bundles for safety-critical Rust builds. It captures environment
fingerprints, source/artifact hashes, command logs, and bidirectional traceability
matrices -- everything a DER needs to evaluate your tool qualification data.

---

## Quick Start

Get your first evidence bundle in under 30 seconds:

```bash
# Install
cargo install cargo-evidence

# Initialize evidence tracking in your project
cargo evidence init

# Generate an evidence bundle
cargo evidence generate --out-dir ./evidence

# Verify bundle integrity
cargo evidence verify ./evidence/dev-20260207-*/
```

That is it. The `init` command scaffolds a `cert/` directory with boundary
configuration, profile templates, and example trace files. The `generate`
command produces a complete evidence bundle. The `verify` command checks every
SHA-256 hash in the bundle and confirms structural integrity.

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
  index.json              # Bundle metadata, schema versions, content_hash
  env.json                # Environment fingerprint (rustc, cargo, LLVM, libc, OS)
  inputs_hashes.json      # SHA-256 hashes of all source inputs
  outputs_hashes.json     # SHA-256 hashes of all build outputs
  commands.json           # Recorded command executions with exit codes
  SHA256SUMS              # Content-layer integrity manifest
  trace/                  # Traceability matrix outputs (Markdown)
    matrix.md             # HLR <-> LLR <-> Test bidirectional matrix
  tests/                  # Test execution artifacts (stdout/stderr)
```

**Design invariants:**

- `SHA256SUMS` covers all content-layer files. `index.json` is metadata-layer
  and excluded from `SHA256SUMS`, so timestamps never affect the content hash.
- `content_hash` in `index.json` is the SHA-256 of `SHA256SUMS` itself --
  two runs on the same commit produce the same `content_hash`.
- All paths in `SHA256SUMS` use forward slashes, regardless of OS.
- Bundle directories are prefixed with the profile name to prevent accidental
  submission of `dev` bundles as `cert`.
- Existing bundle directories are never overwritten.

---

## Commands Reference

### `cargo evidence generate`

Generate a new evidence bundle.

```bash
cargo evidence generate --out-dir ./evidence
cargo evidence generate --out-dir ./evidence --profile cert
cargo evidence generate --out-dir ./evidence --boundary cert/boundary.toml
cargo evidence generate --out-dir ./evidence --trace-roots cert/trace
cargo evidence generate --out-dir ./evidence --json --quiet
```

| Flag                | Description                                           |
|---------------------|-------------------------------------------------------|
| `--out-dir <DIR>`   | Output directory (required unless `--write-workspace`) |
| `--profile <PROF>`  | Override auto-detected profile (`dev`/`cert`/`record`) |
| `--boundary <FILE>` | Path to `boundary.toml` (default: `cert/boundary.toml`)|
| `--trace-roots <D>` | Comma-separated trace root directories                 |
| `--write-workspace` | Write to `evidence/` in workspace (xtask integration)  |
| `--quiet`, `-q`     | Suppress non-error output                              |
| `--json`            | Output results as JSON                                 |

### `cargo evidence verify`

Verify an existing evidence bundle.

```bash
cargo evidence verify ./evidence/cert-20260207-143022Z-a1b2c3d4/
cargo evidence verify ./evidence/cert-20260207-*/  --strict
cargo evidence verify ./evidence/cert-20260207-*/  --json
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

| # | Invariant        | Status | How enforced                                         |
|---|------------------|--------|------------------------------------------------------|
| 1 | Non-mutating     | PASS   | `--out-dir` required; never modifies source tree      |
| 2 | Self-describing  | PASS   | `index.json` + `SHA256SUMS` in every bundle           |
| 3 | Deterministic    | PASS   | `content_hash` excludes timestamps; BTreeMap ordering  |
| 4 | Data-driven      | PASS   | TOML policy files in `cert/`                           |
| 5 | Offline-capable  | PASS   | Zero network calls; all operations are local           |
| 6 | Cross-platform   | PASS   | Forward-slash normalization in all hash paths           |

### Known Limitations (P1 Items)

These items are tracked and not yet resolved:

1. **No structured SVR capture** -- exit codes and `TestSummary` are recorded but
   per-test-case structured verification results are not.
2. **No derived requirements safety report** -- derived LLRs are validated but no
   summary report is generated for safety analysis.
3. **`engine_git_sha` conflation** -- records the consuming project's SHA instead of
   the evidence engine's own build-time SHA.

Previously tracked items now resolved:

- ~~No cryptographic signing~~ → HMAC-SHA256 via `sign_bundle()` + `BUNDLE.sig`
- ~~No extra-file detection~~ → `verify.rs` walks bundle and flags unexpected files
- ~~Incomplete SCI/SECI~~ → `Cargo.lock` hash, `RUSTFLAGS`, `rust-toolchain.toml` captured



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
