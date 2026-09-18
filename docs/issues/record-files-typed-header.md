# record-files-typed-header

**Status:** In progress -- increments 1-3 done; 4 mostly done
(fresh-reader trimmed, template-feedback stale claims fixed,
`## Resolved` cascade removed). The template-feedback broad
rationale trim is now decided against (2026-09-18): the file is
kept full so rustbase can adopt the improvements -- see
`documentation-overhaul.md` item 3.
**Captured:** (see `docs/todo.md`)
**Started:** 2026-09-17

Prerequisite for move 4 of the documentation-overhaul program
(`docs/issues/documentation-overhaul.md`). Started at the operator's
direction as its own code project; it may span several sessions.

## Problem

Five "record-collection" files carry many small entries and are
mutated by tooling, but their per-entry metadata (status, id, dates,
cross-references) lives in prose. The tooling parses that prose, which
drifts, and status is encoded as *which section* an entry sits in
(`## Pending` vs `## Done`), so `todo done` splices a block between
sections instead of flipping a field.

Give each entry a small, strict, machine-parseable header so the
tooling reads fields, not prose, and one integrity gate can validate
every cross-reference through a shared `id:` field.

## Scope

In: `docs/todo.md`, `docs/developer/redteam-log.md`,
`docs/developer/artisan-log.md`, `docs/developer/fresh-reader-log.md`,
`docs/developer/template-feedback.md`. Out: `CHANGELOG.md` (external
Keep-a-Changelog convention) and `backfeed-ledger.toml` (already TOML).

## Context

- `xtask/src/todo.rs` (1,454 lines, ~40 tests) is section-based:
  `add` appends under `## Pending`; `done` moves a bullet to the top
  of `## Done`; `list` reads one section. Entry shape is
  `- **slug** -- summary` with wrapped continuation/body lines.
- `feedback-add` (in xtask) maintains `template-feedback.md`
  (`tf-<date>-<slug>` ids, three sections).
- ID schemes: todo kebab-case slugs; redteam `RT-<n>`; artisan
  `AQ-<n>`; fresh-reader `FR-<n>`; template-feedback
  `tf-<date>-<slug>`. (Format map in progress.)

## Decisions (2026-09-17)

The format map (an Explore run) surfaced the real shape, and two
operator decisions reshaped the project:

- **Files hold LIVE work only.** Completed/closed/resolved entries are
  dropped -- git history, the CHANGELOG, and commit messages already
  record what shipped. This merges the project with move 4's "collapse
  the backlogs." Consequences: `todo.md` loses `## Done`; the reviewer
  logs keep only open (deferred) findings; `template-feedback.md`'s
  `## Resolved` (already `_None yet._`) goes. `fresh-reader-log.md`'s
  **Explanations to keep** stays -- those are do-not-trim markers, not
  done items. `todo done` becomes *remove the entry*, not move-to-Done.
- **State is a field, tool regroups on write** -- but with live-only
  there is barely any state left to track, so this mostly collapses:
  `todo.md` becomes one live queue, no sections to regroup.
- **Cross-ref check validates durable IDs only.** Round-local reviewer
  numbers (`RT-7`, `AQ-9`, `FR-12`) stay as unchecked provenance in a
  `source` field / prose. The gate validates durable IDs
  (`<rt|aq|fr|tf>-<date>-<slug>` and todo slugs) that appear in
  dedicated fields (`depends_on`, `supersedes`). The two known-dangling
  durable IDs are citations *of done items*, so they largely vanish
  with the done items.

### The map's key facts

- Two entry shapes: `todo.md` = `- **slug** -- summary` bullets in
  `## Pending`/`## Done`; the four logs = `### <id>` + `**Category:**`
  + prose. Two ID schemes: todo dateless kebab slug; logs/feedback
  `<prefix>-<date>-<slug>`.
- `fresh-reader-log.md` has two `##` sections; only "Deferred findings"
  is a backlog. "Explanations to keep" is finished work -- never an
  open item.
- `template-feedback.md` repeats its title after the ID on the heading
  line and has three lifecycle sections (Resolved is empty).

## Header format (2026-09-17, operator-confirmed)

Two `AskUserQuestion` answers fix the shape:

