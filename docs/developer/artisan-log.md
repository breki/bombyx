# Artisan Findings -- Deferred backlog

Quality (Artisan) review findings. Newest first.

---

### aq-2026-09-04-blocks-rebuilt-per-check

**Category:** Efficiency

`collect` in `xtask/src/canon.rs` calls `reference_targets`
over every canon file and then `unresolved_xrefs` over every
canon file, and each call rebuilds the paragraph blocks from
scratch, so each file is copied into `String`s twice.
`reference_targets` also walks the content twice on its own,
once for headings and once for blocks. Building the blocks
once per file in `collect` and passing `&[Block]` to both
would end that. `Block` is private while both checks are
`pub`, so the two would have to change visibility together.

Deferred: 23 small markdown files, so the cost is invisible
today.
