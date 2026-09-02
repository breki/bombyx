# Code reviewers (gating rules)

Three reviewers guard every code commit. Their personas live as
first-class subagents in `.claude/agents/`, so their read-only
nature is a harness guarantee (they have no `Edit`/`Write`
tools), not just an instruction:

- **`red-team`** (`.claude/agents/red-team.md`) -- security &
  correctness. Tools: `Read, Grep, Glob, Bash`. It runs `git
  show` and `git log` itself, so it needs the shell. Tell it
  which commit range to review.
- **`artisan`** (`.claude/agents/artisan.md`) -- code quality &
  craftsmanship beyond clippy. Tools: `Read, Grep, Glob` only --
  no shell, so it is read-only by construction. Pass the diff to
  it in the spawn prompt (capture it once in the calling skill
  and hand it over).
- **`fresh-reader`** (`.claude/agents/fresh-reader.md`) --
  comprehension. Tools: `Read, Grep, Glob` only. It reads the
  changed files **whole**, not as a diff, because it is judging
  what a newcomer lands on rather than what moved. Pass it the
  list of changed file paths and a one-line description; do not
  pass it the diff.

**The three lanes do not overlap.** `red-team` asks whether the
code is safe and correct, `artisan` whether it is well made, and
`fresh-reader` whether a person arriving cold can work out what
it does from the files alone. The third catches a class the
other two are constitutionally blind to: both of them already
know what the change is for, so neither can notice that the code
never says.

`fresh-reader` is also the only one asked what **worked**. Its
report ends with the two or three explanations worth keeping,
which is what stops them being edited away by somebody who
cannot tell a load-bearing comment from padding.

This file defines *which* reviewers run, *when*, and *how* to
spawn them. The review criteria themselves live in the agent
files above. `/commit` is the only caller.

## Reviews run after the commit

The target is a **commit**, never the index. `/commit` commits
first, then spawns the reviewers against what it just made.
Fixes from a round land as their own commit, and the next round
reviews that. The reasoning is in `CLAUDE.md` under **Commits
and releases**; the operational consequences are:

- Pass a **commit range**, not a staged diff. For a first round
  that is `HEAD~1..HEAD` -- capture it once with
  `git show HEAD` or `git diff HEAD~1..HEAD`.
- **Never amend the commit under review.** A reviewer holding
  a SHA must be able to trust it still means what it meant.
- Give the reviewers the *previous* commit as context when the
  round is reviewing a fix commit, so a fix can be judged
  against what it was fixing.
- **The cycle stops when a round produces nothing you choose to
  fix.** All-deferred, all-declined or all-already-covered is
  the last round. Do not chase an empty report.

## When to run

Run **all three** reviewers whenever the commit contains code
changes:
Rust (`.rs`, `.toml`), frontend (`.svelte`, `.js`, `.ts`,
`.css`), config (`playwright.config.ts`, `vite.config.js`,
`vitest.config.js`, ...), or deployment / infrastructure files
(`.service`, `Dockerfile`, `docker-compose.yml`, `.conf`,
`.nginx`, `.env.example`, ...). Never skip them, even for
"straightforward" changes. The only exception is a commit with
no code at all (docs-only markdown / `.md` files) -- and even
there, consider `fresh-reader` alone when the commit rewrites
prose a newcomer will land on, such as `README.md` or anything
under `docs/`.

## How to spawn

Spawn all three in a **single parallel message** -- one `Agent`
call per reviewer -- so they run concurrently:

- `subagent_type: red-team`
- `subagent_type: artisan`
- `subagent_type: fresh-reader`

Give each spawn:

1. A one-line description of what the change does.
2. The commit range under review. For `artisan`, the captured
   diff as well (it has no shell); `red-team` runs `git show`
   itself. For `fresh-reader`, the **list of changed file
   paths** and nothing else -- it reads the files whole, and
   handing it a diff defeats the point of the persona.
3. The instruction to report each finding with the six labeled
   bullet fields: **ID**, **Source**, **Category**,
   **Description**, **Impact / Why it matters**, **Suggested
   fix**. `fresh-reader` uses its own three-field shape
   (**What**, **Where it left me**, **What would have helped**)
   and `FR-<n>` IDs; do not ask it for the six.

**Diff handoff.** Never write the diff to `/tmp` (on Windows +
Git Bash that resolves outside the workspace and is invisible to
the user). `red-team` reads it via `git show`; `artisan`
receives it inline. If a file is genuinely needed, use a
git-ignored path under `target/`.

Each agent's final message *is* its report (a plain-text finding
list, or "No issues found."), consumed by the calling skill --
not shown to the user directly.
