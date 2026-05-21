# evidence-mcp — local conventions

MCP (Model Context Protocol) server. Stateless per-request — every tool
call resolves the workspace path, spawns a fresh `cargo evidence <verb>`
subprocess, parses the result, and returns. The one piece of server-
lifetime state is a cached `VersionSkew` from the startup probe of
`cargo evidence --version`.

For per-module trace + boundary + floors context on any file in this crate,
call `evidence_context` (MCP) or `cargo evidence context <path>`.

## Module layout

- `server.rs` — `Server` struct, `ServerHandler` impl, one `#[tool]` method
  per surface
- `server/responses.rs` — response shapers
  (`jsonl_response_from_run_error`, `prepend_skew_signal`, etc.)
- `subprocess.rs` — `run_evidence()` spawner, `parse_jsonl`, timeout cap
- `schema.rs` — request / response types (`schemars`-derived)
- `version_probe.rs` — startup `cargo evidence --version` probe + skew
  classification
- `workspace.rs` — workspace path resolution + `MCP_WORKSPACE_FALLBACK`
  signal

## Conventions

- **Streaming verbs** (`check`, `doctor`, `floors`) go through
  `Server::run_streaming_verb`. Skipping the helper is the only way to
  miss the version-skew + workspace-fallback prepend. Don't.
- **Single-blob verbs** (`rules`, `diff`, `ping`, `context`) shape their
  own responses but still call `skew_diagnostic` to populate
  `warnings`.
- **`name = "evidence-mcp"`** on `#[tool_handler]` is load-bearing —
  `rmcp`'s default identifies the server as `"rmcp"` in the `initialize`
  response. Don't remove the override (LLR-062 pins it).
- **Version skew** is probed **once** at `Server::new()` and cached in an
  `Arc<VersionSkew>`. Per-request skew checks read the cache; they don't
  re-probe.
- **No new transport machinery** for new tools. Reuse `run_evidence` +
  `parse_jsonl` + `server/responses.rs`. If a new verb truly needs new
  plumbing, justify it in the LLR.
- **Subprocess timeout:** capped by `EVIDENCE_MCP_TIMEOUT_SECS`
  (default 600s) — `evidence_check --mode=source` can run `cargo test
  --workspace` for minutes.

## Scoped test command

```bash
cargo test -p evidence-mcp --all-targets
```

(Tests spawn the MCP binary via `assert_cmd`; expect ~10s overhead vs.
evidence-core's unit-only tests.)
