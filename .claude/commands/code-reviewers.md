# Code reviewers (gating rules)

Three reviewers guard every code commit. Their personas live as
first-class subagents in `.claude/agents/`.

**Two of them cannot write; the third is asked not to.** None
has `Edit` or `Write`. `artisan` and `fresh-reader` have no
shell at all, so for those two "read-only" is a harness fact.
`red-team` needs a shell to run `git show`, and **an agent's
`tools:` field takes tool names, not permission rules** -- so
`Bash` there is unscoped, and the only thing stopping it writing
is the instruction in its own file.

Do not write that it is read-only by construction. It was
written that way once, on the strength of a
`tools: ... Bash(git show:*)` line that looked like scoping and
enforced nothing; the reviewer disproved it by creating a file
and saying so. If you want it enforced, the place is a
`permissions` block in settings, and the way to confirm it is a
refused write -- not a line of frontmatter that parses.

The practical consequence: `red-team` reads files a repo
controls, so a prompt injected through one of them has a shell.
That risk is accepted for now, and it is written down here
rather than papered over.

- **`red-team`** (`.claude/agents/red-team.md`) -- security &
  correctness. Tools: `Read`, `Grep`, `Glob`, `Bash`. It runs
  `git show` and `git log` itself, so it needs the shell. Tell
  it which commit range to review.
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

**An edit to an agent file takes effect on the next session,
not this one.** Claude Code reads `.claude/agents/` at startup,
so a round that just rewrote a reviewer's definition spawns the
old one. Two consequences: a new agent cannot be used in the
session that created it, and a round reviewing its own agent
files is judging them with the previous version's brief. Say so
in the summary when it applies.

## Reviews run after the commit

The target is a **commit**, never the index. `/commit` commits
first, then spawns the reviewers against what it just made.
Fixes from a round land as their own commit, and the next round
reviews that. The reasoning is in `CLAUDE.md` under **Commits
and releases**; the operational consequences are:

- Pass a **commit range**, not a staged diff. The range is
  **the parent of the first commit this run made, up to
  `HEAD`** -- work it out at step 9 rather than assuming
  `HEAD~1..HEAD`, because one `/commit` run may land several
  commits. `HEAD~1..HEAD` is the common case, not the rule.
  Write it `<older>..<newer>` and check it with
  `git rev-list --count` before putting it in a message: a
  reversed range prints nothing and exits 0.
- **Never amend the commit under review.** A reviewer holding
  a SHA must be able to trust it still means what it meant.
- On a fix round, say in the prompt that `HEAD~1` is the commit
  being fixed, so a fix is read against what it was fixing.
  **Do not hand a bare SHA to `artisan` or `fresh-reader`** --
  neither has a shell, so a SHA is a string they cannot
  resolve. Give `artisan` both diffs, and `fresh-reader` the
  union of both commits' changed file paths.
- **Stop when you would not fix anything the round found** --
  every finding deferred or declined. Do not
  chase an empty report. After three rounds, hand what is left
  to the operator rather than starting a fourth.
- The fix commit uses **steps 1-8 of `/commit` only**. A nested
  `/commit` would run its own steps 9-11, so the reviewers
  would fire twice on one SHA and `/retrospect` would run
  mid-cycle.
- **A round that only defers still makes a commit**, carrying
  the backlog files alone. It is the terminal round, so no
  later commit exists to hold them. That commit starts no new
  round: `docs/developer/*-log.md` is exempt under **When to
  run**, being the reviewers' own output rather than prose
  anybody reads to learn the project.

## When to run

Three cases, and every commit is one of them.

**Code.** `.rs`, `.toml`, `.sh`, `.ps1`, a template under
`crates/bombyx/templates/`, or a workflow under `.github/`.
That is the whole list; bombyx is CLI-only, so there is no
frontend here and no `playwright.config.ts`. Run **all three**
reviewers, and never skip them for a "straightforward" change.

**Canon and workflow.** `CLAUDE.md` or anything under
`.claude/**`. Run **all three** as well. This project keeps
its rules in prose, so a stale step number or a rule nobody
can follow is a real defect -- `artisan.md` has a category for
exactly this, and the reviewers have found false claims in
these files that no gate could catch.

**Everything else** -- documentation and prose. Run
`fresh-reader` alone, and only when the commit rewrites
something a newcomer lands on: `README.md`, `docs/*.md`, a
module doc. Two kinds of file are exempt even then, because
their content is not prose anybody reads to learn the project:
the reviewers' own backlogs (`docs/developer/*-log.md`) and
the diary (`docs/developer/DIARY.md`).

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
