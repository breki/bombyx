---
description: Log feedback for the rustbase template, or sweep commits for it
allowed-tools: Read, Write, Grep, AskUserQuestion, Bash(cargo xtask feedback-add:*), Bash(git log:*), Bash(git status:*), Bash(git diff:*), Bash(git show:*)
---

Log observations about the rustbase template in
`docs/developer/template-feedback.md`.

Entry placement, date stamping, ID minting, and dedup are
owned by `cargo xtask feedback-add` -- do **not** hand-edit
the file. Your job is the judgement: which observations are
about the *template*, the section, a short title, and the
body prose.

## Usage

```
/template-improve                 # sweep commits for candidates
/template-improve <observation>   # log this one thing
```

With an argument, log that observation and stop -- skip to
**Writing an entry**.

With no argument, **sweep**: work through every commit since
the feedback file was last updated and find what belongs in
it. Do not ask the user what they noticed. Asking wastes the
one thing this command is good at, which is that the commits
remember more than the operator does.

## Sweeping

### 1. Establish the boundary

```
git log -1 --format=%h -- docs/developer/template-feedback.md
git log <boundary>..HEAD --oneline
git status --short
```

The boundary is the last commit that touched the feedback
file. Say which commit it is and how many commits are in
range before listing anything.

**If the feedback file has uncommitted changes, say so.**
Entries added but not yet committed mean some commits in
range are already covered, and the sweep will otherwise
offer them again.

**Then read the whole working tree, not just that file.** A
`git log` boundary cannot see work that is not committed
yet, and the session running this command is exactly the one
most likely to have some. The first time this sweep ran it
missed the change sitting in its own editor -- the operator
had to point at it. So `git status --short` unqualified, and
`git diff` over anything template-provided that it lists.

A sweep that finds nothing does not move the boundary, so
the next run re-reads the same range. That is correct but
wasteful; if it becomes a nuisance the fix is a watermark of
the kind `/template-backfeed` keeps, which is `xtask` work
and not a change to this file.

### 2. Read more than the commit messages

A commit subject rarely says "this was the template's
fault". The content that does is in four places, and all
four are in range:

- **Commit bodies.** This project writes why, not just
  what, so a body often names the surprise directly.
- **`docs/developer/DIARY.md`.** The entries record what
  cost an hour and why, which is exactly the asymmetry
  template feedback wants.
- **`docs/developer/redteam-log.md` and `artisan-log.md`.**
  A deferred finding against template-provided code *is*
  template feedback, and it is already written up.
- **The diffs**, where a message is thin.

### 3. Decide what is the template's problem

This is the whole judgement, and a sweep that gets it wrong
fills the file with noise. Ask three questions of each
candidate:

1. **Does it live in a file the template provides?**
   `xtask/`, `.claude/`, the `CLAUDE.md` scaffolding,
   root config (`clippy.toml`, `rustfmt.toml`,
   `rust-toolchain.toml`, `Cargo.toml` lint blocks),
   `build.ps1`, `scripts/` wrappers. A defect in this
   project's own modules is not template feedback however
   painful it was. Check `.template-sync.toml` for the
   origin when unsure.
2. **Would another project generated from the template hit
   it?** If the answer needs this project's VM host, its
   config shape or its domain, the answer is no.
3. **Was it a surprise?** The valuable entries are the
   misleading default, the gate that does not gate, the
   dangling reference, the boilerplate every project
   deletes, the feature every project adds. A thing that
   behaved exactly as documented is not feedback.

Do not log: defects in project-specific code, anything
already in the file, one-off environment quirks (a stale
process, a machine without a tool), or a preference with no
argument behind it.

### 4. Check for duplicates before offering anything

`feedback-add` dedups by **ID**, which catches only an
identical title. A differently-worded duplicate goes
straight in. So grep the file for the subject first, and
grep the two backlogs too -- an item may already be recorded
there as deferred project work rather than as template
feedback.

### 5. Offer the candidates, then log the chosen ones

Present each candidate in one line: what it is, which
template file it lives in, and which section it would go
to. Then ask with `AskUserQuestion` which to log --
multi-select, since a sweep normally turns up several. Never
log a swept candidate without asking; the operator knows
which of them they consider the template's business.

If the sweep finds nothing, say so plainly and stop. A
sweep that manufactures an entry to look productive is
worse than a quiet one.

## Writing an entry

Decide which lifecycle section it belongs in. The file has
three (read the file header for the full semantics):

- **Open divergences** (`--section open`) -- something this
  project knows is suboptimal or differently-shaped than
  the template baseline, and still carries. A pending or
  intentional difference.
- **Resolved** (`--section resolved`) -- closed out here by
  a retrofit or fix commit. The entry records what was
  wrong and how it was closed.
- **Suggestions to flow back** (`--section suggestion`) --
  an idea to push upstream. **This is where a thing this
  project already fixed for itself goes**, because the
  template still has it. Do not file that as `resolved`:
  resolved means the divergence is gone, not that the
  problem is.

Default to **open** when the section is genuinely unclear.

Choose a short title (a few words -- it drives the ID slug)
and write the body prose, wrapped at 80 characters. The body
explains the issue, why it matters, and the suggested fix;
for a **resolved** entry, end with a one-line summary of the
fix.

**Record what was measured, not what was assumed.** The
value of an entry to the template is the fact it does not
have to rediscover -- the exact error text, the flag that
turned out to be required, the value a variable actually
held. An entry that says "this seems fragile" saves nobody
anything. Where a claim rests on an argument rather than a
measurement, say which.

Cite a commit SHA, a backlog ID or a file and line when the
detail lives somewhere else, so the entry is a pointer as
well as a description.

## Adding it

Write the body to a temp file (use the scratchpad, not the
repo), then:

```
cargo xtask feedback-add --section <open|resolved|suggestion> \
  --title "<short title>" --body-file <tmp>
```

The command mints a `tf-<yyyy-mm-dd>-<slug>` ID, inserts the
entry at the top of the chosen section (newest first), and
skips silently if that ID is already present. The body may
be piped on stdin instead of `--body-file`.

Delete the temp files afterwards.

## Rules

- Never hand-edit `docs/developer/template-feedback.md`.
  Placement, dating, ID minting and dedup belong to
  `feedback-add`; a hand edit puts an entry in the wrong
  section or with a colliding ID.
- Do NOT commit. Entries ride along in the next `/commit`.
- Report each ID the command minted, and say plainly which
  candidates you offered and the operator declined -- a
  declined candidate is a decision, not a gap.
- The `### tf-…` headings the appender generates exceed 80
  columns. That is the appender's format, not drift; leave
  them alone.
