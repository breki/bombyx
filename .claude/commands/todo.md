---
description: Capture an issue or idea into the TODO list (no implementation)
allowed-tools: Bash(cargo xtask*), Read, Grep
---

Collect an issue or idea into `docs/todo.md`. This
command **only captures** -- it never implements. Use
`/implement` to act on a captured item.

The mechanical work -- slug-uniqueness check, bullet
placement, listing, wrapping to 80 columns -- lives in
`cargo xtask todo`, so this skill never hand-edits
`docs/todo.md`. The skill owns the *judgment*: turning
the user's words into a slug and a summary.

## Behaviour

- **With arguments** (e.g. `/todo search bar is slow`):
  add the text as a new pending item with a generated
  slug.
- **Without arguments** (just `/todo`): list the
  current pending items, then stop.

## Adding an item

1. **Generate a slug** from the user's text:
   - Lowercase, ASCII only, words joined by `-`.
   - Drop filler words (`a`, `the`, `to`, `for`,
     `is`, `of`, `in`, `on`, `and`, `or`).
   - 3-6 words, <= 50 chars total.
   - Should read as a topic, not a sentence:
     `search-bar-perf`, not
     `make-the-search-bar-faster`.

2. Add it mechanically:

   ```
   cargo xtask todo add --slug <slug> \
     --summary "<one-line summary, <= 80 chars>"
   ```

   Add `--body "<longer text>"` when the user gave
   more than a one-liner (kept verbatim, wrapped by the
   command). The command refuses a slug that already
   exists (pending or done); if it errors on a
   collision, append `-2` / `-3` to the slug and retry.
   Keep the user's wording -- do not paraphrase or
   expand.

3. Confirm: print the slug the command reported. Mention
   `/implement <slug>` as the next step. Do not start
   implementing.

## Listing pending items

When called with no arguments, run `cargo xtask todo
list` and show its output (each line is
`<slug> -- <summary>`). Nothing else.

## Rules

- Never hand-edit `docs/todo.md`; go through
  `cargo xtask todo`.
- Never edit the `## Done` section from this command
  (`todo done` is `/implement`'s finalise step).
- Never create files in `docs/issues/` from this
  command -- that is `/implement`'s job.
- Never run tests, builds, or git commands.
