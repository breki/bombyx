# todo-tooling-format-mismatch

**Status:** Done
**Captured:** 2026-08-09 (as `todo-summary-truncation`)
**Started:** 2026-08-10
**Completed:** 2026-08-10

## Problem

`cargo xtask todo` had two defects with one root cause: the
code that writes `docs/todo.md` and the code that reads it back
disagreed about the format.

**The reader ignored backticked entries.** `todo add` writes a
bullet as `- **slug** -- summary`. The four entries originally
typed by hand use backticks instead: `` - `slug` -- summary ``.
`parse_slug` only recognised the bold form, so:

- `todo list` silently omitted `wire-frosti`, `packer-box` and
  `agent-vlan` from the moment they were written.
- `todo done first-real-run` failed with "no pending todo with
  slug", for an item that was plainly sitting in the file.
- `slug_exists` could not see them either, so `add` would have
  happily created a duplicate of a hand-written slug.

The listing being *silently* short is what made this expensive.
It was reported as the complete queue several times before
anyone noticed it disagreed with the file.

**Done entries were truncated mid-sentence.** `add` wrapped a
long summary across lines using a two-space continuation
indent, and wrote the optional `--body` with *the same* indent.
Nothing distinguished "rest of the summary" from "first line of
the body", so reading a summary back could only ever take the
first line. The `drop-frontend-tooling` entry under `## Done`
read "Delete leftover frontend tooling from the".

## Context

- `xtask/src/todo.rs` -- `parse_slug`, `parse_section`,
  `slug_exists`, `add`, `move_to_done`.
- The file has three bullet spellings in use: backticked
  (hand-written), bold (`todo add`), and linked
  (`- [**slug**](issues/slug.md)`, written by `todo done`).
- `docs/todo.md` is read by `/todo` and `/implement`, so an
  incomplete listing feeds the wrong work queue to both.

## Decisions

- **2026-08-10 -- Accept all three spellings when reading, do
  not migrate the file.** Rewriting existing bullets to one
  spelling would be a bigger, riskier change than teaching the
  reader the format that was already in use. `add` keeps
  writing the bold form for new entries.
- **2026-08-10 -- Keep summaries on one line and enforce it at
  write time, rather than trying to rejoin wrapped ones when
  reading.** Rejoining cannot work: the summary continuation and
  the body's first line are byte-identical in structure, so no
  reader can tell them apart. Removing the wrap removes the
  ambiguity at its source, and turns the CLI's advisory
  "<= 80 chars recommended" into a checked contract. The cost is
  that an overlong summary is now an error; that is the right
  trade, because the alternative is silent truncation.

## Plan

1. Teach `parse_slug` the backticked form, keeping the ` -- `
   separator requirement so inline code in prose is not
   mistaken for a slug.
2. Add a pure `summary_line` helper that renders
   `<lead> <summary>` on one line and errors when it would
   exceed 80 columns, naming the remaining budget.
3. Use it in both `add` and `move_to_done` in place of
   `wrap_markdown`.
4. Normalise the summaries already wrapped in `docs/todo.md`.
5. Correct the `--summary` help text, which described the limit
   as a recommendation.

## Test strategy

Behaviour change in shipped code, so tests first. All pure
functions over fixture markdown, no I/O:

- A `MIXED` fixture holding both bullet spellings, asserting
  that `parse_section` returns all three slugs in order. This
  is the regression guard for the silent-omission defect.
- `parse_slug` accepts a backticked slug, and still rejects
  inline code in a prose bullet.
- `slug_exists` and `move_to_done` both find a backticked
  entry.
- `summary_line` keeps a short summary intact, refuses one that
  would wrap, names the budget and points at `--body`, and
  counts characters rather than bytes.

## Outcome

Nine tests added, all passing; `cargo xtask validate` green
with 100% coverage.

`cargo xtask todo list` now shows all nine pending entries with
complete summaries. Before this change it showed six, three of
them cut off mid-phrase.

Verified live that an overlong summary is refused before the
file is touched:

```
$ cargo xtask todo add --slug throwaway-check --summary "<159 chars>"
xtask error: summary is too long: 159 columns, limit 80. It must
fit on one line, so keep it to 55 characters after
'- **throwaway-check** --' and move the detail into --body.
```

Changed: `xtask/src/todo.rs:189` (`parse_slug`), `:217`
(`summary_line`), `:138` and `:302` (call sites), plus the
`--summary` help text and the module doc comment. Five
already-wrapped summaries in `docs/todo.md` were shortened.

### Follow-ups

- `move_to_done` always writes the Done link as
  `issues/<slug>.md` whether or not that file exists. Items
  completed outside `/implement` therefore get a dead link. Not
  fixed here; this document exists partly so that this item's
  own link resolves.
