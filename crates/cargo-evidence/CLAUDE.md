# cargo-evidence — local conventions

The user-facing binary: a Cargo subcommand (`cargo evidence <verb>`). All
CLI parsing, flag handling, and stdout shaping live here. Library logic
belongs in `evidence-core`.

For per-module trace + boundary + floors context on any file in this crate,
call `evidence_context` (MCP) or `cargo evidence context <path>`.

## CLI layout

- `src/cli/<verb>/` — one module per verb (`check`, `generate`, `verify`,
  `diff`, `init`, `schema`, `floors`, `rules`, `context`, `keygen`)
- `src/cli/output.rs` — JSONL emission helpers (`emit_jsonl` flushes per
  event so partial-stream readers see one whole record at a time)
- `src/cli/parse.rs` — clap derive structs

## Conventions

- **Agent-facing verb is `check`.** Humans use `verify` / `generate`. Agents
  default to `check` (auto-detects source vs bundle). Don't add new
  agent-facing flows that bypass `check`.
- **JSONL invariants** (HLR-001, HLR-002): every `--format=jsonl` run emits
  **exactly one** terminal as the last stdout line (`*_OK` / `*_FAIL` /
  `*_ERROR`). Tracing logs + human prose go to stderr. The JSONL error path
  keeps stderr silent so agents reading both streams don't see duplicate
  data.
- **Terminals are hand-emitted**, not via `DiagnosticCode`. Register every
  new terminal in `evidence_core::diagnostic::TERMINAL_CODES` —
  `diagnostic_codes_locked` fails CI if you forget.
- **CLI-layer signals** (not from a `DiagnosticCode` impl) must be listed
  in `evidence_core::diagnostic::HAND_EMITTED_CLI_CODES` so the bijection
  test stays green.
- **No `process::exit()`.** Each verb returns a `Result`; `main()` maps
  the result to an exit code.
- **`unwrap_used` / `expect_used` / `panic` / `todo` deny.** The
  workspace `[lints.clippy]` block forbids them. Tests may opt into
  `#[allow(clippy::expect_used, clippy::panic)]` where a `Result`-
  returning pattern isn't ergonomic.

## Scoped test command

```bash
cargo test -p cargo-evidence --all-targets
```

(Includes the golden-fixture integration tests under `tests/fixtures/`.
Regenerate fixtures via `tools/regen-golden-fixtures.sh` — do not edit
them by hand.)
