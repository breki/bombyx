---
description: Commit current changes following project conventions (no reviewing; that is /review)
allowed-tools: Bash(git status:*), Bash(git diff:*), Bash(git log:*), Bash(git add:*), Bash(git commit:*), Bash(git config --get:*), Bash(git tag --list:*), Bash(cargo xtask*), Read, Edit, AskUserQuestion, Skill(retrospect)
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

**`/commit` does no reviewing.** Run `/review` beforehand when
you want the work hardened first, or not at all. `CLAUDE.md`
under **Reviewing is its own process** says why the two are
separate.

## Instructions

1. **Analyze current state** - Run these commands in parallel:
   - `git status` (never use -uall flag)
   - `git diff` for unstaged changes
   - `git diff --cached` for staged changes
   - `git log --oneline -5` for recent commit style reference
   - `git config --get user.email` to confirm an author identity
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

     The check is: `git tag --list` (no tags at all means
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

6. **Stage files** - Add specific files by name (avoid
   `git add -A` or `git add .`). Never commit sensitive
   files (.env, credentials, etc.). Include diary and
   changelog if updated.

   A `/review` run before this one leaves two things
   behind. Its edits to `docs/developer/*-log.md` are
   part of the work, so stage them with it. It may also
   have left intent-to-add index entries, which
   `git status` shows as staged additions. An
   intent-to-add entry holds no content, so `git commit`
   skips it and staging by name cannot pick it up --
   there is nothing here you must undo. If the developer
   wants the index clean, tell them to run
   `git reset -- <path>` themselves; this skill has no
   `git reset` grant, deliberately.

7. **Fix line endings** - `git add` prints a CRLF warning
   when it converts one, so check its output now that the
   files are staged. All text files must use LF endings.

8. **Commit** using this exact format (use HEREDOC):

```bash
git commit -m "$(cat <<'EOF'
<type>: <subject>

<body>

AI-Generated: Claude Code (<ModelName> <YYYY-MM-DD>)
EOF
)"
```

9. **Workflow retrospective** -- delegate to `/retrospect`.
   It critiques how the work was done rather than the diff,
   so it wants the whole run to look back on, and it runs
   last so it cannot block shipping.

   The `/retrospect` skill owns the full set of rules: the
   four buckets (Efficiency / Quality / Speed / Cleanup),
   `[trivial]` vs `[propose]` tagging, the offer to
   auto-apply trivial findings, and the recursive-skip
   carve-out for workflow-only diffs (`.claude/**` /
   `CLAUDE.md` only). See `.claude/commands/retrospect.md`
   for the full contract.

   From here, simply invoke `/retrospect`. If the committed
   work would trigger the recursive skip, `/retrospect`
   no-ops silently. Otherwise it produces the report
   inline.

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
