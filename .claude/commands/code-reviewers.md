# Code reviewers (gating rules)

Three reviewers guard every code commit. Their personas live as
first-class subagents in `.claude/agents/`, and the harness --
not an instruction they could talk themselves out of -- is what
keeps them from changing anything. None has `Edit` or `Write`.
`artisan` and `fresh-reader` have no shell at all. `red-team`
needs one to run `git show`, so its `Bash` grant is scoped to
read-only git subcommands; an unscoped `Bash` would let a
reviewer write files, which would make the guarantee a wish.

- **`red-team`** (`.claude/agents/red-team.md`) -- security &
  correctness. Tools: `Read`, `Grep`, `Glob`, and a `Bash`
  scoped to read-only git subcommands. It runs `git show` and
  `git log` itself, so it needs the shell. Tell it which commit
  range to review.
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
cannot tell a comment that carries a reason from padding.

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
- **Stop when you would not fix anything the round found** --
  every finding deferred, declined or already covered. Do not
  chase an empty report. After three rounds, hand what is left
  to the operator rather than starting a fourth.
- The fix commit uses **steps 1-8 of `/commit` only**. A nested
  `/commit` would run its own steps 9-11, so the reviewers
  would fire twice on one SHA and `/retrospect` would run
  mid-cycle.
- **A round that only defers still makes a commit**, carrying
  the backlog files alone. It is the terminal round, so no
  later commit exists to hold them, and a `.md`-only commit
  starts no new round.

## When to run

Run **all three** reviewers whenever the commit contains code:
`.rs`, `.toml`, `.sh`, `.ps1`, the Ruby and shell templates
under `crates/bombyx/templates/`, or a workflow under
`.github/`. That is the whole list. bombyx is CLI-only, so
there is no frontend here and no `playwright.config.ts` --
listing those extensions only sent a reader looking for a
frontend that does not exist. Never skip the reviewers, even
for "straightforward" changes. The only exception is a commit with
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
3. Nothing about the report format. Each agent file states its
   own, and they differ on purpose: `red-team` emits `RT-<n>`
   with a trigger, `artisan` emits `AQ-<n>` with a better
   approach, and `fresh-reader` emits `FR-<n>` with **Where it
   left me**, which is the field the other two cannot produce.
   Asking for a shape the agent file does not specify is how a
   report comes back without the IDs the fix commit needs to
   cite.

**Diff handoff.** `red-team` reads the diff itself via
`git show`. `artisan` has no shell, so it is handed the diff --
inline when it is small, or as a git-ignored path under
`target/` when it is large. Tell it which one it got; it cannot
work that out for itself. **Never `/tmp`**: under Git Bash on
Windows that resolves outside the workspace, where the operator
cannot see it.

Each agent's final message *is* its report (a plain-text finding
list, or "No issues found."), consumed by the calling skill --
not shown to the user directly.
