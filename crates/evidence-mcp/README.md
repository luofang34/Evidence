# evidence-mcp

MCP (Model Context Protocol) server that exposes three `cargo
evidence` verbs over stdio. Subprocess wrapper only — it spawns the
already-installed `cargo-evidence` binary and surfaces the CLI's
JSONL output as structured MCP responses.

Part of the [Evidence](https://github.com/luofang34/Evidence) project
(DO-178C / DO-330 evidence bundles for Rust crates). The workspace
README covers the project's goals, architecture, and invariants; this
page is install + host-configuration notes for MCP clients.

## Tools exposed

| Tool             | Wraps                             | Purpose                                                    |
| ---------------- | --------------------------------- | ---------------------------------------------------------- |
| `evidence_check` | `cargo evidence check --format=jsonl` | One-shot pass/gap validation of a workspace or a bundle |
| `evidence_doctor`| `cargo evidence doctor --format=jsonl`| Audit a workspace's rigor adoption (floors, trace, CI)  |
| `evidence_rules` | `cargo evidence rules --json`         | Full manifest of diagnostic codes the tool can emit     |

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

## Response shape

Every tool call returns structured content with:

- `exit_code` — the underlying `cargo evidence` subprocess exit code
- `terminal` — the final diagnostic (`VERIFY_OK` / `VERIFY_FAIL` /
  `DOCTOR_OK` / `DOCTOR_FAIL`, depending on tool)
- `diagnostics` — parsed JSONL diagnostic stream (per-requirement
  records for `check`, per-objective for `doctor`)
- `summary` — code-to-count map across the diagnostics

`evidence_rules` additionally returns the raw manifest array and
a `count` field for drift detection.

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
