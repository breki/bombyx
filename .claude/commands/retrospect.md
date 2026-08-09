---
description: Workflow retrospective on the session so far -- Efficiency, Quality, Speed, Cleanup
allowed-tools: Bash(git status:*), Bash(git diff:*), Bash(git log:*), Read, Grep, Glob, AskUserQuestion, Edit
---

Reflect on the *process* of the work just completed
(or in progress) and surface concrete improvements
to how the session was run. This complements code
reviews (Red Team / Artisan in `/commit`) which
critique the diff -- the retrospective critiques the
*way* the diff was produced.

Invoke this skill either:
- **Automatically** at the end of `/commit` step 12
  (which delegates to this skill).
- **Manually** at any time -- mid-session, after a
  failed attempt, before a hand-off -- when you want
  a process-level reflection that is not tied to a
  commit.

## When to skip

**Recursive workflow-only skip.** When invoked
automatically from `/commit` and the just-committed
diff is entirely under `.claude/**` or `CLAUDE.md`,
skip silently. Those sessions ARE the workflow
authoring -- any "improvements" loop back to the
files just shipped. `docs/**` and `README.md`
commits still run the retro because those sessions
usually involve real research / tool work worth
reflecting on.

The skip does not apply when the user invokes
`/retrospect` directly -- the user asked for it
explicitly, so run it even on a workflow-only
session.

## Surfacing findings

Walk the session (or the just-committed work) and
collect findings in four buckets. Aim for 0-3
findings per bucket; do not invent filler.

