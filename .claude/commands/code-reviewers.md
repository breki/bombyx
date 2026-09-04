# Code reviewers (which run, and how)

Three reviewers are available to `/review`. Their personas live
as first-class subagents in `.claude/agents/`.

**Two of them cannot write; the third is asked not to.** None
has `Edit` or `Write`. `artisan` and `fresh-reader` have no
shell at all, so for those two "read-only" is a harness fact.
`red-team` needs a shell to run `git log`, and **an agent's
`tools:` field takes tool names, not permission rules** -- so
`Bash` there is unscoped, and the only thing stopping it writing
is the instruction in its own file.

So do not write that `red-team` is read-only by construction.
A `tools:` entry that looks scoped is not one. Enforcement
lives in a `permissions` block in settings, and the only proof
is a refused write -- never a line of frontmatter that parses.

The practical consequence: `red-team` reads files a repo
controls, so a prompt injected through one of them has a shell.
That risk is accepted for now, and it is written down here
rather than papered over.

- **`red-team`** (`.claude/agents/red-team.md`) -- security &
  correctness. Tools: `Read`, `Grep`, `Glob`, `Bash`. It runs
  `git log` itself for the surrounding history, so it needs the
  shell. See **Diff handoff** below for what it is given.
- **`artisan`** (`.claude/agents/artisan.md`) -- code quality &
  craftsmanship beyond clippy. Tools: `Read, Grep, Glob` only --
  no shell, so it is read-only by construction. Pass it the
  snapshot path, and say the path names a file to read -- see
  **Diff handoff** below.
- **`fresh-reader`** (`.claude/agents/fresh-reader.md`) --
  comprehension. Tools: `Read, Grep, Glob` only. It reads the
  changed files **whole**, not as a diff, because it is judging
  what a newcomer lands on rather than what moved. See
  **Diff handoff** below for what it is given.

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
files above.

Three commands read this file. `/review` spawns the three
reviewers together and reviews the tree in rounds; `/review2`
runs them one at a time, each reading the previous one's fixes;
`/issue` offers a review at its step 8 and points here for the
rest. All three take the reviewers, the handoff and the
reading-back checks from here, and each owns its own loop.

`/review` asks each reviewer for every category its agent file
lists. `/review2` asks each for one of them and carries the
rest to the stage that owns it -- that narrowing is written in
`/review2`, because it is a property of that sequence rather
than of the reviewers. The criteria stay in the agent files
either way.

**An edit to an agent file takes effect on the next session,
not this one.** Claude Code reads `.claude/agents/` at startup,
so a round that just rewrote a reviewer's definition spawns the
old one. Two consequences: a new agent cannot be used in the
session that created it, and a round reviewing its own agent
files is judging them with the previous version's brief. Say so
in the summary when it applies.

## The target a round reviews

A **snapshot** file that `/review` writes. `CLAUDE.md` under
**Reviewing is its own process** argues why the target must not
move, and `/review` under **Snapshot** specifies how it writes
one. Neither argument is repeated here. What each reviewer is
handed is in **Diff handoff** below.

Never commit, amend or push. The calling command owns the rest
of its loop, including what a later round or stage hands over.

`red-team.md` repeats the no-commit rule for itself, because
that agent has a shell and never reads this file. The other two
have no shell, so the harness settles it for them.

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
`fresh-reader` alone, and only when the changed files include
something a newcomer lands on: `README.md`, `docs/*.md`, a
module doc. Two kinds of file are exempt even then, because
their content is not prose anybody reads to learn the project:
the reviewers' own backlogs (`docs/developer/*-log.md`) and the
diary (`docs/developer/DIARY.md`). `/review` under **Snapshot**
already subtracts the backlogs. It does not subtract
the diary, because `/commit` writes that after `/review` has
finished, so a diary edit reaches a snapshot only when one is
already sitting in the tree.

## How to spawn

**When to run** above specifies which reviewers a change
needs. Spawn those in a **single parallel message** -- one
`Agent` call per reviewer -- so they run concurrently. The
three names are:

- `subagent_type: red-team`
- `subagent_type: artisan`
- `subagent_type: fresh-reader`

Give each spawn:

1. A one-line description of what the change does.
2. What **Diff handoff** below says to give that reviewer. It
   differs per reviewer, and getting it wrong defeats a
   persona rather than merely inconveniencing it.
3. Nothing about the report format. Each agent file states its
   own, and they differ on purpose: `red-team` emits `RT-<n>`
   with a trigger, `artisan` emits `AQ-<n>` with a better
   approach, and `fresh-reader` emits `FR-<n>` with **Where it
   left me**, which is the field the other two cannot produce.
   Asking for a shape the agent file does not specify is how a
   report comes back without the IDs `/review` needs to cite
   when it reports what it fixed.

**Diff handoff.** `red-team` and `artisan` are both handed the
snapshot path `/review` writes under **Snapshot**, and are told
the tree may have moved since. `artisan` has no shell, so say
plainly that the path names a file to read; it cannot work that
out for itself. **Never `/tmp`**: under Git Bash on Windows
that resolves outside the workspace, where the operator cannot
see it.

`fresh-reader` is handed the changed-file list instead, and
reads those files as they stand. That persona judges the
finished file, so pointing it at a diff defeats the point of
having it.

Each agent's final message *is* its report (a plain-text finding
list, or "No issues found."), consumed by `/review` -- not
shown to the user directly.

## Reading the reports back

Run two checks on the reports before acting on them.

**A reply can arrive with a finding's body missing.** Subagent
replies are occasionally truncated, and the tell is an ID that
appears in a summary line or a cross-reference but has none of
the fields its agent file specifies. A reply ending "RT-1
(permission globs), RT-2 (test robustness)" with no body for
RT-2 has lost RT-2. Message that agent again -- its ID is in
the tool result -- and ask it to re-emit the missing findings
verbatim, in the same structure. If you cannot reach it, spawn
that reviewer again with the same prompt and the IDs whose
bodies are missing, asking for only those. Do this before
presenting anything, because a dropped finding is one the
reviewer did raise and nobody will notice is gone.

**Two reviewers reaching one defect is the strongest signal
there is.** They count as the same finding when either holds:

- they name the same `file:line`, or overlapping line ranges in
  the same file; or
- they describe one defect in different vocabulary. `red-team`
  reporting "TOCTOU on `is_dir` then `remove_dir_all`" and
  `artisan` reporting "follows symlinks during deletion despite
  `dir_size`'s guard" are one finding about one line.

Say which reviewers agreed when reporting such a finding. A
`fresh-reader` finding pairing with one of the other two carries
extra weight: the defect is real *and* invisible from the files,
so fixing the code without fixing what it says leaves the next
reader where the last one stopped.