- **Bold-label lines.** Fields are written as `**Label:** value`,
  continuing the reviewer logs' existing `**Category:**` convention --
  not a fenced TOML block. Renders cleanly, smallest migration.
- **Unify onto headed entries.** `docs/todo.md`'s bullets become
  `### <slug>` sections carrying the same fields, so there is one
  entry shape and one parser across all five files.

The record model the parser reads:

- An entry opens with an `### <id>` heading. The id is the heading
  text up to a ` -- ` separator if present (so template-feedback's
  `### tf-... -- title` keeps its trailing title and `feedback-add`
  needs no change), else the whole heading. Durable ids are
  `<rt|aq|fr|tf>-<date>-<slug>`; the queue uses a dateless kebab slug.
- Directly under the heading (after an optional blank line) sits the
  **field block**: consecutive one-line `**Label:** value` fields.
  The block ends at the first blank or non-field line; a `**X:**` that
  appears later in the body is prose, not a field. So a body's
  `**Swept ...**` or `**Status:** ...` is never mistaken for a field.
  Known labels: Category, Summary, Source, Issue, Depends on,
  Supersedes. No field is mandatory (template-feedback entries carry
  none).
- Everything after the field block is the prose body.

`records-check` validates four things and nothing about phrasing:
duplicate id across the set, unknown field label, malformed heading
id, and a `Depends on`/`Supersedes` id that is in no entry. Only ids
in those two **fields** are checked; a durable id in prose stays
unchecked provenance -- which is where a citation of a done/removed
item lives, so it does not dangle the gate.

## Plan (increments, each its own commit)

Increment 3 splits into two commits so each leaves `validate` green
and is reviewable on its own:

- **3a -- parser + gate.** Shared `records` parser in `xtask` and a
  `cargo xtask records-check` gate over the four `###`-heading files
  (redteam, artisan, fresh-reader, template-feedback), wired into
  `validate` after Canon. Those files already conform, so the
  migration is near-zero; the gate's teeth today are duplicate-id,
  unknown-label and malformed-id, with the cross-ref check ready for
  the refs the queue brings. TDD via fixtures, red test per failure
  mode.
- **3b -- queue onto headed entries.** Convert `docs/todo.md` to
  `### <slug>` sections, rewrite `todo.rs` to add/list/remove over
  them, add `docs/todo.md` to the record set, and update the `/todo`,
  `/implement`, `/issue` skills. The queue is where live-to-live refs
  first appear (e.g. `minimal-vagrantfile` -> `doctor-checks-hyperv-
  support`), so it gives the cross-ref check real data.

1. **`todo.md` live-only.** `todo done <slug>` removes the pending
   entry (drop `--date`/`--doc`, drop `move_to_done` + `DoneDate` +
   `DocLink` + link guards + `list --done`); drop the `## Done`
   section and its content; update the preamble and the `/implement`
   skill (which calls `todo done`). Behaviour change -> TDD.
2. **Reviewer logs + feedback live-only.** Remove the closed/swept
   findings from `redteam-log.md`, `artisan-log.md`,
   `fresh-reader-log.md` (Deferred section only); drop
   `template-feedback.md`'s empty `## Resolved`. Docs edits; no tool.
3. **Typed header + integrity gate.** Give each live entry a
   machine-parseable header; build a shared parser + a
   `cargo xtask records-check` gate (validates `depends_on`/
   `supersedes` durable IDs), added to `validate`. TDD, red test per
   failure mode.
4. **Move-4 remainder.** Fold-or-delete `fresh-reader-log.md`
   (decision pending). The `template-feedback.md` rationale trim is
   decided against (2026-09-18) -- kept full for upstream adoption.

## Test strategy

Unit tests in `xtask` for every tooling change (behaviour change ->
red first). The integrity gate gets a red test per failure mode
(dangling id, malformed header, duplicate id). Migrations are data
edits, verified by the tooling round-tripping them.

## Progress log

