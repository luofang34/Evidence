# Agent-context from Evidence — design spec

**Date:** 2026-05-19
**Status:** Draft — pending user review
**Goal:** Make `cargo-evidence`'s trace + boundary + floors graph, surfaced
through `evidence-mcp` and a new `cargo evidence context` CLI verb, serve as
the per-module context substrate that coding agents read before editing —
both for this repo and for any project that adopts `cargo-evidence`.

This spec follows the brainstorming → writing-plans → implementation flow.
The writing-plans skill consumes this file to produce the per-PR
implementation plan.

---

## 1. Background

Anthropic's [*How Claude Code Works in Large
Codebases*](https://claude.com/blog/how-claude-code-works-in-large-codebases-best-practices-and-where-to-start)
identifies seven extension points: `CLAUDE.md` files, hooks, skills, plugins,
LSP, MCP servers, and subagents. Two of its core recommendations are directly
relevant here:

1. **Lean, layered `CLAUDE.md`.** Root file = big picture; subdirectory files
   = local conventions. Loading everything into root degrades performance;
   per-subdirectory init keeps context focused.
2. **MCP servers exposing structured search.** "The most sophisticated teams
   build MCP servers exposing structured search as a tool Claude can call
   directly," reducing reliance on grep-based exploration.

This project already has the underlying data:

- `cert/trace/{sys,hlr,llr,tests}.toml` — every LLR carries `modules =
  ["evidence_core::trace::..."]`, `emits = ["DIAG_CODE", ...]`, and
  `traces_to = [HLR-uid, ...]`. The trace graph is a *queryable per-module
  spec*: given any source path, it can return the requirements governing it,
  the tests verifying it, the diagnostic codes it owns, and the SYS root that
  justifies it.
- `cert/boundary.toml` — per-crate DAL, in-scope set, forbidden
  dependencies.
- `cert/floors.toml` — per-dimension regression gate.
- `evidence-mcp` — already exposes six tools (`check`, `rules`, `doctor`,
  `floors`, `diff`, `ping`).

The gap is the bridge: today an agent editing `crates/x/src/y.rs` cannot
directly ask "what is the trace context for this file?" — it has to grep
TOML. Downstream projects that adopt `cargo-evidence` get the data but no
agent-facing scaffolding.

## 2. Goals & non-goals

### Goals

- **G1.** From an MCP-connected agent, a single call returns the trace +
  boundary + floors slice for any selector (file / crate / module).
- **G2.** Humans and non-MCP agents reach the same data via `cargo evidence
  context`.
- **G3.** This repo demonstrates the pattern: root `CLAUDE.md` stays lean +
  three per-crate `CLAUDE.md` files carry local conventions only.
- **G4.** A project that runs `cargo evidence init` can opt into the same
  scaffolding (a starter root `CLAUDE.md`, an `.claude/settings.json`
  snippet) without obligating any agent-facing markdown beyond what they
  ask for.
- **G5.** All four PRs are independently revertible and individually
  valuable.

### Non-goals

- **N1.** Auto-generated per-module `CLAUDE.md` from the trace. Mixing
  generated and hand-written conventions risks the article's
  "everything-in-`CLAUDE.md`" anti-pattern. Structured data belongs behind a
  query, not in `CLAUDE.md`.
- **N2.** A Stop-hook recipe that turns `check` GAPs into proposed
  `CLAUDE.md` edits. Plausible follow-up; out of scope here.
- **N3.** Packaging `evidence-mcp` as a Claude Code plugin. Worth a
  separate spec once PRs 1–4 land.
- **N4.** LSP integration. Orthogonal to what `cargo-evidence` uniquely
  provides.
- **N5.** Backfilling `CLAUDE.md` files into already-adopted downstream
  projects automatically. Adoption is opt-in via `init` or by hand.

## 3. Surfaces

### 3.1 MCP tool — `evidence_context`

Request:

```jsonc
{
  "workspace_path": "/optional/abs",
  "selector":       "<file|crate|module|null>"
}
```

`selector` semantics (resolution order, first match wins):

1. **File** — a relative path under `crates/<crate>/...` or an absolute
   path inside `workspace_path`.
2. **Crate** — a workspace crate name (matches `[package].name` in
   `crates/*/Cargo.toml`).
3. **Module** — a Rust module path (`evidence_core::trace`) matched against
   each requirement's `modules` field.
4. **Null** (omitted) — returns the workspace overview only.

Response (single JSON blob, ordered fields):

```jsonc
{
  "selector":         { "kind": "file|crate|module|workspace", "input": "...", "resolved": "..." },
  "crate":            "evidence-core",
  "dal":              "D",
  "requirements":     [ { "id":"LLR-001", "uid":"...", "layer":"llr",
                          "title":"...", "description":"...",
                          "modules":[...], "emits":[...],
                          "traces_to":[...], "verification_methods":[...] } ],
  "parents":          [ { "id":"HLR-001", "uid":"...", "layer":"hlr",
                          "title":"...", "traces_to":["<sys-uid>"] } ],
  "tests":            [ { "id":"TEST-001", "uid":"...", "name":"...",
                          "selector":"...", "traces_to":[...] } ],
  "diagnostic_codes": [ "VERIFY_OK", "VERIFY_FAIL", "VERIFY_ERROR" ],
  "floors":           [ { "dimension":"test_count", "current":42, "floor":40 } ],
  "boundary":         { "in_scope":true, "forbidden_deps":[...] },
  "conventions":      { "nearest_claude_md":"crates/evidence-core/CLAUDE.md" },
  "warnings":         [ /* CONTEXT_NO_REQUIREMENTS_FOR_SELECTOR, etc. */ ]
}
```

Properties:

- **Pure inspection.** No `cargo test`. Reads `cert/`, `Cargo.toml`,
  source paths.
- **Cheap.** Designed to be called on every agent loop iteration. Order of
  magnitude: tens of milliseconds.
- **Stable wire shape.** Byte-locked against a golden fixture at
  `crates/evidence-mcp/tests/fixtures/golden_context_response.json`
  (regen via the existing `tools/regen-golden-fixtures.sh` pattern).

### 3.2 CLI verb — `cargo evidence context`

```bash
cargo evidence context crates/evidence-core/src/trace.rs       # file selector
cargo evidence context --crate evidence-mcp                    # crate selector
cargo evidence context --module evidence_core::trace --json    # module selector
cargo evidence context                                         # workspace overview
```

Output:

- Default — human table summarizing the requirements, tests, diagnostic
  codes, floors, and the nearest `CLAUDE.md` path.
- `--json` — the same JSON blob the MCP tool returns. Byte-locked to a
  golden fixture at
  `crates/cargo-evidence/tests/fixtures/golden_context.json`.

Exit codes (matches the rest of the CLI):

- `0` — `CONTEXT_OK`. Context resolved successfully. Also `0` for
  `CONTEXT_NO_TRACE_CONFIGURED` (non-adopter graceful path, mirroring
  `floors`'s "not configured" behavior).
- `1` — `CONTEXT_ERROR`. Runtime error (trace files unreadable, IO
  failure, parse failure).
- `2` — `CONTEXT_FAIL`. Selector invalid for the workspace
  (`CONTEXT_SELECTOR_OUT_OF_SCOPE`).

### 3.3 Per-crate `CLAUDE.md` in this repo

Each ≤60 lines, local conventions only, no re-statement of root rules.

- **`crates/evidence-core/CLAUDE.md`**
  - Library crate (no binary).
  - Module groups: `trace`, `hash`, `env`, `verify`, `policy`, `boundary_check`, `compliance`, `coverage`, `floors`, `rules`, `diagnostic`.
  - Convention: `thiserror` for errors; never `anyhow`. Unit tests live in `src/*/tests.rs`; integration in `tests/`.
  - Scoped test command: `cargo test -p evidence-core --all-targets`.

- **`crates/cargo-evidence/CLAUDE.md`**
  - Cargo subcommand binary; user-facing entry point.
  - CLI layout: `src/cli/<verb>/...`.
  - Agent-facing verb is `check`; humans get `verify`, `generate`, `diff`,
    `floors`, `rules`, `context`.
  - JSONL invariants: every `--format=jsonl` run emits exactly one
    terminal (`*_OK` / `*_FAIL` / `*_ERROR`) as the last stdout line; one
    JSON object per line, error prose on stderr only.
  - Scoped test command: `cargo test -p cargo-evidence --all-targets`.

- **`crates/evidence-mcp/CLAUDE.md`**
  - MCP wrapper over the CLI. Stateless per-request; one-shot
    version-skew probe at startup.
  - Subprocess pattern: `subprocess::run_evidence`; streaming verbs go
    through `Server::run_streaming_verb`.
  - Six tools today; `evidence_context` is the seventh.
  - Scoped test command: `cargo test -p evidence-mcp --all-targets`.

Root `CLAUDE.md` gets one new short paragraph: a pointer to
`cargo evidence context` and `evidence_context`.

### 3.4 Init scaffolding for downstream — `cargo evidence init --with-agent-context`

`init` (existing) emits `cert/boundary.toml`, `cert/profiles/*`,
`cert/trace/*.toml`.

New (opt-in) emissions:

- `CLAUDE.md` at workspace root — starter template (≤30 lines): one line
  per project rule the user wrote, plus a pointer paragraph to
  `cargo evidence context` / `evidence-mcp`.
- `.claude/settings.json` (or merged with existing) — registers
  `evidence-mcp` as an MCP server and adds a `permissions.deny` entry for
  the default `evidence/` output dir.

Flags:

- `--with-agent-context` — emit (the default).
- `--no-agent-context` — skip.
- Existing files are **never overwritten** (matches `init`'s current
  behavior); the command prints which files it skipped and the diff the
  user would need to apply by hand.

## 4. Architecture

### 4.1 Library layer (`evidence-core`)

New module `evidence_core::context`:

- `pub fn resolve_selector(workspace_root: &Path, raw: Option<&str>) -> Result<ResolvedSelector, ContextError>`
- `pub fn context_for(workspace_root: &Path, selector: &ResolvedSelector) -> Result<ContextReport, ContextError>`

Internally composes:

- `evidence_core::trace::read_all_trace_files` (existing).
- A new resolver that walks `crates/*/Cargo.toml` to map crate name ↔
  directory ↔ package name ↔ root module name.
- A new index: `BTreeMap<ModulePathPrefix, Vec<RequirementRef>>` keyed by
  prefix-match against each LLR's `modules` field.
- A floors-slice helper that filters `cert/floors.toml` to dimensions
  semantically scoped to the resolved crate (`test_count`,
  `library_panics`, etc.).

Errors are typed via `thiserror`:

```rust
#[derive(thiserror::Error, Debug)]
pub enum ContextError {
    #[error("selector {0:?} is outside the workspace")]
    SelectorOutOfScope(String),
    #[error("trace not configured at {0}")]
    TraceNotConfigured(PathBuf),
    #[error("trace read failed")]
    TraceRead(#[from] TraceError),
    /* ... */
}
```

Each variant carries the context for its message; no `From<anyhow::Error>`
back door.

### 4.2 CLI layer (`cargo-evidence`)

New module `cli::context`:

- Parses flags into a `ResolvedSelector`.
- Calls `evidence_core::context::context_for`.
- Renders human table or `--json` blob.
- Emits hand-built terminals: `CONTEXT_OK`, `CONTEXT_FAIL`,
  `CONTEXT_ERROR`. Registered in `TERMINAL_CODES` per the existing
  diagnostic-code locking.

### 4.3 MCP layer (`evidence-mcp`)

New tool method `evidence_context` on `Server`. Unlike the streaming verbs
(`check`, `doctor`, `floors`), this is a single-blob response — it follows
the `evidence_diff` shape:

```rust
#[tool(name = "evidence_context", description = "...")]
pub async fn evidence_context(
    &self,
    Parameters(req): Parameters<ContextRequest>,
) -> Result<Json<ContextToolResponse>, String> { ... }
```

Implementation: spawns `cargo evidence context <args> --json` via the
existing `subprocess::run_evidence`, deserializes the blob, prepends
workspace-fallback + version-skew warnings via the existing helpers. No
new transport machinery.

### 4.4 Init layer (`cargo-evidence`)

`cli::init` extended to (conditionally) write the new files. Templates
live as `const &str` in the crate, not on disk, so the binary is
self-contained.

## 5. Data flow — an agent session

```
1. Agent opens session in crates/evidence-mcp/.
   Harness walks up: loads crates/evidence-mcp/CLAUDE.md, then root CLAUDE.md.
2. Agent calls evidence_context({ selector: "<file or crate>" }).
3. Response: governing LLRs, test selectors, diagnostic codes, floors,
   boundary, nearest CLAUDE.md path.
4. Agent edits; knows precisely which `cargo test -p <crate> -- <selector>`
   to re-run.
5. Agent calls evidence_check(--mode=source) to validate.
6. On GAP, agent reads root_cause_uid, calls evidence_context on the
   owning LLR's modules[0] to re-orient.
```

## 6. Diagnostic codes (new)

Each must be (a) backed by `DiagnosticCode::code()`, and (b) claimed by at
least one LLR's `emits` list — per `diagnostic_codes_locked`.

| Code                                       | Severity | Layer    | Meaning |
|--------------------------------------------|----------|----------|---------|
| `CONTEXT_OK`                               | Info     | terminal | Selector resolved, response built. |
| `CONTEXT_FAIL`                             | Error    | terminal | Selector invalid or no resolution possible. |
| `CONTEXT_ERROR`                            | Error    | terminal | Runtime failure (trace files unreadable, etc.). |
| `CONTEXT_NO_REQUIREMENTS_FOR_SELECTOR`     | Warning  | content  | Selector resolved but matches zero requirements (signal: untraced module). |
| `CONTEXT_SELECTOR_OUT_OF_SCOPE`            | Error    | content  | Selector resolves outside the workspace (user-fixable typo). |
| `CONTEXT_NO_TRACE_CONFIGURED`              | Info     | content  | `cert/trace/` missing — non-adopter graceful path. |
| `CONTEXT_AMBIGUOUS_SELECTOR`               | Warning  | content  | Input matched multiple kinds; resolver picked the highest-priority one. |
| `CONTEXT_RUNTIME_ERROR`                    | Error    | content  | Tool-side I/O fault (unreadable `Cargo.toml`, `canonicalize` failure, underlying trace TOML read error). Distinct from `CONTEXT_SELECTOR_OUT_OF_SCOPE` so agents can tell user-typo from tool-fault. |

## 7. Tests

- **Unit (`evidence-core/src/context/tests.rs`):** selector classifier
  cases, LLR-by-`modules` lookup (exact + prefix), parent rollup, floors
  per-crate slicing, error variants. ≤80 lines per test fn.
- **Integration (`crates/cargo-evidence/tests/cli_context.rs`):** spawn
  `cargo evidence context <args> --json`; assert against golden
  `crates/cargo-evidence/tests/fixtures/golden_context.json`.
- **MCP integration (`crates/evidence-mcp/tests/context_roundtrip.rs`):**
  spawn the MCP binary, send `tools/call` for `evidence_context`, assert
  shape and skew-signal forwarding.
- **Trace gating:** `diagnostic_codes_locked` covers the new
  `CONTEXT_*` codes automatically once they're in `RULES` + claimed by
  LLRs.
- **Walker gating:** any new dir walks use
  `WalkDir::new(...).follow_links(false)` (`walker_usage_locked`).
- **CRLF gating:** golden fixtures live under `tests/fixtures/` with the
  existing `** binary` `.gitattributes` rule.

## 8. Compatibility

- **Determinism.** Response key order is fixed via `BTreeMap`-backed
  serialization; arrays are sorted by stable keys (`id` for
  requirements, `name` for tests). The golden fixture pins the order
  byte-for-byte.
- **Schema version.** New `ContextReport` is in
  `evidence_core::context`; not part of any existing on-disk schema, so
  no `schema_versions.rs` bump is required. If we choose to embed it
  into the bundle later, it gets its own schema and version.
- **Backwards compatibility.** Pure additions to the CLI surface, MCP
  surface, and `init`. No existing behavior changes.
- **MSRV.** No new toolchain requirements; uses only crates already in
  the workspace.

## 9. Risks & mitigations

| Risk | Mitigation |
|------|------------|
| `evidence_context` becomes a dumping ground for every agent-helpful field; response bloats. | Hard-cap the response schema in the spec (above). New fields require a new MCP tool method, not a wider response. |
| Per-crate `CLAUDE.md` duplicates root content over time. | Lint helper in PR 1: simple regex check that flags duplicated workspace-wide rules. Reviewer-enforced for now; promote to CI gate if drift returns. |
| Downstream `init` scaffolding writes over a user's existing `CLAUDE.md`. | `init` already refuses to overwrite; new files use the same guard. Skipped writes are logged. |
| Selector resolution ambiguity (file path also valid as crate name). | Fixed priority: file > crate > module. Surface `CONTEXT_AMBIGUOUS_SELECTOR` warning. |
| Trace TOML grows; lookups slow. | Index built once per call, not per request fan-out. Lookups are `BTreeMap` `range` queries. Re-measure if cumulative LLR count crosses 500. |
| MCP server CWD-fallback misroutes context. | Inherit the existing `MCP_WORKSPACE_FALLBACK` signal path; surfaced in `warnings`. |

## 10. Scope split — one issue per PR

| PR | Scope | Trace seed | Independently valuable |
|----|-------|------------|------------------------|
| **PR 1** | This spec + layered `CLAUDE.md` (root pointer + 3 per-crate) + new SYS/HLR/LLR/TEST chain | Yes — full chain seeded in commit 1 | Yes — improves agent context in this repo without new code |
| **PR 2** | `evidence_core::context` library + `cargo evidence context` CLI + golden fixture + tests | Implements LLRs from PR 1 | Yes — humans + non-MCP agents benefit |
| **PR 3** | `evidence_context` MCP tool + roundtrip tests | Implements MCP-tool LLR from PR 1 | Yes — MCP users get full benefit |
| **PR 4** | `cargo evidence init --with-agent-context` + `.claude/settings.json` snippet template + tests | Implements init-scaffold LLR from PR 1 | Yes — downstream users get scaffolding |

## 11. Explicitly out of scope

- Auto-generated per-module `CLAUDE.md`.
- Stop-hook recipe.
- Plugin packaging.
- LSP integration.
- Backfilling existing downstream projects.

## 12. Open questions to revisit during writing-plans

- Should `--with-agent-context` default to on or off in PR 4? Leaning
  **on** (the article argues for low-friction adoption), but the project's
  "no excessive .md documents" rule suggests caution. The user opted into
  `cargo evidence init` already, so emitting a single 30-line `CLAUDE.md`
  is consistent with that opt-in.
- Should the workspace overview (selector = null) embed a high-level
  crate map, or just list crate names + DAL? Leaning **embed**, capped at
  ~10 lines per crate to keep the response small.
- Should the response include git status (current branch, dirty bit)?
  Tempting (it's free via `GitSnapshot::capture`) but it's not strictly
  *context* — it's *state*. Leaning **no** for v1; add later if asked.
