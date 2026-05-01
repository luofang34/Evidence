# cargo-evidence self-trace

This directory dogfoods the tool's own traceability format against
the tool itself, across the full DO-178C §5.1 chain:

```
System Requirements  (sys.toml)    traces_to = []
High-Level Reqs      (hlr.toml)    traces_to SYS
Low-Level Reqs       (llr.toml)    traces_to HLR
Test Cases           (tests.toml)  traces_to LLR
```

Current counts per layer live in `cert/floors.toml`
(`trace_sys` / `trace_hlr` / `trace_llr` / `trace_test`) under
strict `floor == current` equality — editing a count here would
rot the moment the next ratchet-bump PR lands.

The CI job `trace-self-validate` runs
`cargo evidence trace --validate --require-hlr-sys-trace --require-hlr-surface-bijection --check-test-selectors`
on every push — default `--trace-roots` discovery picks up this
directory automatically. A validation failure blocks merge.

This is not the cert-profile dogfood (`cert/trace/`). That directory
exercises the bundle-generation pipeline against its own cert config;
this one pressure-tests **format expressivity** by applying the trace
format to the tool's source code.

## UUID policy

**UUIDs are machine-generated only.** The tool's own `trace
--backfill-uuids` is the single authoritative generator. Hand-crafting
UUIDs — even with valid v4 syntax — is banned:

1. It hides collision risk (two entries can share a hand-crafted UID
   and the check won't fire until one of them is deleted).
2. It weakens the "UUIDs are opaque" contract downstream tooling
   should be able to rely on.
3. It tempts readers to read meaning into the digits, which is the
   entire class of bug UUIDs were invented to prevent.

Workflow for adding a new entry:

1. Append the entry to `sys.toml` / `hlr.toml` / `llr.toml` /
   `tests.toml` **without** a `uid` field.
2. Run `cargo evidence trace --backfill-uuids` (no `--trace-roots`
   needed — discovery picks this directory).
3. Commit the populated TOML.

Re-runs are no-ops; the `trace-self-validate` CI job asserts
backfill reports "all entries already have UUIDs" to catch an
uncommitted backfill step before it reaches main.

## Mapping table

SYS groups cluster by load-bearing property:

| SYS     | Property                                        | HLRs allocated                     |
|---------|-------------------------------------------------|------------------------------------|
| SYS-001 | Verifiable evidence bundle                      | HLR-006, 008, 011, 012, 014        |
| SYS-002 | Machine-readable diagnostic stream              | HLR-001..005, 016, 017, 018, 020   |
| SYS-003 | Cross-host reproducibility for the same commit  | HLR-007, 009                       |
| SYS-004 | Policy-gated evidence emission                  | HLR-013, 019                       |
| SYS-005 | Refusal when integrity guarantees unmet         | HLR-010, 015                       |
| SYS-006 | Self-enforcement of the trace contract          | HLR-021, 022, 023, 024             |
| SYS-007 | Single agent-facing command reports pass/gap    | HLR-025, 026, 027, 028             |
| SYS-008 | Self-describing diagnostic vocabulary           | HLR-029..034                       |
| SYS-009 | Ratcheting floors — rigor only moves up         | HLR-035, 036, 037                  |
| SYS-010 | Trace layer supports genuine decomposition      | HLR-038, 039, 040                  |

Every HLR has at least one LLR; every LLR has at least one Test; every
Test points at a real `#[test] fn` via `test_selector` (enforced by
`--check-test-selectors`).

## Format-expressivity journal

This journal is the living deliverable of the self-trace. When the
format can't express something we need, the workaround **does not go
into the trace files** — it goes here as a ticket for a format
change. The journal's existence is the guarantee that we discover
format gaps before another project hits them.

### Journal entries

#### [2026-04 · PR #46 · open] `verify` vs `check` sibling commands

PR #46 introduces `cargo evidence check`, a higher-level entry point
that dispatches to `verify` under the hood in bundle mode. Both
commands can validate a bundle; they are not the same command. Rule
(documented in `--help` and `README.md`):