- 2026-09-17: increment 1 (todo.md live-only) done.
  - `xtask/src/todo.rs`: `todo done <slug>` now removes the pending
    entry (new `remove_pending`) instead of moving it to `## Done`.
    Deleted the dead machinery -- `move_to_done`, `DoneDate`,
    `DocLink` and its three link guards, the `list --done` flag, and
    the `Done` subcommand's `--summary`/`--date`/`--doc` args -- and
    rewrote the test module (dropped the `## Done` fixtures and the
    move_to_done/DoneDate/DocLink tests; added `remove_pending`
    tests). Fixed every surviving doc-comment that linked to the
    removed items (the doc gate enforces intra-doc links).
  - `docs/todo.md`: removed the `## Done` section. Removed two items
    made moot by the change (`done-drops-the-body-silently`,
    `doc-link-guard-path-family` -- the latter guards the deleted
    `DocLink`), using the new `todo done`; reworded
    `add-issue-flag-unused` off its stale `done --doc` references and
    trimmed the in-progress `record-files-typed-header` item to a
    pointer here.
  - Skills: `/implement`, `/issue`, `/todo` reworded -- `todo done`
    removes rather than moves; `issue.md`'s guard anecdote no longer
    names the removed `--doc`.
  - `cargo xtask validate` green (coverage 98%); dogfooded on the
    real file (removed the two moot items; `list` works; `--done` is
    gone).
  - Left for increment 2 (logs live-only): reviewer-log and
    template-feedback entries about the removed `--doc`/Done are now
    moot findings, to be dropped there.
- 2026-09-17: increment 4b (template-feedback stale claims) done.
  - **Operator decision:** trim depth for `template-feedback.md`
    is *minimal* -- fix only the now-false claims, defer the
    broader rationale trim. The file's job is carrying arguments
    upstream, so aggressive trimming cuts against its purpose.
  - Fixed the stale bombyx-state claims in
    `tf-2026-09-04-todo-done-should-take-its-link-target`: added a
    dated note that bombyx has dropped queue issue links entirely
    (live-only queue; no `done --doc`, no `add --issue`), and
    reworded the `add --issue` clause from present to past. The
    `tf-2026-08-10` entries describe the template and use
    past-tense bombyx anecdotes, so they stay true and untouched.
    records-check green (5 files, 118 entries).
  - **Deferred, still open:**
    - The broad `template-feedback.md` rationale trim was later
      decided against outright (2026-09-18): the file is kept full
      so rustbase can adopt the improvements. Not just deferred --
      dropped. See `documentation-overhaul.md` item 3.
    - The pre-existing `### <id> -- <title>` heading lines in
      `template-feedback.md` run past 80 columns (the title-repeat
      convention). No gate reads them; a future pass could move the
      title into a `**Summary:**` field.
- 2026-09-17: increment 4c (`## Resolved` cascade) done.
  - Removed the empty `## Resolved` section from
    `template-feedback.md` and, so the tooling cannot offer a
    section that no longer exists, dropped `FeedbackSection::
    Resolved` from `xtask/src/feedback.rs` (variant, `header_
    keyword` arm, doc comments). Refactored the dozen tests that
    used `## Resolved` as a fixture onto the two live sections.
  - A resolved divergence is now removed, not filed: updated the
    file preamble (three -> two sections, noting the deliberate
    divergence from the template, which keeps three), the
    `/template-improve` skill (dropped the resolved routing;
    resolution means remove the entry), and CLAUDE.md's feedback
    description. `feedback-add --help` now offers only
    open/suggestion. `backfeed.rs` still names `## Resolved` as an
    example section header, correctly -- it reads downstream files
    that may still carry one.
  - clippy, tests, canon and records all green.
- 2026-09-17: increment 4a (fresh-reader log) done.
  - **Operator decision, reversing the increment-1 note above:**
    the **Explanations to keep** do-not-trim registry is retired --
    "we are not going to use it any longer." Removed the section
    from `fresh-reader-log.md`, and with it the instructions that
    fed it: the "What worked" section in `.claude/agents/fresh-
    reader.md` and the matching sentences in
    `.claude/commands/code-reviewers.md` and `review.md`, so the
    reviewer no longer produces output with nowhere to go.
  - Trimmed the deferred backlog to the fact: cut re-derived
    rationale, "Swept" lifecycle paragraphs and round-local
    "Found by FR-N" trailers; dropped the one finding my own 3b
    work resolved (`fr-2026-09-04-todo-md-header-documents-one-
    entry-shape` -- the queue now has one shape and its header says
    so). Verified the older `.claude` and `vm-host-setup.md`
    findings are still live before keeping them. 24 deferred
    entries -> 23; records-check now 5 files, 118 entries. canon
    and records green.
  - Left for 4b: trim `template-feedback.md` rationale, including
    the now-stale `--doc`/`--issue`/`move_to_done` sentences.
