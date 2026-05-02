# Contributing to cargo-evidence

This project ships software-of-interest (SoI) for DO-178C / DO-330
certification evidence. Every change has to clear the same gates a
downstream cert program would expect: trace coverage, ratchet floors,
deterministic bundles. The PR loop below mechanizes those gates.

If anything below is wrong or unclear, open an issue or PR — the
guidance evolves with the toolchain.

---

## PR loop

A landed PR carries:

1. **A trace-seed commit** (when adding a behavior an auditor would
   review): SYS / HLR / LLR / TEST entries under `cert/trace/`,
   added in the first commit on the branch, before any
   implementation. The trace chain is the contract; the code
   implements it. See [Trace-first convention](#trace-first-convention)
   below.
2. **An implementation commit** that satisfies the seed.
3. **A guardrail in the same PR** — a test, lint, or CI check that
   prevents this same regression from recurring. A fix without a
   guardrail is temporary.

Both commits must be locally CI-clean before pushing. The PR landing
that's "almost green" tomorrow gets reverted today.

## Local CI

The minimum command set, mirroring `.github/workflows/ci.yml`:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
RUSTDOCFLAGS='-D missing_docs -D rustdoc::broken_intra_doc_links' \
    cargo doc --workspace --no-deps
cargo build --release --workspace
cargo run -p cargo-evidence -- evidence trace --validate
cargo run -p cargo-evidence -- evidence floors --format=jsonl
```

The trace + floors gates are project-internal — they catch most
self-cert regressions before they reach CI. Don't skip them.

## Trace-first convention

Default: every PR seeds its SYS/HLR/LLR/TEST entries in the first
commit on the branch, before any implementation. UUIDs are **never
hand-crafted** (even with valid v4 syntax) and **never generated
externally** (e.g., a one-liner Python script): the tool's own
`cargo evidence trace --backfill-uuids` is the single authoritative
generator. The full rationale lives in `cert/trace/README.md`'s
"UUID policy" section.

Workflow for a new entry:

1. Append the entry to the appropriate trace file
   (`cert/trace/sys.toml` / `hlr.toml` / `llr.toml` / `tests.toml`)
   **without** a `uid` field. Set `traces_to` to point at the
   parent layer's UID — those already exist in the file.
2. Run `cargo evidence trace --backfill-uuids` from the workspace
   root. Discovery picks `cert/trace/` automatically; no
   `--trace-roots` flag needed.
3. Commit the populated TOML.

Re-runs are no-ops; the `trace-self-validate` CI job asserts
backfill reports "all entries already have UUIDs", catching an
uncommitted backfill step before it reaches main.

Each trace seed bumps the matching counter in `cert/floors.toml`
(`trace_sys`, `trace_hlr`, `trace_llr`, `trace_test`). After the
implementation commit lands its tests, also bump the affected
`per_crate.<crate>.test_count` row to match the new measurement.
The `floors_equal_current_no_slack` test enforces equality, not
inequality — drift either way fails CI.

Exception — bidirectional contracts spanning two PRs: when a single
SYS-level claim covers both directions of a contract (e.g., forward-
enrichment in one PR + reverse-verification in a follow-up), the
*second* PR seeds the chain for both halves. The first PR ships
under an implicit trace obligation; the second PR's chain-seed
discharges it for both. This is rare — only legitimate when the two
halves form one logical deliverable and splitting the chain would
force referencing UUIDs that don't yet exist.

## Floors are ratchet-only

`cert/floors.toml` is a one-way gate: `current >= committed_floor`
on every dimension. The `current_measurements_satisfy_committed_floors`
test enforces this; the companion `floors_equal_current_no_slack`
test enforces `current == committed_floor` (no slack — a slack
floor lets a later PR delete things along that dimension without
firing the gate).

Lowering a floor requires either:

- Rare and rejustified: a `Lower-Floor: <dimension> <reason>` line
  in the PR body or commit message, OR
- A schema break: bump `schema_version` in the file header.

Default expectation: floors only go up.

## What lands together, what stays separate

- **One issue per PR.** Break large refactors into independently
  revertible steps. A PR that fixes two unrelated bugs is harder to
  review and harder to revert.
- **No mixed reformatting.** `cargo fmt` results land as their own
  commit, never co-mingled with logic changes.
- **No silent dependency adds.** Each new workspace dep gets a
  one-line note in the commit body (why it's needed, what it
  costs).
- **No unsafe.** `unsafe_code` is `forbid`-level workspace-wide.

## Style snapshots

The full style guide lives in `CLAUDE.md` at the repo root. The
high-impact rules:

- **Max 500 lines per `.rs` file.** Locked tests count too — split
  to a sibling module before the limit, not after.
- **No `mod.rs`.** Use `foo.rs` + `foo/` directory pattern.
- **No `eprintln!` / `println!` for diagnostics** — use `tracing`
  (`info`, `warn`, `error`, `debug`).
- **No `unwrap` / `expect` / `panic!` in library code.** Tests
  may opt out via `#[allow(clippy::expect_used, clippy::panic)]`.
- **WHY-only comments.** No PR-number breadcrumbs, no absolute
  line counts, no temporal phrasing (`migrated from`, `previously`).
  These are mechanically enforced by `rot_prone_markers_locked`.
- **No editor-duplicate filenames** (`* 2.rs`, `* 2.toml`, …).
  Mechanically enforced by `editor_duplicates_locked`.

## Reporting

Open an issue at <https://github.com/luofang34/Evidence/issues>.
Security-relevant findings: please prefer a private channel
first — the project has no formal security disclosure policy yet
but will respond to good-faith reports.
