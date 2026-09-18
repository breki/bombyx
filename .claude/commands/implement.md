---
description: Plan and implement a captured issue from docs/todo.md
allowed-tools: Bash(cargo xtask*), Bash(git status:*), Bash(git diff:*), Bash(git log:*), Bash(git rm:*), Read, Write, Edit, Glob, Grep, Agent, AskUserQuestion, Skill(commit)
---

Plan and implement an item captured by `/todo`. The plan lives
at `docs/issues/<slug>.md` while the work is in progress -- a
working document, committed as the work lands and **removed when
the item is done**. What outlives it is the code, the reference
docs (`docs/architecture.md`, `docs/trust-boundary.md`), and the
`/commit` body; Phase 3 promotes anything durable into those
before the doc goes.

## Selecting the issue

- **With a slug argument** (e.g.
  `/implement search-bar-perf`): use that slug. If it
  is not an `### <slug>` entry in `docs/todo.md`,
  stop and tell the user.
- **Without arguments**: read `docs/todo.md`, list
  the pending slugs with their summaries, and ask the
  user which one to implement (use
  `AskUserQuestion`). Do not pick one yourself.

## Phase 1 -- Plan

1. Read `docs/todo.md` and locate the chosen item.

2. If `docs/issues/<slug>.md` already exists, read
   it -- earlier analysis may already be there.
   Otherwise create it.

3. Investigate the codebase enough to write a real
   plan: relevant files, current behaviour, where the
   change lands, risks. Use `Grep`/`Glob`/`Read` or
   delegate broad searches to an `Agent` with
   `subagent_type=Explore`.

4. Write `docs/issues/<slug>.md` with this structure:

   ```
   # <slug>

   **Status:** Planning
   **Captured:** <date the item was added, if known,
   else "unknown">
   **Started:** <today's date>

   ## Problem
   <what the user asked for, in your own words>

   ## Context
   <relevant files, current behaviour, constraints>

   ## Open questions
   <bulleted list -- fill in as you find them>

   ## Plan
   <numbered steps -- concrete, file-level when
   possible>

   ## Test strategy
   <unit tests, E2E tests, edge cases,
   bug-reproduction tests>

   ## Decisions
   <to be filled as questions get answered>
   ```

5. For every open question or design decision that
   materially changes the plan, call
   `AskUserQuestion`. Record the question **and the
   answer** under `## Decisions` in the issue doc.
   One decision per bullet, with date.

6. When the plan is ready, show the user a short
   summary (3-5 bullets max) and ask whether to
   proceed with implementation. Wait for an explicit
   yes. Do not start coding before that.

## Phase 2 -- Implement

Follow the project rules in `CLAUDE.md`. In
particular:

- **TDD applies to behaviour change.** For new logic
  in existing code or a bug fix in shipped code,
  write the failing test first. For structural
  additions (new self-contained module, new helper,
  new enum variant with no callers yet), test and
  implementation may land together as one unit --
  the pre-impl failure step adds no signal there.
  When in doubt, prefer the behaviour-change
  discipline. See `CLAUDE.md` "Test-Driven
  Development" for the full rule.
- **Test level -- prefer the cheapest that proves
  the behaviour.** Rust unit tests for library
  logic; integration tests (`--dry-run` against the
  real binary) for CLI behaviour. This is a CLI-only
  project: there is no browser, no Vitest and no
  Playwright suite. Note the choice briefly in the
  issue doc's `## Test strategy` section.
- **A dry run is not a real run.** `--dry-run`
  proves the argv, not that the VM host accepts it.
  For anything that changes the commands bombyx
  emits, say plainly in the issue doc whether it has
  been exercised against a real host.
- **All tests must pass.** Fix pre-existing failures
  you encounter; do not work around them.
- **Update `Status:`** in `docs/issues/<slug>.md` to
  `In progress` when you start coding. Append a
  `## Progress log` section with short dated entries
  as milestones land.
- **Ask, do not guess.** Use `AskUserQuestion`
  whenever a requirement or trade-off is unclear.
  Record the answer under `## Decisions`.

## Phase 3 -- Finalise

1. Run `cargo xtask validate`. Every gate must
   pass. `CLAUDE.md`'s **Definition of Done** lists them
   in execution order; do not restate the list here, or
   the two copies will disagree about how many there are.

2. If the change affects developer workflow or skills,
   update the relevant files under `.claude/commands/`
   and `docs/`. Clean up stale content while you are
   there.

3. **Promote anything durable, then remove the working doc.**
   The issue doc was scratch for planning and decisions; it does
   not stay in the tree. Before removing it, move anything worth
   keeping into its permanent home:
   - a design or security decision with its rationale, and any
     non-obvious constraint -- into `docs/architecture.md` or
     `docs/trust-boundary.md`, or a comment beside the code it
     governs;
   - what shipped and, above all, **what was and was not
     verified** (Definition of Done item 3) -- into the `/commit`
     body in step 6.

   Then remove the doc: `git rm docs/issues/<slug>.md` if it was
   ever committed, or just delete the working file if it only sat
   in the tree this session. Either way the finished item leaves
   no `docs/issues/<slug>.md`.

4. In `docs/todo.md`, remove the finished item mechanically (do
   not hand-edit the file):

   ```
   cargo xtask todo done <slug>
   ```

   The queue holds live work only, so this removes the entry.
   What shipped is recorded by the `/commit` body, the CHANGELOG
   and git history, so there is no `## Done` section to move it
   into.

5. Verify the change manually -- actually run it,
   do not infer from a green suite. For a change to
   the commands bombyx emits, that means a real
   run against a real VM host (Definition of Done
   item 3); `--dry-run` only proves the argv. For a
   change to `xtask`, run the affected subcommands
   and check their real output, including a failure
   path where one exists. Report plainly what was
   exercised and what was not.

6. Optionally run `/review`, then commit with `/commit`.

   `/review` hardens the work while it is still
   uncommitted, and neither command requires the other. For
   a change that touches the commands bombyx emits,
   `/review` is worth the time: it runs the thing rather
   than reading it.

## Rules

- Never skip the plan phase, even for a small change.
  The issue doc organizes the work while it is in flight;
  the record that survives is the `/commit` body and the
  reference docs Phase 3 feeds, not the doc itself.
- Never start implementing before the user explicitly
  approves the plan.
- Never edit `## Done` items in `docs/todo.md` except
  to add a new one when finalising.
- Use 80-character margins in all Markdown.
