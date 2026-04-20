# Evidence — Project Notes

## Rules

- **No excessive .md documents.** Do not create markdown files (changelog, compliance reviews, audit docs, etc.) unless explicitly asked to or strictly necessary for the project.
- README.md is the only mandatory markdown file.

## File-tree traversal

Use `walkdir::WalkDir` for any recursive `.rs` / `.md` / `.toml` walk. `walkdir` is already a workspace production dependency (used by `verify::bundle::cmd_verify_jsonl`, `hash::sha256_dir`, `floors::walk_rs_files`, and `trace::selector_check::collect_rs_files`); no incremental dep cost.

Hand-rolled `fs::read_dir` recursion is banned project-wide. The `walker_usage_locked` integration test fires on any file that calls `fs::read_dir` outside `ALLOWED_READ_DIR_FILES` — single-directory non-recursive uses must be allowlisted there with written justification beside the entry.

Convention for `WalkDir` callsites:

- **Always pin `.follow_links(false)` explicitly.** Three reasons:
  - *Soundness*: the tool produces certification bundles. A walker that follows symlinks can include out-of-tree content (e.g., a symlink to `/etc/passwd`) in the SHA256SUMS or integrity scan — audit signs off on files that aren't in the repo. `fs::read_dir`'s default-no-follow is the cert-correct behavior; walkdir's default-follow inverts it.
  - *Determinism*: symlink targets can differ across checkouts of the same git state, breaking `deterministic_hash` parity (SYS-003). The cross-host gate assumes same-git-state implies same-bundle.
  - *Loop safety*: symlink cycles (`a → b → a`) consume resources even with walkdir's loop detection.
  No production callsite today has a legitimate reason to follow symlinks; if one ever does, state *why* at the callsite and add a `walker_usage_locked` exemption rather than weakening the rule.
- **Prune subtrees with `.filter_entry(|e| !is_skipped(e))`**, not post-filter. `filter_entry` runs before descent, so returning `false` for a directory prunes its subtree cheaply.
- **Share the primitive, not filter logic.** A config-knobbed generic walker is usually more complex than the duplication it would replace (abstraction-wrong trap). `tests/walker_helpers.rs::walk` is the shared `WalkDir::new(root).follow_links(false).into_iter()` for integration tests; production code inlines the two-line call.

Mechanical enforcement: `walker_usage_locked` asserts (a) no unallowlisted `fs::read_dir` callsite anywhere in `crates/**/*.rs`, and (b) every `WalkDir::new(` call pins `.follow_links(false)` within the same call chain.
