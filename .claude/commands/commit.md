---
description: Commit current changes following project conventions
allowed-tools: Bash(git status:*), Bash(git diff:*), Bash(git log:*), Bash(git add:*), Bash(git commit:*), Bash(git config:*), Bash(cargo xtask*), Read, Edit, Agent, AskUserQuestion, Skill(retrospect)
---

Commit the current changes following the project's git commit
conventions.

`/commit` is a save-point, not a release event. It does **not**
bump the version, sync `Cargo.lock`, or run
`cargo xtask validate`. Those belong to `/release`, which is
invoked separately. `/commit` itself never runs
`cargo xtask validate`; if you want the full gate on a
work-in-progress, run it manually at your own shell (outside
this flow). `/release` runs it as the authoritative release
gate.

**Reviews run after the commit, not before it.** The order is:
commit, review, commit the fixes, review again, stop when a
round turns up nothing worth fixing. Never amend the commit a
review was against. `CLAUDE.md` under **Commits and releases**
says why; `.claude/commands/code-reviewers.md` has the
mechanics.

That means a `/commit` invocation does not finish at the
commit. Steps 9 and 10 are part of the same run.

## Instructions

1. **Analyze current state** - Run these commands in parallel:
   - `git status` (never use -uall flag)
   - `git diff` for unstaged changes
   - `git diff --cached` for staged changes
   - `git log --oneline -5` for recent commit style reference
   - `git config user.email` to confirm an author identity
     exists. It costs nothing when set, and when it is not,
     `git commit` fails at step 8 with the diary and the
     CHANGELOG already written. Cheaper to find here.

2. **Read the diff and decide the message** - work out what
   changed and settle on:
   - The commit type: feat, fix, chore, refactor, docs, test,
     style, perf
   - A concise subject line (imperative mood, no period)
   - A brief body explaining what and why

3. **Update development diary** (for significant changes):
   - Read `docs/developer/DIARY.md` to see format and
     recent entries
   - Add an entry for:
     - `feat`, `fix`, `perf` commits (functional changes)
     - Infrastructure/setup changes that affect developer
       workflow
   - Entries are in reverse chronological order (newest
     first)
   - Merge entries for the same day under one
     `### YYYY-MM-DD` heading
   - Title entries by topic only (no `(vX.Y.Z)` suffix).
     The version is unknown at commit time -- it is
     assigned later by `/release` when the changes ship.
   - Use backticks for technical terms
   - Skip diary update for: docs, style, test, refactor,
     minor chores

4. **Update CHANGELOG.md** (for user-observable
   changes):
   - The trigger is the **observable effect**, not the
     commit type. If a user of the software would see
     a difference (new feature, fixed bug, changed
     default, removed flag, new config knob, port
     change, new env var, ...), add a bullet to the
     `[Unreleased]` section -- **even if the commit
     type is `chore`** (e.g., a `chore:` that changes a
     default port still needs a `Changed` entry).
   - Add it mechanically rather than hand-editing (the
     `[Unreleased]` subsections can sit dozens of lines
     apart, so a hand edit easily splits a block with a
     duplicate heading):

     ```
     cargo xtask changelog add --kind <added|changed|fixed|removed> \
       [--breaking] "<entry text>"
     ```

     The command finds the right `### <kind>` heading
     under `[Unreleased]` (creating it in canonical
     order only if absent) and wraps the text to 80
     columns. `--breaking` prefixes `**BREAKING:**` so
     `/release` infers a major bump from the accumulated
     `[Unreleased]` entries.
   - **Confirm the behaviour shipped before you feed
     the major-bump inference.** Two inputs reach it,
     not one: `--breaking` and a non-empty
     `### Removed`. The rule protects the *inference*,
     so it applies to both -- avoiding `--breaking`
     and then filing `--kind removed` for the same
     never-shipped behaviour produces exactly the
     spurious major bump it is meant to prevent.

     The check is: `git tag` (no tags at all means
     nothing has ever been released), and when tags do
     exist, `git log <latest-tag>..HEAD` or the
     `[Unreleased]` block -- behaviour introduced since
     the last release never shipped either.

     When it did not ship, *correct the existing
     bullet* instead of adding a new one, and say which
     you did and why in the summary. Nothing is
     breaking for users who never saw the old
     behaviour.

     Correcting a bullet is the one case that **is** a
     hand edit: `cargo xtask changelog` only appends,
     so there is no mechanical path for it. Re-read the
     `[Unreleased]` block afterwards and check you have
     not split a subsection or left a duplicate
     `### <kind>` heading -- the hazard the bullet above
     warns about.
   - Skip only for commits with no user-observable
     effect: pure refactors, internal tooling, test-
     only changes, CI/lint config tweaks invisible to
     users, docs-only edits.

