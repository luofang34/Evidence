# Evidence — Project Notes

## Rules

- **No excessive .md documents.** Do not create markdown files (changelog, compliance reviews, audit docs, etc.) unless explicitly asked to or strictly necessary for the project.
- README.md is the only mandatory markdown file.

## File-tree traversal

Use `walkdir::WalkDir` for any recursive `.rs` / `.md` / `.toml` walk. `walkdir` is already a workspace production dependency (used by `verify::bundle::cmd_verify_jsonl`, `hash::sha256_dir`, `floors::walk_rs_files`, and `trace::selector_check::collect_rs_files`); no incremental dep cost.

Hand-rolled `fs::read_dir` recursion is banned project-wide. The `walker_usage_locked` integration test fires on any file that calls `fs::read_dir` outside `ALLOWED_READ_DIR_FILES` — single-directory non-recursive uses must be allowlisted there with written justification beside the entry.

Convention for `WalkDir` callsites:
- Always pin `.follow_links(false)` explicitly. walkdir's default follows symlinks, which diverges from `fs::read_dir`'s implicit behavior.
- Prune subtrees with `.filter_entry(|e| !is_skipped(e))`, not post-filter. `filter_entry` runs before descent, so returning `false` for a directory prunes its subtree cheaply.
- Share the primitive (`WalkDir::new(root).follow_links(false).into_iter()`), not filter logic. A config-knobbed generic walker is usually more complex than the duplication it would replace.
