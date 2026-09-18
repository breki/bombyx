---
description: Write a handoff document for a fresh session -- branch and push state, what shipped, standing decisions, and what is open and unverified
allowed-tools: Bash(git status:*), Bash(git log:*), Bash(git branch:*), Bash(git diff:*), Bash(cargo xtask todo:*), Read, Glob, Write
argument-hint: "[output path]"
---

Write a self-contained document a fresh session can read to pick
up this one's work, then stop. The next session has none of this
session's transcript, so the document is the only bridge -- it
must stand on its own.

This is a **reporting** command with a single write, the document
itself. It makes no other edit, no commit, and runs no gate.

## Usage

```
/handoff                       # write bombyx-handoff.md in your home dir
/handoff <path>                # write it somewhere else
```

The default output is `bombyx-handoff.md` in your home directory
(`~` on Linux and macOS, `%USERPROFILE%` on Windows) -- outside the
repository, so the handoff is never placed under source control.

## Instructions

### 1. Read the state, do not recall it

Run these and read them, in the same breath as any claim built on
them:

- `git branch --show-current` and `git status -sb` -- the branch,
  and whether it is ahead of or behind its upstream. Whether the
  commits are pushed is the first thing the next session needs.
- `git status --short` -- uncommitted and untracked files.
- `git log --oneline @{upstream}..HEAD` (or against the commit
  this session started from) -- the commits this session made.
- Glob `docs/issues/` -- a working doc left there is work in
  flight, and the next session should finish or remove it.

Say plainly whether this is a git worktree and where it lives; a
fresh session started in the main checkout will not be in it.

### 2. Gather from git, the tree, and the transcript

The transcript is the half git cannot show and the next session
cannot see, so it is the half that matters most. Walk it and
collect what a diff does not record: measurements and what they
showed, actions with no local diff (pushes, tags, remote
commands), designs tried and dropped, decisions the operator made
and why, and anything verified against -- or only asserted about
-- a real VM host.

### 3. Write the document

Write the sections below to the output path. Fill each from the
state and the transcript; when a section is truly empty, say so
rather than dropping it silently.

- **Where things are** -- branch, push state, clean or dirty,
  worktree path, and any working doc still under `docs/issues/`.
  The next session acts on this first.
- **What shipped** -- grouped by subject, each with the decision
  or measurement the commit message alone does not carry.
- **Standing decisions** -- durable choices this session made that
  the next session must honour, each with where it is recorded (a
  file, a memory note) so it is not re-litigated.
- **Open and next** -- the backlogs and the queue
  (`cargo xtask todo list`), and any ready action, naming where
  each lives.
- **Not verified** -- what rests on a dry run, a unit test, or an
  argument rather than a real run against the VM host (Definition
  of Done item 3). An assumed item must not read as a confirmed
  one.
- **Pointers** -- reports, artifacts, and files that live outside
  the tree (a Desktop report, a scratch file).

### 4. Hand it over

Print the output path and one line: start the new session, have it
read the file, and delete the file once it has. Keep it outside the
repository (the default home-directory path does this): the handoff
is throwaway session state, not a durable record, and must not land
under source control.

## Rules

- **Self-contained.** A reader with no transcript must understand
  every line; spell out any session-local shorthand.
- **Facts from the commands, not memory.** A branch name, a push
  state, a commit list: read them, do not recall them.
- **Mark what is unverified**, the way `/rundown` does -- an
  asserted claim and a measured one must not look alike.
- **Do not invent an outcome.** A background job that never
  reported, or a remote command whose status was piped away, is
  unknown, not passed.
- **Change nothing but the document.** No commit, no gate, no edit
  to the backlogs; if the handoff surfaces something durable, name
  it under **Open and next** and let the operator record it.
- Voice rules from `CLAUDE.md` apply.