**Every finding must name a concrete proposed
fix** -- a specific edit, command, or deletion (see
the **Fix** field under Tagging). A pure observation
with no fix attached ("be aware of X", "this went
fine") is not a finding: drop it, do not show it to
the user. The retro lists only items the user can act
on.

1. **Efficiency** -- tool calls that wasted budget.
   Examples:
   - Reading the same file twice when the first read
     was still in context.
   - Running full `cargo xtask validate` when
     `cargo xtask check` or `cargo xtask test`
     would have caught the same thing in a fraction
     of the time.
   - Repeated round-trips on a pattern that could
     have been extracted into a helper.
   - `cd subdir && ...` constructions that lose
     working-directory context across calls.
   - Sequential Agent calls that had no dependency
     and could have run in parallel.

2. **Quality** -- process shortcomings the code
   reviewers do not catch. Examples:
   - Committed before running a verification step
     (e.g. browser-checked a UI change only after
     the commit, not before).
   - Skipped a cross-reference (e.g. did not check
     `docs/developer/template-feedback.md` before
     applying a template change).
   - Undocumented decision -- chose between two
     approaches without recording why.
   - Did not capture a finding the reviewers raised
     into the long-lived RT/AQ logs.

3. **Speed** -- wall-time delays caused by ordering.
   Examples:
   - Slow step ran in the foreground when
     `run_in_background: true` would have unblocked
     other work.
   - Reviewer agents ran serially when they could
     have been one parallel message.
   - The /commit reviewers could have been
     pre-launched during `/implement` Phase 3 so
     `/commit` had results waiting.

4. **Cleanup** -- stale, duplicate, or redundant
   entries in canon or memory (including `.claude/`
   skills and commands). Growth is otherwise
   monotonic: retros add rules, nothing prunes them.
   Examples:
   - A memory entry whose rule was promoted to
     `CLAUDE.md` but the memory copy was not deleted.
   - Two `CLAUDE.md` bullets covering the same ground
     in different sections.
   - A skill/command referencing a tool, file, or
     workflow that no longer exists.
   - Two skills with overlapping responsibility where
     one supersedes the other.

   Scope the routine pass to what the session touched.
   **Broader canon scan (periodic, not every retro):**
   when the session-touched Cleanup pass has surfaced
   nothing for 3+ consecutive retros, expand the next
   retro's Cleanup pass to a full canon scan -- look
   for adjacent paragraphs covering the same ground,
   deprecated triggers, automatic rules that could be
   dropped, and examples referencing code that no
   longer exists. Exclude structural reorganization
   from scope (that is its own deliberate task).

## Tagging findings

For each finding, assign:

- **ID:** `<N>-<slug>` (e.g. `1-redundant-reads`,
  `2-serial-agents`). Single digit per session; the
  N resets each retro.
- **Short:** a one-line summary of the issue -- the
  compressed label a reader skims (e.g. "read
  `coverage.rs` twice"). The claim only: no rationale,
  no fix.
- **Long:** two or three sentences with the detail --
  what happened, why it matters, and turn numbers or
  tool names so the user can verify.
- **Fix:** one line naming the concrete change
  proposed -- the file to edit and what to
  add/remove/merge, the command to run, the entry to
  delete. State the destination explicitly (e.g.
  "add a clause to `CLAUDE.md ## Build Commands`",
  "delete `foo.md` from memory"). "Be aware of X" /
  "consider Y" / "this went fine" are NOT fixes. **If
  you cannot name a concrete fix, the finding is not
  actionable -- omit it entirely; do not show it.**
- **Tag:**
  - `[trivial]` if the fix is a single tool call
    right now (append a clause to a doc, add a
    permission to settings, rename a constant).
  - `[propose]` if the fix needs user input,
    cross-cuts files, implies a policy change, or
    requires architectural judgement.

## Presenting

Every shown finding is presented as three lines --
`Short:` (short description), `Long:` (longer
description), and `Fix:` (proposed fix) -- under its
`ID [tag]` header. Findings without a concrete fix
were already dropped during surfacing, so they never
appear here. A bucket with no actionable findings
shows `(none)` -- do not pad it with observations.

Output a short report like:

```
Workflow retrospective

Efficiency:
  1-redundant-reads [trivial]
    Short: read `coverage.rs` twice.
    Long: read coverage.rs at turns 3 and 7; the
      first read was still in context, so the second
      round-trip was wasted budget.
    Fix: note already-read files before re-reading.

Quality:
  (none)

Speed:
  2-serial-reviewers [propose]
    Short: fix iteration ran gates serially.
    Long: Red Team and Artisan ran in parallel, but
      the follow-up fixes ran validate/fmt/validate
      one at a time (turns 8-11), roughly doubling the
      wall time of that stretch.
    Fix: add a clause to `CLAUDE.md ## Build Commands`
      to fold fmt into the validate wrapper.

Cleanup:
  3-stale-skill-ref [trivial]
    Short: `web-dev` names a renamed config file.
    Long: the web-dev skill still names
      `playwright.config.js`; the file is
      `playwright.config.ts`.
    Fix: rename the reference in the web-dev skill.
```

Every shown finding names a concrete edit / command /
deletion in its `Fix:` line. An item whose only "fix"
would be "be aware of X" has no concrete fix, so it is
dropped during surfacing and never reaches this report.

End the report with one of:

- "Apply trivial findings now?" -- if any
  `[trivial]` items exist, offer to apply them via
  `AskUserQuestion`. Apply only the selected ones.
- "No trivial findings to auto-apply." -- if every
  finding is `[propose]` or there are no findings.

## What stays ephemeral

`[propose]` findings are surfaced for awareness and
discarded unless the user asks to escalate them. The
escalation paths:

- **Real RT/AQ finding:** append to
  `docs/developer/redteam-log.md` or
  `artisan-log.md` (only when the finding describes
  a defect in shipped code, not a process gap).
- **TODO item:** capture via `/todo <text>` for a
  follow-up implementation pass.
- **Doc edit:** small process rule changes (e.g. "add
  this to CLAUDE.md") land directly via `Edit`.

Without explicit user direction, do not write the
findings anywhere. The transcript is the record.

## Rules

- Be specific. "Could have been faster" is not a
  finding; "the two Agent calls in turn N had no
  dependency and could have run in one message" is.
- Every shown finding carries all three lines --
  `Short:`, `Long:`, and a concrete `Fix:` (a specific
  edit, command, or deletion). If a finding has no
  concrete fix -- it is pure awareness or a "went fine"
  note -- omit it entirely. Never show the user an item
  they cannot act on.
- Cite turn numbers or tool names when possible so
  the user can verify.
- Do not duplicate Red Team / Artisan findings.
  Those critique the code; this critiques the
  process.
- Cap output at ~15 lines of finding text. Beyond
  that, prioritise the highest-impact items.
- One retrospective per session is usually enough.
  Repeated invocations within the same session
  should focus on work since the last retro.