5. **E2E tests** -- **Not applicable to this project.**
   bombyx is CLI-only: the template's frontend, backend and
   Playwright suite were removed, and `scripts/e2e.sh` does
   not exist. Skip this step without comment.

   The equivalent verification here is Definition of Done
   item 3 -- running the real thing against a real VM host.
   `--dry-run` proves the argv, not that the remote accepts
   it, so for any change to the commands bombyx emits, say
   plainly in the summary whether that check has been done.

6. **Fix line endings** - After staging, check for CRLF
   warnings. All text files must use LF line endings.

7. **Stage files** - Add specific files by name (avoid
   `git add -A` or `git add .`). Never commit sensitive
   files (.env, credentials, etc.). Include diary and
   changelog if updated.

8. **Commit** using this exact format (use HEREDOC):

```bash
git commit -m "$(cat <<'EOF'
<type>: <subject>

<body>

AI-Generated: Claude Code (<ModelName> <YYYY-MM-DD>)
EOF
)"
```

9. **Code review** -- **after the commit lands**, spawn the **three**
   dedicated reviewer agents **in parallel** (in a single
   message with three Agent tool calls). They are read-only by
   construction -- none has `Edit`/`Write`.

   **IMPORTANT:** Always run all three reviews when the
   commit contains code changes: Rust (`.rs`, `.toml`),
   frontend (`.svelte`, `.js`, `.ts`, `.css`), config
   files (`playwright.config.ts`, `vite.config.js`,
   `vitest.config.js`, etc.), or
   deployment/infrastructure files (`.service`,
   `Dockerfile`, `docker-compose.yml`, `.conf`,
   `.nginx`, `.env.example`, etc.).
   Never skip them -- even for "straightforward"
   changes. The only exception is commits that contain
   no code at all (docs-only markdown, `.md` files) -- and
   there, still consider `fresh-reader` alone when the
   commit rewrites prose a newcomer lands on (`README.md`,
   anything under `docs/`).

   Spawn **Red Team** (security & correctness,
   `subagent_type: red-team`), **Artisan** (code quality,
   `subagent_type: artisan`) and **Fresh Reader**
   (comprehension, `subagent_type: fresh-reader`) in the
   single parallel message, giving each a one-line
   description of what the change does, and the commit
   range under review (`HEAD~1..HEAD` for the first
   round). `red-team` runs `git show` itself; `artisan` has
   no shell, so pass it the captured diff in its spawn
   prompt; `fresh-reader` gets the **list of changed file
   paths** and no diff, because it reads the finished files
   whole rather than what moved. The gating
   rules -- when to run, how to spawn, the diff-handoff rule
   (never `/tmp`; a `target/`-local file if one is truly
   needed), and the six labeled-bullet reporting format -- live
   in `.claude/commands/code-reviewers.md` (also used by
   the only caller). The review criteria
   live in the agent files under `.claude/agents/`.

   **Cross-confirmed findings:**
   Before presenting findings, scan all three reviewers'
   output for overlap. Two or more findings are
   **cross-confirmed** when they describe the same
   root cause -- either:
   - Same `file:line` reference (or overlapping line
     ranges in the same file), OR
   - Same defect described in different vocabulary
     (e.g. Red Team flags "TOCTOU on `is_dir` then
     `remove_dir_all`" while Artisan flags "follows
     symlinks during deletion despite `dir_size`'s
     guard" -- both pointing at the same line)

   Cross-confirmed findings are a stronger signal
   than unique ones. When found, present them under a
   **Cross-confirmed** heading naming which reviewers
   flagged it independently. Empirically (from sessions
   on this project and its siblings) every
   cross-confirmed finding has been selected for
   fixing; unique findings have a lower hit rate.

   A `fresh-reader` finding pairing with one of the
   other two is worth extra weight: it means a defect is
   both real and invisible from the files, so fixing the
   code without fixing what it says would leave the next
   reader in the same place.

   **Truncated reviewer output:**
   Before presenting findings, scan each reviewer's
   reply for finding IDs that appear in its summary
   or cross-references but whose full bodies (the
   six labeled-bullet fields) are not present in the
   returned text. Subagent replies are occasionally
   truncated and a summary line like "RT-001
   (permission globs), RT-002 (test robustness)" with
   no matching body for those IDs is a strong signal
   the body was dropped. In that case, use
   `SendMessage` to the same agent (its ID is in the
   tool result) and ask it to re-emit the missing
   findings verbatim, with the same labeled-bullet
   structure. Do this *before* presenting to the
   user -- otherwise findings the reviewer actually
   raised are silently dropped.

   **Presenting findings to the user:**

   Auto-apply is the default. Most findings are
   mechanical (exact-match regression, missing
   aria-label, rename a local, tighten a regex,
   stale-doc fix); apply those directly and announce the
   set you are applying so the user can interrupt. Only
   escalate a finding via `AskUserQuestion` when it
   crosses a threshold:
   1. large rework (>5 files, >100 lines, or
      out-of-diff churn),
   2. two findings conflict with each other,
   3. a genuine design tradeoff,
   4. a public-surface or breaking change,
   5. a new dependency,
   6. out of scope for the work in hand.

   The commit has already landed, so nothing here blocks
   shipping and there is no "Commit as-is" option to
   offer. Present each escalated finding in full (ID,
   Source, Category, Description, Impact, Suggested fix)
   and ask whether to fix it now, defer it to the backlog,
   or decline it; split across questions (max 4 options
   each) if needed. Still surface **every** finding --
   applied, escalated, deferred or declined -- in your
   summary; never silently drop one. Cross-confirmed findings
   (two or more reviewers, same root cause) are the
   strongest signal -- note which ones agreed.

   `fresh-reader`'s **What worked** section is not a
   finding and needs no action. Do not act on it, and do
   not drop it either: carry it into the summary, so the
   passages it named are known to be load-bearing the
   next time somebody trims comments.

   **Deferred findings backlog:**

   A **fixed** finding gets **no** log entry -- its
   resolution lives in the *fix* commit's message, which
   cites the ID, so `git log -S` on an ID finds both the
   finding and what was done about it. Only a finding
   deliberately *deferred* (real, but not fixed now) is
   logged, as a backlog:
   - `docs/developer/redteam-log.md` (Red Team)
   - `docs/developer/artisan-log.md` (Artisan)
   - `docs/developer/fresh-reader-log.md` (Fresh Reader)

   All three are newest-first; new entries go right after
   the `---`. Use a self-describing date-slug ID --
   `<rt|aq|fr>-<YYYY-MM-DD>-<kebab-slug>` (e.g.
   `rt-2026-07-14-fetch-no-timeout`) -- so there is no
   central counter to maintain and the ID is greppable
   from commit messages. Each entry is the ID heading, a
   `**Category:**` line, and a short description of the
   deferred issue. A later commit that acts on or
   reverses a deferred item cites its ID inline
   ("supersedes rt-2026-07-14-..."). Stage any changed
   backlog file **with the fix commit** in step 10, never
   with the commit under review -- that one is already made
   and must not be amended. **Threshold:** if 10+ items sit
   open in any one backlog, tell the user a full-codebase review
   is warranted.

10. **Commit the fixes, then review again.** Every fix from
    step 9 lands as its **own** commit. Never amend the
    commit the review was against: a reviewer holding a SHA
    has to be able to trust it, and folding a fix into the
    commit that needed it erases the fact that anything was
    found.

    Write the fix commit like any other -- through this same
    skill, with the diary and CHANGELOG steps if they apply
    -- and name the finding IDs it resolves in the body,
    with the reasoning for any you declined.

    Then **run step 9 again** against the fix commit, giving
    the reviewers the previous commit as context so a fix
    can be judged against what it was fixing. A fix is code,
    and code written under review pressure is the code most
    likely to be wrong.

    **Stop when a round produces nothing you choose to
    fix.** All-deferred, all-declined or all-already-covered
    is the last round. Do not chase an empty report:
    reviewers will always find something, and the stopping
    rule is agreement on what matters.
11. **Workflow retrospective** -- delegate to
    `/retrospect`, once the review cycle in steps 9 and 10
    has stopped. It critiques how the work was done, so it
    wants the whole run to look back on, and it runs last
    so it cannot block shipping.

    The `/retrospect` skill owns the full set of
    rules: the four buckets (Efficiency / Quality /
    Speed / Cleanup), `[trivial]` vs `[propose]` tagging,
    the offer to auto-apply trivial findings, and
    the recursive-skip carve-out for workflow-only
    diffs (`.claude/**` / `CLAUDE.md` only). See
    `.claude/commands/retrospect.md` for the full
    contract.

    From here, simply invoke `/retrospect`. If the
    committed work would trigger the recursive skip,
    `/retrospect` no-ops silently. Otherwise it produces
    the report inline.


## Rules

- DO NOT include "Co-Authored-By" lines
- DO NOT include "Generated with [Claude Code]" lines
- Use the AI-Generated footer format shown above
- If no changes to commit, inform the user
- If changes look incomplete or risky, ask before committing.
  A review finding is not a reason to hold the commit -- it lands
  first and the fix follows in its own commit
- Never bump `crates/bombyx/Cargo.toml` from `/commit`.
  That is `/release`'s job.

## Commit Types

The commit type no longer drives a version bump directly.
`/release` computes the bump from the accumulated
`[Unreleased]` CHANGELOG entries since the last release --
the "eventually" below refers to that later `/release`.

- `feat`: New feature (eventually a minor bump at release)
- `fix`: Bug fix (eventually a patch bump at release)
- `perf`: Performance improvement (eventually a patch bump)
- `chore`: Maintenance, tooling, dependencies
- `refactor`: Code restructuring
- `docs`: Documentation only
- `test`: Adding or updating tests
- `style`: Formatting, whitespace
