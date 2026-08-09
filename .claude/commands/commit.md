---
description: Commit current changes following project conventions
allowed-tools: Bash(git status:*), Bash(git diff:*), Bash(git log:*), Bash(git add:*), Bash(git commit:*), Bash(scripts/e2e.sh*), Bash(cargo xtask*), Read, Edit, Agent, AskUserQuestion, Skill(retrospect)
---

Commit the current changes following the project's git commit
conventions.

`/commit` is a save-point, not a release event. It does **not**
bump the version, sync `Cargo.lock`, or run
`cargo xtask validate`. Those belong to `/release`, which is
invoked separately (and is required before
`cargo xtask deploy`). `/commit` itself never runs
`cargo xtask validate`; if you want the full gate on a
work-in-progress, run it manually at your own shell (outside
this flow). `/release` runs it as the authoritative release
gate.

## Instructions

1. **Analyze current state** - Run these commands in parallel:
   - `git status` (never use -uall flag)
   - `git diff` for unstaged changes
   - `git diff --cached` for staged changes
   - `git log --oneline -5` for recent commit style reference

2. **Review changes** - Analyze what was changed and determine:
   - The commit type: feat, fix, chore, refactor, docs, test,
     style, perf
   - A concise subject line (imperative mood, no period)
   - A brief body explaining what and why

3. **Code review** -- Before E2E tests, spawn the **two**
   dedicated reviewer agents **in parallel** (in a single
   message with two Agent tool calls). They are read-only by
   construction -- neither has `Edit`/`Write`.

   **IMPORTANT:** Always run both reviews when the diff
   contains code changes: Rust (`.rs`, `.toml`),
   frontend (`.svelte`, `.js`, `.ts`, `.css`), config
   files (`playwright.config.ts`, `vite.config.js`,
   `vitest.config.js`, etc.), or
   deployment/infrastructure files (`.service`,
   `Dockerfile`, `docker-compose.yml`, `.conf`,
   `.nginx`, `.env.example`, etc.).
   Never skip them -- even for "straightforward"
   changes. The only exception is commits that contain
   no code at all (docs-only markdown, `.md` files).

   Spawn the **Red Team** (security & correctness,
   `subagent_type: red-team`) and **Artisan** (code quality,
   `subagent_type: artisan`) agents in the single parallel
   message, giving each a one-line description of what the
   change does. `red-team` runs `git diff --cached` itself;
   `artisan` has no shell, so pass it the captured
   `git diff --cached` output in its spawn prompt. The gating
   rules -- when to run, how to spawn, the diff-handoff rule
   (never `/tmp`; a `target/`-local file if one is truly
   needed), and the six labeled-bullet reporting format -- live
   in `.claude/commands/code-reviewers.md` (also used by
   `/implement`'s pre-launch reviewers). The review criteria
   live in the agent files under `.claude/agents/`.

   **Cross-confirmed findings:**
   Before presenting findings, scan both reviewers'
   output for overlap. Two findings are
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
   **Cross-confirmed** heading noting that both
   reviewers flagged it independently. Empirically
   (from sessions on this project's siblings) every
   cross-confirmed finding has been selected for
   fixing; unique findings have a lower hit rate.

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
   6. out of scope for this commit.

   Present each escalated finding in full (ID, Source,
   Category, Description, Impact, Suggested fix) with
   "Commit as-is" and "Abort" options; split across
   questions (max 4 options each) if needed, and wait for
   the answer before committing. Still surface **every**
   finding -- applied or escalated -- in your summary;
   never silently drop one. Cross-confirmed findings
   (both reviewers, same root cause) are the strongest
   signal -- note them as such.

   **Deferred findings backlog:**

   A **fixed** finding gets **no** log entry -- its
   resolution lives in the commit message. Only a finding
   deliberately *deferred* (real, but not fixed now) is
   logged, as a backlog:
   - `docs/developer/redteam-log.md` (Red Team)
   - `docs/developer/artisan-log.md` (Artisan)

   Both are newest-first; new entries go right after the
   `---`. Use a self-describing date-slug ID --
   `<rt|aq>-<YYYY-MM-DD>-<kebab-slug>` (e.g.
   `rt-2026-07-14-fetch-no-timeout`) -- so there is no
   central counter to maintain and the ID is greppable
   from commit messages. Each entry is the ID heading, a
   `**Category:**` line, and a short description of the
   deferred issue. A later commit that acts on or
   reverses a deferred item cites its ID inline
   ("supersedes rt-2026-07-14-..."). Stage any changed
   backlog file. **Threshold:** if 10+ items sit open in
   either backlog, tell the user a full-codebase review
   is warranted.

4. **Update development diary** (for significant changes):
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

5. **Update CHANGELOG.md** (for user-observable
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
   - Skip only for commits with no user-observable
     effect: pure refactors, internal tooling, test-
     only changes, CI/lint config tweaks invisible to
     users, docs-only edits.

6. **E2E tests** -- Run `scripts/e2e.sh` to verify the
   full stack works end-to-end. The script kills stale
   servers and runs Playwright, which auto-starts both
   backend and frontend using test data (not production
   data).
   - If E2E tests **fail**, ask the user whether to
     commit anyway or abort.
   - Skip if no frontend or API changes in the diff.

7. **Fix line endings** - After staging, check for CRLF
   warnings. All text files must use LF line endings.

8. **Stage files** - Add specific files by name (avoid
   `git add -A` or `git add .`). Never commit sensitive
   files (.env, credentials, etc.). Include diary and
   changelog if updated.

9. **Commit** using this exact format (use HEREDOC):

```bash
git commit -m "$(cat <<'EOF'
<type>: <subject>

<body>

AI-Generated: Claude Code (<ModelName> <YYYY-MM-DD>)
EOF
)"
```

10. **Workflow retrospective** -- delegate to
    `/retrospect` (runs *after* the commit lands so
    it cannot block shipping).

    The `/retrospect` skill owns the full set of
    rules: the four buckets (Efficiency / Quality /
    Speed / Cleanup), `[trivial]` vs `[propose]` tagging,
    the offer to auto-apply trivial findings, and
    the recursive-skip carve-out for workflow-only
    diffs (`.claude/**` / `CLAUDE.md` only). See
    `.claude/commands/retrospect.md` for the full
    contract.

    From here, simply invoke `/retrospect`. If the
    just-committed diff would trigger the recursive
    skip, `/retrospect` no-ops silently. Otherwise
    it produces the report inline.

## Rules

- DO NOT include "Co-Authored-By" lines
- DO NOT include "Generated with [Claude Code]" lines
- Use the AI-Generated footer format shown above
- If no changes to commit, inform the user
- If changes look incomplete or risky, ask before committing
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
