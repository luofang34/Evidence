# evidence-core — local conventions

Library crate. No `main`, no CLI parsing. Everything here is callable from
`cargo-evidence` (CLI), `evidence-mcp` (MCP server), and any downstream
consumer that wires the library in directly.

For per-module trace + boundary + floors context on any file in this crate,
call `evidence_context` (MCP) or `cargo evidence context <path>`.

## Module groups

- `trace/` — SYS / HLR / LLR / TEST parser, validator, matrix generator
- `hash/` — SHA-256 over files + directory trees (streaming I/O)
- `env/` — environment fingerprint capture
- `verify/` — bundle verification (re-hashes, cross-file checks)
- `policy/` — `Dal` enum + per-DAL `TracePolicy` derivation
- `boundary_check/` — `boundary.toml` enforcement
- `compliance/` — DO-178C Annex A objectives (per-DAL applicability)
- `coverage/` — coverage level + summary types
- `floors/` — measurement helpers for the ratchet
- `rules/` — diagnostic code registry (the self-describe surface)
- `diagnostic/` — `DiagnosticCode` + `FixHint` enums + terminal-code sets

## Conventions

- **Errors:** `thiserror` typed enums. Never `anyhow`. Each variant carries
  the context for its message (paths, IDs, counts) via `#[source]` or
  `#[from]`. The workspace's `disallowed_types` clippy rule bans
  `anyhow::Error` here.
- **Determinism:** every output (Markdown matrices, JSON blobs) sorts via
  `BTreeMap` / sorted iteration. Adding a `HashMap` in an output path is a
  determinism regression.
- **Tests:** unit tests live in `src/<module>/tests.rs` with direct access
  to internals. Integration tests in `tests/` use the public API only.
- **File-tree walks:** always `WalkDir::new(...).follow_links(false)`. Pinned
  by `walker_usage_locked`. Single-directory non-recursive uses must be
  allowlisted with written justification.
- **No `process::exit()`.** Library code returns `Result`; the binary
  decides exit codes.

## Scoped test command

```bash
cargo test -p evidence-core --all-targets
```

(Use this when only this crate changed — `cargo test --workspace` rebuilds
`cargo-evidence` and spawns the MCP binary in `evidence-mcp/tests/`, adding
seconds.)
