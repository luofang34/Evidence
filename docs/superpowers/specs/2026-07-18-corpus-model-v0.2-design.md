# Corpus model v0.2 — design decisions

Status: accepted. Program tracking: the epic at
<https://github.com/luofang34/Evidence/issues/157>; milestones `v0.2-M1` … `v0.2-M7`.

v0.2 makes a single typed corpus graph the source of truth for certification
data: frozen source documents → committed source graph → reviewed
requirements → SYS/HLR/LLR → code/test/result, with every traceability
report a derived view. The direction was validated by a downstream pilot
(onboarding CCSDS 355.0-B-2 / 355.1-B-1) that had to hand-roll a document
registry, requirement↔text excerpts, extraction provenance, review batches,
ambiguities, decisions, and PICS tables — none of which the trace model
could load or validate — while smuggling lifecycle state through the
free-text `category` field.

Settled direction: delete-last convergence (legacy trace generalizes into
the graph; the legacy loader is removed only at cutover behind a parity
gate); requirement lifecycle lands once, on the corpus store; the work is
v0.2 of this repo, in place; ambiguities and decisions are first-class
graph nodes.

## DD-1 — Single corpus model; traceability is a derived view

One typed, uid-keyed graph is the source of truth. Matrix, floors,
bijections, coverage/gap reports, and context queries are computed views
over it. The end state has no compatibility projection and no dual-write.

*Rejected:* keeping `cert/trace` as a peer model with synchronization —
that recreates the excerpt/rationale duplication the pilot suffered.

## DD-2 — Convergence path: generalize, then cut over (delete-last)

The four-file trace is already a corpus subset (uid nodes, typed edges).
M1 loads it into the graph unchanged; M6 migrates this repo's own
`cert/trace` into the corpus layout and deletes the legacy loader in the
same PR as the parity guardrail. Parity gate: floor values preserved
exactly; all bijection lock-tests pass; trace-matrix and golden fixtures
identical (or intentionally regenerated with review); `check` / `context`
outputs equivalent.

*Rejected:* delete-first big-bang — it leaves main red for the whole
rebuild, zeroes the ratchet floors, and removes the place where the
rewrite's own trace chain must live.

## DD-3 — Store: `cert/corpus.toml` index over linked TOML files

`corpus.toml` holds `schema_version` plus per-kind path lists (sources,
source-graphs, requirements, ambiguities, decisions, profiles, reviews,
tests). File layout is non-semantic; the loader unions all indexed files
into one graph. Schemas are strict (unknown fields rejected) with per-file
schema versions and the floors policy: an older tool refuses a newer
schema rather than skipping fields. Target layout after cutover:

```
cert/
  corpus.toml
  sources.lock
  sources/            source-graph/        requirements/{source,sys,hlr,llr}/
  ambiguities/        decisions/           profiles/
  pics/               reviews/             tests/
```

`results/` is deliberately absent: run results are bundle-scoped graph
overlays (DD-15), not committed baseline files.

## DD-4 — Node identity and uid scheme

`uid` is permanent identity; the human `id` is unique per kind and
renameable. Corpus-native node kinds use typed-prefix uids (`src_`,
`req_`, `rev_`, `amb_`, `dec_`, `prof_`) over a UUIDv4 core so
cross-references self-document and edge type-checks are cheap. Legacy
bare-UUID uids are accepted by the legacy `cert/trace` adapter.
Source-locator identity hierarchy: explicit spec ID/numbering → section
path + local ordinal → content hash/structural fingerprint → page/DOM/line
positions, which are diagnostic only, never identity.

## DD-5 — Edges are typed and live on the owning node

`derives_from` (requirement→requirement), `quotes` (requirement→source
node + span), `verifies` (test→requirement), `reviews`
(review→requirement@digest), `resolves` (decision→ambiguity), `concerns`
(ambiguity→source node or requirement), `supersedes` (document revision
chains). Edges stay embedded in the owning node's record — one owner per
edge, clean diffs.

*Rejected:* a separate edge file (merge-conflict magnet with no owner).

## DD-6 — Source freezing

A baselined source is never just a URL. Registry entry: id, media type,
canonical location, retrieval time, sha256, capture mode. Capture modes:
`vendored` (raw bytes kept; high-assurance default), `hash-only` (digest +
location; redistribution-restricted documents), `external-controlled`
(immutable ID in an organizational document system). `sources.lock` pins
the resolved digest set. Content change at the same location produces a
new document revision node linked by `supersedes`; silent update is a
validation error. Unobtainable transitive references are representable as
registry entries with an availability status — trackable and lintable.

## DD-7 — Ingestion contract

