---
description: Grouped one-line rundown of work done, ending with what is open for the operator
allowed-tools: Bash(git status:*), Bash(git diff:*), Bash(git log:*), Bash(git tag:*), Read, Grep
---

Report what has been done, grouped by theme, one line per item,
ending with what is left for the operator to decide or do.

This is a **reporting** command. It changes nothing: no edits, no
commits, no gates. Its whole value is that it can be trusted to
describe the session rather than tidy it up.

## Usage

```
/rundown              # since the last commit that predates this session
/rundown HEAD         # uncommitted work only
/rundown <ref>        # since <ref>
```

## Instructions

### 1. Establish the boundary

With no argument, the boundary is the most recent commit **not
made during this session**. Commits the session itself made are
part of the work and belong in the rundown.

Run `git log --oneline -10` and `git status --short` to place it.
Say which boundary you used in one line before the list, because
"since the last commit" is ambiguous the moment the session has
committed something.

### 2. Gather from three sources, not one

- **Commits in range** -- `git log`, and the diffs where the
  message is thin.
- **The working tree** -- `git status --short`, `git diff`.
- **The transcript** -- and this is the one that matters most.

**A rundown assembled from git alone will miss half the work.**
Nothing in a diff records that a repository was audited, that a
claim was measured on a live host, that a force-push re-triggered
CI, that a design was tried and abandoned when a fact came in, or
that a question was put to the operator and answered. Those are
things done. Walk the session and collect them.

Include, specifically:

- Measurements and what they showed, especially where they
  contradicted an earlier assumption.
- Actions with no local diff: pushes, force-pushes, tags,
  published releases, remote commands, machine changes.
- Designs proposed and dropped, with the fact that killed them.
- Review findings fixed, and findings deliberately deferred.

### 3. Group by theme

Three to seven groups. Group by *what the work was about*, not by
the order it happened in -- chronology belongs inside a group, if
anywhere. Give each group a short heading that names its subject.

Number items `N.M` so the operator can refer back to one
("apply 5.2, skip 5.3"). That referencing is the reason for the
numbering, so do not renumber a list you are extending.

### 4. Close with "Open for you"

The last group is always this one, and it is the point of the
command. Split it three ways:

- **Decisions** -- things only the operator can settle. Name the
  options in the same line, briefly.
- **Pending actions** -- work that is ready and simply not done.
- **Loose ends** -- things that will rot if nobody looks: an
  unverified claim, a backlog past its threshold, a file living
  only in a temp directory, a capability documented but not built.

Only list what the operator can act on. "Be aware of X" is not an
open item; either it needs a decision, an action, or it is not on
the list.

## Output shape

```
Boundary: since <ref> (<why>).

**1. <group heading>**
- 1.1 <one line>
- 1.2 <one line>

**2. <group heading>**
- 2.1 <one line>

...

**N. Open for you**

*Decisions*
- N.1 <one line naming the choice>

*Pending actions*
- N.2 <one line>

*Loose ends*
- N.3 <one line>
```

## Rules

- **One line per item.** No sub-bullets, no second sentence, no
  trailing rationale clause. If an item cannot be said in a line,
  it is two items.
- **Include what went wrong.** A rundown that reads as a success
  report is not a rundown. Defects found in your own work,
  assumptions that turned out false, and steps that failed all go
  in, at the same weight as the things that worked.
- **Mark what is unverified.** If a claim rests on a dry run, a
  unit test, or an argument rather than a measurement against the
  real thing, say so in the line. Do not let a verified item and
  an assumed one look alike.
- **Do not invent.** If the outcome of something is unknown --
  a background job that never reported, a remote command whose
  status was piped away -- say it is unknown rather than assuming
  it passed.
- **No filler items.** "Ran the tests" belongs in a rundown only
  when the result is interesting. A green gate is one line at the
  end, not one per gate.
- **Never edit anything**, including the backlogs and the diary.
  If the rundown surfaces something that should be recorded
  durably, put it under *Loose ends* and let the operator send you
  to `/todo` or `/commit`.
- Voice rules from `CLAUDE.md` apply, and bite hardest here: these
  are one-liners, so every word that is not carrying information
  is costing the reader.
