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
  it which snapshot file to review.
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
files above. `/review` is the only caller: `/commit` does no
reviewing, and the two are independent processes.

**An edit to an agent file takes effect on the next session,
not this one.** Claude Code reads `.claude/agents/` at startup,
so a round that just rewrote a reviewer's definition spawns the
old one. Two consequences: a new agent cannot be used in the
session that created it, and a round reviewing its own agent
files is judging them with the previous version's brief. Say so
in the summary when it applies.

## The target a round reviews

A **snapshot** of the working diff, written to a file under
`target/`. Never the index, and never the live tree: a named
file does not move while the reviewers read it, and this repo
has already had a reviewer report against a tree that no longer
compiled because fixes had landed underneath it.

- Pass the **snapshot path** to `red-team` and `artisan`, and
  tell them the tree may have moved since. Pass `fresh-reader`
  the changed-file list, and let it read those files as they
  stand.
- On a later round, hand over the earlier rounds' findings in
  the prompt.
- **Never commit, amend or push.** `/review` owns the rest of
  its loop; see that file.

## When to run

Three cases, and every change under review is one of them.
Judge the change as a whole, not file by file.

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
2. The snapshot path from step 1 of `/review`. `artisan` has
   no shell, so tell it the path is a file to read. For `fresh-reader`, the **list of changed file
   paths** and nothing else -- it reads the files whole, and
   handing it a diff defeats the point of the persona.
3. Nothing about the report format. Each agent file states its
   own, and they differ on purpose: `red-team` emits `RT-<n>`
   with a trigger, `artisan` emits `AQ-<n>` with a better
   approach, and `fresh-reader` emits `FR-<n>` with **Where it
   left me**, which is the field the other two cannot produce.
   Asking for a shape the agent file does not specify is how a
   report comes back without the IDs `/review` needs to cite
   when it reports what it fixed.

**Diff handoff.** Both `red-team` and `artisan` are handed the
snapshot path from step 1 of `/review`. `artisan` has no shell,
so say plainly that the path names a file to read; it cannot
work that out for itself. **Never `/tmp`**: under Git Bash on
Windows that resolves outside the workspace, where the operator
cannot see it.

Each agent's final message *is* its report (a plain-text finding
list, or "No issues found."), consumed by the calling skill --
not shown to the user directly.
