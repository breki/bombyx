---
description: Plan and implement a captured issue from docs/todo.md
allowed-tools: Bash(cargo xtask*), Bash(git status:*), Bash(git diff:*), Bash(git log:*), Bash(scripts/e2e.sh*), Read, Write, Edit, Glob, Grep, Agent, AskUserQuestion, Skill(commit)
---

Plan and implement an item captured by `/todo`. The
plan lives at `docs/issues/<slug>.md` and is updated
as the work progresses.

## Selecting the issue

- **With a slug argument** (e.g.
  `/implement search-bar-perf`): use that slug. If it
  does not exist under `## Pending` in `docs/todo.md`,
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
  logic, integration tests for CLI behaviour,
  Vitest unit/component tests for frontend logic.
  Reach for a Playwright E2E test only when the
  behaviour genuinely requires a real browser:
  full-page navigation, multi-route flows, real
  network boundaries, focus or keyboard
  interactions that depend on the actual DOM, or a
  bug that only reproduces end-to-end. Note the
  choice briefly in the issue doc's
  `## Test strategy` section. Use the `web-dev`
  skill when writing E2E tests.
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

1. Run `cargo xtask validate`. All checks must pass
   (fmt, clippy, tests, coverage >= 90%, duplication
   <= 6%, frontend type-check).

2. If the change affects developer workflow or skills,
   update the relevant files under `.claude/commands/`
   and `docs/`. Clean up stale content while you are
   there.

3. In `docs/issues/<slug>.md`:
   - Set `Status:` to `Done`.
   - Add `**Completed:** <today's date>`.
   - Add a final `## Outcome` section: what shipped,
     links to changed files (path:line where
     useful), follow-ups.

4. In `docs/todo.md`, move the item to Done
   mechanically (do not hand-edit the file):

   ```
   cargo xtask todo done <slug> --date <today's date>
   ```

   The command moves the bullet to the top of `## Done`
   (newest first), stamps the date, and links the slug
   to `issues/<slug>.md`. Pass `--summary "<text>"` to
   override the pending summary for the Done entry.

5. **Pre-launch code reviewers in the background**
   (optional optimisation). The next `/commit` runs
   the Red Team and Artisan agents against the same
   diff this implementation produces. When the diff
   is *likely to stay stable* through user
   verification, you can spawn both agents now with
   `run_in_background: true` -- `subagent_type: red-team`
   and `subagent_type: artisan` (pass `artisan` the
   captured diff; `red-team` reads it itself). Follow the
   gating rules in `.claude/commands/code-reviewers.md`
   (the same file `/commit` step 3 uses, so the pre-launch
   and the commit-time review are identical). Note the agent
   IDs in conversation context so `/commit` can reuse the
   results.

   **Skip the pre-launch** when:
   - The diff is docs-only (`*.md` only) -- no
     reviewers run for docs-only commits anyway.
   - User verification at step 6 is likely to
     invalidate the diff. Signals: the change
     introduces inference / heuristic logic, has
     open clarifying questions, involves user data
     the agent hasn't seen, or introduces an undo /
     inverse / optimistic-concurrency path. That
     last shape is review-volatile because it hinges
     on a backend contract an adversarial review
     routinely overturns -- an undo or inverse that
     looks right for the simple case but breaks the
     recurring / concurrent one, so the design gets
     reshaped after review and the heavy gates would
     run twice. Stale pre-launched findings are worse
     than no pre-launch -- they describe code that no
     longer exists. When in doubt, skip; `/commit`
     will spawn fresh reviewers when it runs.

6. Verify the change manually. For UI / API
   changes, exercise the feature in a browser (run
   the backend + frontend dev servers; see
   `CLAUDE.md` "Frontend Development"). For
   deployment-affecting changes consider running
   `cargo xtask deploy` against a staging target.

7. Commit with `/commit`. If pre-launched reviewers
   from step 5 are still in flight, `/commit` should
   wait for them rather than spawning duplicates.
   If their findings already arrived, `/commit`
   consumes them directly.

## Rules

- Never skip the plan phase, even for a small change.
  The issue doc is the audit trail.
- Never start implementing before the user explicitly
  approves the plan.
- Never edit `## Done` items in `docs/todo.md` except
  to add a new one when finalising.
- Use 80-character margins in all Markdown.
