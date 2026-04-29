# evidence-mcp

MCP (Model Context Protocol) server that exposes six `cargo
evidence` verbs over stdio. Subprocess wrapper only — it spawns the
already-installed `cargo-evidence` binary and surfaces the CLI's
JSONL or JSON output as structured MCP responses.

Part of the [Evidence](https://github.com/luofang34/Evidence) project
(DO-178C / DO-330 evidence bundles for Rust crates). The workspace
README covers the project's goals, architecture, and invariants; this
page is install + host-configuration notes for MCP clients.

## Tools exposed

| Tool              | Wraps                                 | Side-effect scope | Purpose                                                           |
| ----------------- | ------------------------------------- | ----------------- | ----------------------------------------------------------------- |
| `evidence_ping`   | *(cached startup probe, no spawn)*    | Inspection        | Liveness + version-skew probe; returns cached MCP/CLI versions    |
| `evidence_rules`  | `cargo evidence rules --json`         | Inspection        | Full manifest of diagnostic codes the tool can emit               |
| `evidence_doctor` | `cargo evidence doctor --format=jsonl`| Inspection        | Audit a workspace's rigor adoption (floors, trace, CI, merge-style)|
| `evidence_floors` | `cargo evidence floors --format=jsonl`| Inspection        | Query ratchet-gate state; streams dimension-level pass/fail events |
| `evidence_diff`   | `cargo evidence diff <a> <b> --json`  | Inspection        | Compare two on-disk bundles; returns the structured delta blob    |
| `evidence_check`  | `cargo evidence check --format=jsonl` | **Execution** (source mode) | One-shot pass/gap validation of a workspace or a bundle |

**Inspection vs execution.** Only `evidence_check` with
`--mode=source` *executes project code* — it runs
`cargo test --workspace` and therefore carries whatever
side-effects those tests have (file writes under the workspace,
bound sockets, env mutations, spawned processes). Every other
verb, and `evidence_check --mode=bundle`, reads files and
reports — no project-code execution. Hosts gating tool calls on
side-effect scope should treat `evidence_check --mode=source` as
execution and the rest as inspection.

`verify` is intentionally not exposed — `check` in bundle mode
delegates to it internally.

## Install

```sh
cargo install evidence-mcp
cargo install cargo-evidence   # evidence-mcp shells out to this
```

Both must be on `$PATH` for the MCP server to work. The server
itself is the binary `evidence-mcp`; it needs `cargo-evidence`
reachable because every tool call is a subprocess spawn.

## Register with an MCP host

### Claude Code (CLI)

```sh
claude mcp add evidence evidence-mcp
claude mcp list   # should show "evidence: evidence-mcp  - ✓ Connected"
```

### Claude Desktop (GUI)

Add to your MCP config
(`~/Library/Application Support/Claude/claude_desktop_config.json` on macOS):

```json
{
  "mcpServers": {
    "evidence": {
      "command": "evidence-mcp"
    }
  }
}
```

### Other stdio MCP hosts

Point the host at the `evidence-mcp` binary; no command-line
arguments or environment variables are required. The server
resolves the workspace from each tool call's `workspace_path`
argument, falling back to the server's working directory with a
visible `MCP_WORKSPACE_FALLBACK` warning in the response.

## Configuration

| Variable | Default | Range | Effect |
|---|---|---|---|
| `EVIDENCE_MCP_TIMEOUT_SECS` | `600` | `60`–`7200` | Per-spawn cap on every `cargo evidence` subprocess. Read on each tool call. Exceeding it surfaces `MCP_SUBPROCESS_TIMEOUT` in the tool response; the child is killed. Out-of-range values clamp and emit a `tracing::warn!`; unparseable values fall back to the default. |
| `RUST_LOG` | `warn` | standard `tracing_subscriber` | Log level for server-side `tracing` output. Written to stderr — stdout is reserved for MCP protocol frames. |

## Response shapes

Three response shapes cover the six verbs, chosen by how the
underlying CLI emits output:

**`JsonlToolResponse`** — used by `evidence_check`,
`evidence_doctor`, `evidence_floors`. The CLI emits one JSONL
diagnostic per event and a terminal row:

- `exit_code` — subprocess exit code. `0` on success, `2` on
  verification failure or tool-layer subprocess failure (cargo
  not on `PATH`, timeout, malformed output — each flipped to a
  matching `MCP_*` terminal).
- `terminal` — the final `.code`. `VERIFY_OK` / `VERIFY_FAIL`
  for `check`, `DOCTOR_OK` / `DOCTOR_FAIL` for `doctor`,
  `FLOORS_OK` / `FLOORS_FAIL` for `floors`. On MCP-layer
  failure: `MCP_CARGO_NOT_FOUND`, `MCP_SUBPROCESS_TIMEOUT`,
  `MCP_MALFORMED_JSONL`, `MCP_NO_OUTPUT`,
  `MCP_SUBPROCESS_SPAWN_FAILED`.
- `diagnostics` — every parsed JSONL row, in stream order.
- `summary` — code-to-count map across `diagnostics`.

**`RulesToolResponse`** — used by `evidence_rules`. The CLI
emits a single JSON array (not a stream):

- `exit_code` — `0` on success, `2` on tool-layer failure.
- `rules` — the full manifest array (empty on failure).
- `count` — `rules.len()` for quick drift checks against
  `evidence_core::RULES.len()`.
- `warnings` — MCP-layer signals (version-skew warnings);
  empty in the happy path.
- `error` — `Some(MCP_* diagnostic)` on tool-layer failure;
  `None` on success.

**`PingResponse`** — used by `evidence_ping`:

- `mcp_version` — evidence-mcp's own `CARGO_PKG_VERSION`.
- `cli_version` — the probed cargo-evidence version
  (`Some(v)` on matched / skewed, `None` on probe failure).
- `skew` — `"matched"` / `"skewed"` / `"probe_failed"`.
- `probe_error` — `Some(reason)` only when
  `skew == "probe_failed"`.

**`DiffToolResponse`** — used by `evidence_diff`. The CLI
emits a single JSON document (not JSONL):

- `exit_code` — `0` on success (differences are reported, not
  judged), `2` on tool-layer failure.
- `diff` — the raw delta blob
  (`{bundle_a, bundle_b, inputs_diff, outputs_diff,
  metadata_diff, env_diff}`). `None` on failure.
- `warnings` — as above.
- `error` — as above.

All `MCP_*` codes appearing in any response field are
registered in `evidence_core::HAND_EMITTED_MCP_CODES` — agents
pattern-match on `.code` against that list.

## Failure-shape contract

`exit_code` is documentation, not the machine contract. Two
distinct failure classes deliberately share `exit_code = 2`:

| Failure class | Origin | Terminal / `error.code` |
|---|---|---|
| **CLI verification failure** | `cargo evidence` ran to completion and reported one of its own failure terminals | `VERIFY_FAIL` / `DOCTOR_FAIL` / `FLOORS_FAIL` |
| **MCP tool-layer failure** | The wrapper couldn't run the subprocess to completion | `MCP_CARGO_NOT_FOUND` / `MCP_SUBPROCESS_SPAWN_FAILED` / `MCP_SUBPROCESS_TIMEOUT` / `MCP_MALFORMED_JSONL` / `MCP_NO_OUTPUT` |

**Dispatch on the structured field, not on `exit_code`.** Per
verb:

- `evidence_check` / `evidence_doctor` / `evidence_floors` —
  read `JsonlToolResponse.terminal`.
- `evidence_rules` / `evidence_diff` — read the `code` field on
  `error` (when `error.is_some()`).
- `evidence_ping` — read `skew`.

Hosts that branch on `exit_code` alone cannot distinguish a
real CLI verification fail from a wrapper failure. The bit is
intentionally erased there because the structured field
already carries the discriminator at machine precision; the
`exit_code = 2` collision is a deliberate design choice
documented at
[`TOOL_FAILURE_EXIT_CODE`](https://docs.rs/evidence-mcp).

## What this crate is not

- Not a Rust library API for programmatic access to evidence bundles
  — use [`evidence-core`](https://crates.io/crates/evidence-core)
  directly for that.
- Not a build-time dependency. This is a runtime MCP server meant
  to be launched by an MCP host.
- Not a replacement for `cargo-evidence` — it's a thin wrapper so
  agents can call the same verbs over MCP instead of shell.

## License

Dual-licensed under MIT OR Apache-2.0.