Ingesters are reproducible given (frozen bytes, pinned tool + version) —
not deterministic-forever; extractor identity and version are recorded on
the source record. The committed source graph is the reviewed artifact;
re-ingestion is a drift lint, not the source of truth: a deterministic
read-only comparison (M4.5) reconciles the candidate parser graph to
committed identities and reports recipe, input, node, patch, review,
and effective-plane drift in closed sorted categories without ever
overwriting the baseline; surface presentation and any
human-authorized baseline write belong to M7. Where a parser
fails structurally (PDF tables, notably PICS), a reviewed `curated` patch
layer corrects parser output; patches are first-class records with the
same review lifecycle. A patch is data in the committed corpus with a
permanent `patch_<UUIDv4>` identity and a per-kind-unique human id; it
binds exactly one source-revision uid, the exact ingester recipe digest,
the exact verified input digest, and the exact pre-patch canonical graph
digest, and carries a reviewed-content digest over its canonical intent —
the ordered operations and all preconditions — with author, rationale,
and creation metadata outside semantic identity. The operation language
is a closed enum (replace canonical text or label, reclassify, reparent
or reorder, insert a fully specified node, remove with an explicit child
disposition), never generic JSON Patch; every operation carries explicit
preconditions that fail closed when stale, and application is atomic and
re-validates the whole graph. Parser output, patch records, and candidate
application results stay separately inspectable planes; a patch never
mutates the frozen source record or `sources.lock`. The review lifecycle
hands off here: only approved patches may contribute to an effective
committed graph (the approval-gated effective graph is the M4.4 review
generalization, not the patch milestone). PDF extraction delegates to a
pinned external extractor (pure-Rust PDF text extraction is not adequate
for CCSDS layouts); this is an ingest-time-only dependency — downstream
consumers read the committed graph. Ingester delivery order: Markdown →
HTML → PDF.

## DD-8 — Source graph schema

`SourceNode { uid, document, parent, kind, ordinal, label, text,
content_sha256, locator }` with kinds Section, Paragraph, ListItem,
DefinitionTerm, DefinitionBody, Table, TableRow, TableCell, CodeBlock,
Note, FigureCaption. Locators are format-specific (PDF
page/section/paragraph/bounding-box; HTML canonical URL/fragment/heading
path/DOM path; Markdown path/git blob/anchor/heading path/byte range).
Text normalization at ingest: Unicode NFC, whitespace-run folding, trim;
no automatic dehyphenation (a curated patch fixes real hyphenation
damage). `content_sha256` is computed over normalized text; requirement
quote spans index into normalized node text, so quote digests are stable
by construction.

## DD-9 — HTML and Markdown are structure-preserving

HTML ingestion keeps the h1–h6 tree, paragraphs and nested lists,
`<dl>/<dt>/<dd>` definition structure, table row/column relationships,
`<code>/<pre>` literals, ids/anchors/internal links, and
note/example/figure classification; it drops navigation, ToC duplicates,
footers, scripts, and styles. Markdown parses via a CommonMark/GFM AST
(headings, lists, tables, blockquotes/admonitions, fenced code, footnotes,
explicit heading IDs); local files pin the git blob SHA, remote ones pin
content SHA-256 + final URL + retrieval metadata. The driving case is
OIDC Core: an RFC 2119 notation section, monospace-means-literal, and
Terminology definitions that are normative without any capitalized
keyword — unreachable from a text dump or a modal-verb regex.

PDF ingestion (M4.6) is layout-aware but honest about loss: a
pinned, offline Poppler `pdftotext -bbox-layout` adapter (strict
tool lock, explicit executable path, bounded isolated execution)
projects page/block/line/word geometry into candidate nodes under
committed recipe rules (headers, footers, columns, numbered
headings, notes, captions), preserves physical page, optional
printed label, and bounding box as diagnostic locators, and never
dehyphenates. Table rows/cells are never inferred: unprovable
structure reports deterministic structural-loss diagnostics and
recovers only through approved curated patches. The raw
extractor-output digest is an output-identity component with its
own drift category.

## DD-10 — Requirement records

`layer = source | sys | hlr | llr`. A requirement may cite multiple
source bindings, each `document + node + span + quoted_text_sha256`; one
source node may yield many atomic requirements, but each must point at
its precise node/span, never a whole chapter. Canonical modality enum:
`required | prohibited | recommended | not_recommended | optional |
permitted | normative_definition | informative`; per-document conventions
map the spec's vocabulary (RFC 2119, CCSDS shall/should/may/permits) onto
it. `[semantics]` behavior tags are optional and non-gating initially.
`[extraction]` metadata (agent, prompt digest, tool version) is mandatory
on machine-proposed candidates.

## DD-11 — Lifecycle and review