- **Agents and humans call `check`.** Auto-detects mode, emits
  `REQ_PASS` / `REQ_GAP` diagnostics keyed on requirement UIDs,
  plumbs `FixHint` variants for mechanical fixes.
- **CI scripts and debugging call `verify`.** Thin shell over
  `verify_bundle_with_key`. No argument-shape inference, no
  source-mode code paths. Predictable for bash pipelines.
- **MCP (PR #50) wraps `check`, not `verify`.** One agent verb, one
  MCP tool. Exposing both would let agents pick differently each
  release.

Deprecating `verify --format=jsonl` is out of scope for PR #46 —
tracked only if the sibling confusion proves real in practice.

#### [2026-04 · PR #44b · open] Backfill strips TOML comments

Observation: `cargo evidence trace --backfill-uuids` writes each file
back via `toml::to_string_pretty`, which does not preserve comments.
PR #44's hand-written top-of-file commentary and inter-entry group
headers were lost when the UID rotation ran. Minimal `#
cert/trace/<name>.toml — … (see ../README.md)` headers were
reinstated by hand; the richer commentary is now in `README.md`
only.

Workaround: treat README.md as canonical documentation, use brief
`# cert/trace/<name>.toml — …` file headers that survive backfill
because the stripper only rewrites `[[sections]]`. Accept the loss
of inter-entry group separators for now.

Decision: defer a comment-preserving serializer (toml_edit crate)
until the pain is proven. If someone re-runs backfill over this
directory and loses commentary a second time, open a focused PR
that swaps `toml::to_string_pretty` for `toml_edit::DocumentMut`.
~150 LOC, contained.

#### [2026-04 · PR #44b · closed] Test-selector resolution is now live

Follow-up to journal entry #1 below. `cargo evidence trace --validate
--check-test-selectors` greps every `test_selector` against the
workspace source (matching `fn <name>\s*\(` near a `#[test]`
attribute); unresolvable selectors produce a
`TRACE_SELECTOR_UNRESOLVED` finding. CI runs with the flag enabled, so
a renamed `#[test] fn` fires validation before the PR can merge.

Limitation: resolver is grep-level, not syn. A `#[test]` defined via
macro expansion won't be found. Accept and document; open a follow-up
if the case appears in practice.

#### [2026-04 · PR #44b · closed] SYS layer enforcement is now live

PR #44 landed the SYS layer structurally, but `HlrEntry.traces_to`
was optional — an HLR with empty `traces_to` validated cleanly. The
SYS layer was present in the data but advisory in the pipeline.

`TracePolicy.require_hlr_sys_trace` closes that gap. When set via the
new `--require-hlr-sys-trace` CLI flag, the validator emits a
Link-phase error for every HLR that doesn't trace up to a SYS UID.
The policy defaults to off so external `cert/trace/` projects stay
unaffected; the tool's own CI enables it.

#### [2026-04 · PR #44b · closed] UUIDs rotated, hand-crafting banned

The original `cert/trace/*.toml` files landed in PR #44 used
hand-authored deterministic UUIDs (`11…000001` per-layer-prefix
scheme). Pre-ship is cheap to rotate — PR #44b replaced every UID
with a real machine-generated v4 from `trace --backfill-uuids` and
locked in the "machine-generated only" policy above. Downstream
projects starting after this rotation never see the hand-crafted
scheme.

#### [2026-04 · PR #44 · open] Test-selector staleness is silent

*Superseded by the "Test-selector resolution is now live" entry
above; kept in the log so future readers can see the original
observation that motivated the follow-up.*

Observation: `TestEntry.test_selector` is a free-form string. A
refactor that renames the underlying `#[test] fn` leaves the UUID
link (`traces_to`) valid but the selector dangling — trace-validate
did not resolve selectors against the workspace source. Agents
reading a self-trace bundle couldn't tell the difference between a
live test pointer and a stale one.

#### [2026-04 · PR #49 · resolved] Trace schema grows decomposition + N:M + surfaces

Context: the trace layer pre-PR-#49 was rigid — HLRs couldn't
declare which user-visible surfaces they governed, TEST entries
could map to only one function, and `LlrEntry.derived` was dead
schema. Audit posture required the schema to express what the
prose already claimed.

Resolution: three additive schema extensions + one Link-phase rule.
`HlrEntry.surfaces: Vec<String>` declares claimed CLI verbs and
named observable contracts; a new `KNOWN_SURFACES` const in
`evidence_core::trace::surfaces` is the catalog; the
`require_hlr_surface_bijection` policy flag asserts both
directions (every surface claim lives in `KNOWN_SURFACES`; every
`KNOWN_SURFACES` entry is claimed by at least one HLR). Emits
`TRACE_HLR_SURFACE_{UNKNOWN,UNCLAIMED}`.
`TestEntry.test_selectors: Vec<String>` is additive alongside the
legacy `test_selector: Option<String>`; `all_selectors()` merges,
dedupes, and sorts. Single-selector entries migrate implicitly —
no rename required. `LlrEntry.derived = true` + non-empty
`rationale` is now an unconditional Link-phase rule (DO-178C
§5.2.2), emits `TRACE_DERIVED_MISSING_RATIONALE`.

CLI surface: `--require-hlr-surface-bijection` on `trace
--validate`; always-on inside `check`.

Ratchet: new floor `known_surfaces = 11` prevents silently
shrinking the surface catalog — doing so without an equal HLR
edit would relax the bijection check without firing.

#### [2026-04 · PR #48 · resolved] Ratcheting floors lock "rigor only goes up"

Context: before PR #48, the self-trace count could silently shrink
— removing an LLR or deleting a test wouldn't trip CI. The plan's
principle 2 (every rigor addition lands with a floor that only
moves up) was aspirational, not enforced.

Resolution: `cert/floors.toml` pins current measurements as
absolute floors across every ratcheted dimension (diagnostic
codes, terminal codes, per-layer trace counts, `#[test]` count,
library panics, known-surfaces catalog). `cargo evidence floors`
checks them on every CI run. Lowering a floor requires a
`Lower-Floor: <dimension> <reason>` line in the PR body,
enforced by `scripts/floors-lower-lint.sh`.

Raising a floor is a PR that edits the TOML. Future rigor-adding
PRs bump the relevant floor in the same commit; the ratchet moves
with the addition, not after.

#### [2026-04 · PR #47 · resolved] LLR.emits closes the code-to-requirement loop

Context: before PR #47, an LLR could claim behavior that the source
didn't implement, and the source could emit diagnostic codes that
no LLR claimed. The only link was prose, verifiable only by human
review.

Resolution: `LlrEntry.emits: Vec<String>` declares which diagnostic
codes each LLR owns. The locked-codes test asserts every code in
`evidence_core::RULES` is claimed by at least one LLR (minus an explicit
`RESERVED_UNCLAIMED_CODES` set — currently empty) and every
`emits` string is a real RULES code. Combined with the existing
LLR↔TEST link (TEST.traces_to) and the TEST↔`#[test] fn` link
(`--check-test-selectors`), every advertised code is now
mechanically traced through a requirement chain to a real function.

Convention: LLRs describing pure structure (schema shapes, config
loaders that wrap errors transparently) leave `emits = []`.
Emitter LLRs list every code they directly return.

#### (Future entries append above this line, newest first.)

## Ratchet ties

With `cert/floors.toml` (PR #48 landed), the self-trace's
per-layer minimums pin `trace_sys`, `trace_hlr`, `trace_llr`,
and `trace_test`. Removing an entry requires a PR that edits
the TOML and lowers the floor, with an explicit justification.
The intent is that self-trace coverage only grows.

Independent enforcement signals on the SYS contract (as of PR #44b):

1. `validate_trace_links_with_policy` emits a Link-phase error for
   empty HLR `traces_to` under `--require-hlr-sys-trace`.
2. `TEST-021` (integration test) asserts the above path fires.
3. `TEST-022` (integration test) asserts `--check-test-selectors`
   fires on a dangling selector.
4. `TEST-023` (integration test) asserts `cert/trace/` discovery
   works without `--trace-roots`.
5. `TEST-024` (`ci_self_check`) greps the committed `ci.yml` to
   assert both enforcement flags are wired on `trace-self-validate`.
6. (PR #47) `min_trace_entries` floor — backstop on coverage.
