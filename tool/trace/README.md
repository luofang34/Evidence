# cargo-evidence self-trace

This directory dogfoods the tool's own traceability format against
the tool itself, across the full DO-178C §5.1 chain:

```
System Requirements  (sys.toml)   ─▶ 5 entries
High-Level Reqs      (hlr.toml)   ─▶ 20 entries  traces_to SYS
Low-Level Reqs       (llr.toml)   ─▶ 20 entries  traces_to HLR
Test Cases           (tests.toml) ─▶ 20 entries  traces_to LLR
```

The CI job `trace-self-validate` runs
`cargo evidence trace --validate --trace-roots tool/trace` on every
push. A validation failure blocks merge.

This is not the cert-profile dogfood (`cert/trace/`). That directory
exercises the bundle-generation pipeline against its own cert config;
this one pressure-tests **format expressivity** by applying the trace
format to the tool's source code.

## Mapping table

SYS groups cluster by load-bearing property:

| SYS    | Property                                        | HLRs allocated |
|--------|-------------------------------------------------|----------------|
| SYS-001 | Verifiable evidence bundle                     | HLR-006, 008, 011, 012, 014 |
| SYS-002 | Machine-readable diagnostic stream             | HLR-001..005, 016, 017, 018, 020 |
| SYS-003 | Cross-host reproducibility for the same commit | HLR-007, 009   |
| SYS-004 | Policy-gated evidence emission                 | HLR-013, 019   |
| SYS-005 | Refusal when integrity guarantees unmet        | HLR-010, 015   |

Every HLR has at least one LLR; every LLR has at least one Test; every
Test points at a real `#[test] fn` via `test_selector`.

## Format-expressivity journal

This journal is the living deliverable of the self-trace. When the
format can't express something we need, the workaround **does not go
into the trace files** — it goes here as a ticket for a format
change. The journal's existence is the guarantee that we discover
format gaps before another project hits them.

### Journal entries

#### [2026-04 · PR #44 · open] Test-selector staleness is silent

Observation: `TestEntry.test_selector` is a free-form string. A
refactor that renames the underlying `#[test] fn` leaves the UUID
link (`traces_to`) valid but the selector dangling — trace-validate
currently does not resolve selectors against the workspace source.
Agents reading a self-trace bundle can't tell the difference between
a live test pointer and a stale one.

Decision: defer resolution to a follow-up. `cargo evidence trace
--validate` will grow an opt-in `--check-test-selectors` flag in
PR #44b that greps for `fn <name>\s*\(` under an ancestor
`#[test]` attribute and fails on unresolvable selectors. Lands
before PR #49 (MCP) so the wire contract never ships an
unvalidated selector shape.

Workaround in the interim: review selectors during code review when
renaming tests; re-run `cargo test --no-run` locally to confirm the
selector resolves.

#### (Future entries append above this line, newest first.)

## One-time UUID backfill workflow

If a new entry is added without a `uid`, run:

```
cargo evidence trace --backfill-uuids --trace-roots tool/trace
```

The tool assigns fresh UUIDv4s in-place and writes the file back.
Commit the populated TOML. Re-runs are no-ops.

## Ratchet ties

Once `cert/floors.toml` lands (PR #47), the self-trace's minimum
entry count will be pinned as `min_trace_entries = 65` (5 + 20 + 20
+ 20). Removing an entry then requires a PR that both edits the TOML
and lowers the floor, with an explicit justification. The intent is
that self-trace coverage only grows.
