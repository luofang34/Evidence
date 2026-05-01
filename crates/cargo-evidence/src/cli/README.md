# `cli/` — taxonomy of cargo-evidence subcommands

Every `cargo evidence <verb>` lives here as one module. This README
is the map: where to look when triaging a verb, where to add a new
one, and which lifecycle phase each piece belongs to.

If a verb's home is unobvious, the answer is "the lifecycle phase it
operates on." That phase determines the directory.

---

## Top-level shape

```
cli/
├── args.rs        # clap derive types: CargoCli, EvidenceArgs, Commands
├── output.rs      # OutputFormat resolution + JSONL emit primitives
│
├── generate.rs    # `cargo evidence generate` (also the implicit default)
├── generate/      # phase pipeline behind generate
│
├── verify.rs      # `cargo evidence verify <bundle>`
├── verify/        # bundle-load + integrity-check helpers
│
├── check.rs       # `cargo evidence check` — agent-facing pass/gap
├── diff.rs        # `cargo evidence diff <bundle_a> <bundle_b>`
├── doctor.rs      # `cargo evidence doctor` — workspace audit
├── doctor/        # per-check helpers (checks.rs, qualification.rs)
├── floors.rs      # `cargo evidence floors` — ratchet-gate query
├── init.rs        # `cargo evidence init`
├── rules.rs       # `cargo evidence rules` — diagnostic-code manifest
├── schema.rs      # `cargo evidence schema {show, validate}`
└── trace.rs       # `cargo evidence trace --validate ...`
```

## Lifecycle taxonomy

Each verb belongs to exactly one phase of the project lifecycle.
Adding a verb? Pick the phase first; the file goes in the matching
group.

### Bundle-producing (writes to `out_dir`)

- **`generate`** — assemble a fresh evidence bundle from the current
  workspace state. The default verb when no subcommand is given.
  The `generate/` subdirectory holds the phase pipeline (`phases.rs`,
  `coverage_phase/`, `envelope.rs`, `policy.rs`, `test_outcomes.rs`)
  — extracted from `generate.rs` to keep that file under the
  workspace 500-line limit.

### Bundle-consuming (reads an on-disk bundle)

- **`verify`** — run the integrity + policy gates against a finished
  bundle. The `verify/` subdirectory holds the load-and-check
  helpers (`incomplete_bundle.rs`, `skipped_notices.rs`,
  `terminals.rs`).
- **`diff`** — compare two bundles structurally. Pure inspection,
  reports differences without judging them; exit code stays `0`
  even when bundles differ.

### Source-tree inspection (no bundle, no `out_dir` write)

- **`check`** — one-shot pass/gap validation. Agent-facing wrapper
  that's `auto`-mode in source mode and bundle mode behind one
  verb. The MCP server's `evidence_check` is a thin wrapper over
  this.
- **`doctor`** — audit a workspace's rigor adoption (boundary,
  trace, floors, CI integration, merge-style policy, override
  docs). The `doctor/` subdirectory holds the per-check
  implementations (`checks.rs`, `qualification.rs`).
- **`floors`** — query the ratcheting-floors gate state.
- **`trace`** — validate the SYS/HLR/LLR/TEST chain in
  `cert/trace/`. Also offers `--backfill-uuids` and a few diagnostic
  switches.

### Self-describing / scaffolding

- **`init`** — bootstrap `cert/boundary.toml` + `cert/floors.toml`
  + `cert/trace/` for a project that hasn't adopted the tool yet.
- **`rules`** — emit the manifest of every diagnostic code the tool
  can produce. Used by agents pinning autofix flows.
- **`schema`** — `show` / `validate` for the JSON schemas under
  `schemas/`. Useful when an integrator wants to generate types
  from the wire format.

## Conventions inside each verb module

- **`cmd_<verb>(...)` is the canonical entry point.** Called from
  `dispatch()` in `main.rs`, returns `anyhow::Result<i32>` (the
  process exit code). All argument parsing happens in clap before
  the call; `cmd_<verb>` receives typed arguments.
- **Output goes through `cli::output`.** Direct `println!` /
  `eprintln!` is reserved for the binary entry-point banner
  (`main.rs`'s `--help` intercept and the top-level error sink).
  Everything else uses `emit_jsonl`, `emit_json`, or the human-
  formatter.
- **Profile / DAL gates live in the verb's policy submodule.** For
  `generate`, that's `generate/policy.rs`. For `doctor`, it's
  `doctor/qualification.rs`. The pattern: the verb module orchestrates,
  the policy submodule holds the dev-vs-cert-vs-record decisions.

## Adding a new verb

1. Pick the lifecycle phase. The verb's home directory follows.
2. Add a `Commands` variant in `args.rs` with `#[command(about = ...)]`.
3. Add a `cmd_<verb>(...)` function in the new module.
4. Wire `dispatch()` in `main.rs` to call it.
5. Update this README's lifecycle taxonomy with the new entry.
6. Update the `EXPECTED_SUBCOMMANDS` list in
   `tests/help_listing.rs` to include the new verb (the locked
   test fires otherwise).
7. Trace seed: SYS / HLR / LLR / TEST entries under `cert/trace/`.
8. Floors bump for `trace_*` and `per_crate.cargo-evidence.test_count`.