- 2026-09-17: increment 3b (queue onto headed entries) done.
  - `docs/todo.md`: converted 38 bullets to `### <slug>` entries
    with `**Summary:**` fields via a scratchpad awk converter
    (entry-count parity checked, no over-width lines). Added the
    two genuine live-to-live cross-refs -- `agent-vlan` ->
    `wire-vm-host`, `status-all-aggregator` -> `status-endpoint` --
    which now exercise the dangling-ref check on real data.
    Refs to completed items (e.g. `project-selection-flag`) stay
    in prose, unchecked. Updated the preamble to state the entry
    shape.
  - `xtask/src/todo.rs`: rewritten to read via `records::parse`
    and mutate `### <slug>` entries (`append_entry`,
    `remove_entry`) instead of `## Pending` bullets. Dropped the
    bullet/link/section machinery and the `--issue` flag (no
    caller; the link shape it wrote is gone), which resolves
    `add-issue-flag-unused` -- removed that entry with the tool.
    `records` exposes `parse`, `Record` accessors and `valid_id`
    so the queue and the gate share one parser. 16 tests.
  - `docs/todo.md` joined the record set: `records-check` covers
    5 files, 123 entries. Skills updated: `/todo` (summary budget
    is now a flat 67 after the `**Summary:**` prefix, not
    70-minus-slug; "bullet" -> "entry"), `/implement` (`## Pending`
    -> `### <slug>` entry).
  - Surfaced, deferred to increment 4: `template-feedback.md`'s
    todo-done-link entries still describe bombyx's `--doc`/
    `--issue`/`move_to_done`, all now removed. Those sentences are
    stale for bombyx (the upstream suggestion still stands). Not
    canon and no gate reads them; increment 4 ("trim
    template-feedback.md rationale") owns it.
- 2026-09-17: increment 3a (record parser + gate) done.
  - `xtask/src/records.rs`: a shared parser (heading id up to
    ` -- `, a field block of `**Label:** value` lines that ends at
    the first blank or non-field line, then prose) and the
    `records-check` gate over the four `###`-heading files. Four
    checks: duplicate id, unknown label, malformed id, dangling
    `Depends on`/`Supersedes`. 14 fixture tests, one red per
    failure mode.
  - Wired `RecordsCheck` into `main.rs` and the `validate` step
    table (after Canon, both markdown-only), updated the order
    test. Renumbered CLAUDE.md's gate list to eleven and added the
    command to Build Commands.
  - The four files already conformed, so no data migration:
    `records-check` runs clean (4 files, 86 entries). `validate`
    green (11 gates, coverage 98%).
  - Left for 3b: `docs/todo.md` conversion + `todo.rs` rewrite,
    where the first live-to-live cross-refs appear and exercise the
    dangling-ref check on real data.
- 2026-09-17: increment 2 (reviewer logs live-only) done.
  - `redteam-log.md`: removed the two moot `--doc`/`DocLink` findings
    (`rt-2026-09-04-doc-cannot-link-a-plans-own-section`,
    `rt-2026-09-04-doc-existence-check-answers-for-this-machine`) and
    the one CLOSED finding (`rt-2026-08-31-chmod-symlink-race`, kept
    "for the lesson" -- the operator confirmed removal; the lesson
    survives in git and in the code comment beside the fix). 15 -> 12
    entries.
  - `fresh-reader-log.md`: removed the moot `--doc` help finding
    (`fr-2026-09-04-todo-help-hides-four-of-five-doc-rules`). The
    "Explanations to keep" section is untouched.
  - Learned that "Swept" means reviewed-and-updated in a backlog
    sweep, not closed, so swept-but-open findings stay.
    `artisan-log.md` had no closed or moot entries.
  - Deferred to increment 3 (tooling): dropping
    `template-feedback.md`'s empty `## Resolved` cascades into the
    `feedback-add` `FeedbackSection::Resolved` enum, CLAUDE.md, and
    the file preamble -- a tooling change, not docs-only, so it rides
    with the typed-header/gate work rather than this cleanup.