States: `candidate → approved | rejected`, plus derived `stale`. Approval
is a separate review record binding `(typed target,
reviewed_content_sha256, decision, reviewer, reviewed_at)`; any content
change makes the target stale because the digest no longer matches. The
target is a closed set — `requirement` or `curated_patch`; the kind owns
the reviewed-content projection the digest covers, and a patch
contributes to its revision's effective structural graph only while
currently approved. Agents may create and update candidates only, through
an append-only proposal path; approval, source-snapshot mutation, and
baseline overwrite are human-only. Enforcement: in strict profiles,
implementation artifacts (LLR modules, code, tests) may only trace into
approved requirements.

## DD-12 — Conventions baseline precedes extraction

Before body extraction, per-document records must be extracted and
approved: `requirements_notation` (vocabulary, case sensitivity) and a
normativity map (default classification, informative sections, per-node
overrides with reasons). Processing order: freeze → source graph →
conventions review → normativity review → batched candidate extraction →
atomicity/completeness/conflict lints → human approval → SYS/HLR/LLR
derivation. The modal-verb scan is a completeness lint, never the
extraction algorithm.

## DD-13 — Ambiguities and decisions are first-class nodes

Ambiguity: id, status, severity, `concerns` edges, question, required
resolution. Decision: id, status, statement, rationale, source, optional
`resolves` edges. Validation: an open blocking ambiguity prevents approval
of requirements whose cited nodes it concerns; release-grade profiles gate
on zero open high-severity ambiguities. Decisions get the same
digest-bound review treatment as requirements.

## DD-14 — Profiles and PICS

The applicability/profile filter ships with extraction (M5) — honest gap
reports for a profile-scoped implementation need it early. Full PICS form
modeling and rendering ship with the surfaces milestone. PICS items link
to the requirements they claim; a claim status cannot be asserted while
linked requirements are unapproved or ambiguity-blocked.

## DD-15 — Derived views, floors, bundles

All reports (matrix, coverage, gap, context) are graph queries. Floors
gains corpus dimensions after cutover (frozen source count, approved
source-requirements per document; open high-severity ambiguities as a
ceiling for release profiles) while `trace_*` dimensions carry over with
values preserved exactly. Corpus artifacts (graph files, requirements,
reviews, `sources.lock`) enter the bundle's `SHA256SUMS`/verify surface;
vendored source bytes are referenced by hash from `sources.lock`, never
copied into bundles. Run results remain bundle-scoped overlays — the
committed baseline stays pure intent; bundles remain the evidence of
execution.

## DD-16 — Validation is phase-aware and incremental

A corpus declares its phase per document or workstream (`frozen`,
`graphed`, `conventions-approved`, `extracting`, `baselined`); validators
key on the declared phase so "not yet extracted" is distinguishable from
"invalid" — and phase claims are themselves checked (a `baselined`
document with zero approved requirements is an error). Per-document
content-hash short-circuiting keeps validation fast on large graphs.

## DD-17 — CLI and MCP surfaces

CLI verb family: `cargo evidence corpus source add|inspect`, `corpus
ingest`, `corpus validate`, `corpus queue`, `corpus context`, `corpus
review`, `corpus render`. MCP: read-only `evidence_corpus_validate`,
`_source_context`, `_extraction_queue`, `_requirement_context`, `_trace`,
plus the restricted candidate-proposal API. Every new verb registers in
`KNOWN_SURFACES` with a matching HLR, and the JSONL invariants (single
terminal, stdout-strict) apply unchanged.

## DD-18 — Code placement and repo discipline

Corpus code lives in `evidence-core` under `corpus.rs` + `corpus/`
(domain-named submodules, standard file-size limits, no `mod.rs`).
Existing conventions bind throughout: trace-first chain seeding per PR,
one issue per PR, fix + guardrail in the same PR, `walkdir` with
`follow_links(false)`, thiserror-typed errors, no panics in library code.

## Milestone map

| Milestone | Scope anchor |
|---|---|
| v0.2-M1 Corpus graph core | DD-1..5, DD-18 |
| v0.2-M2 Lifecycle & review | DD-11 |
| v0.2-M3 Source layer | DD-6 |
| v0.2-M4 Ingesters | DD-7..9 |
| v0.2-M5 Extraction & corpus semantics | DD-10, DD-12..14 |
| v0.2-M6 Cutover | DD-2 parity gate; legacy loader deleted |
| v0.2-M7 Surfaces | DD-17, PICS forms |

## Acceptance fixtures

- OIDC Core HTML: the Requirements Notation section; a Terminology
  definition that is normative without capitalized keywords; ID Token
  claims (one node → several atomic requirements with distinct spans).
- An equivalent small Markdown spec (heading IDs, tables, fenced code).
- A CCSDS PDF fragment with document page labels and paragraph numbering.
- A CCSDS PICS table — the known parser-hostile case; exercises the
  curated-patch path end to end.

## Non-goals for v0.2

- No requirement NLP or semantic deduplication beyond the declared lints.
- No live-fetch of sources at validate time (frozen bytes or recorded
  digests only).
- No agent-side approval authority anywhere.
