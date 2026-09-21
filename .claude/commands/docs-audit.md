---
description: Read-only audit of the whole documentation corpus -- rate each file and recommend keep, trim, or delete
allowed-tools: Bash(find:*), Bash(wc:*), Bash(ls:*), Bash(grep:*), Read, Grep, Glob, Agent, AskUserQuestion, Skill(html-report)
---

Audit the documentation as a corpus, not a diff: read every doc
file, rate each one, and recommend keep, trim, merge, or delete.

This is a **reporting** command. It changes nothing -- no edits,
no commits, no gates. Its whole value is that it can be trusted to
describe the docs rather than tidy them up. Acting on a finding is
a separate step (`/todo`, `/implement`, or a plain edit).

It fills the gap between the two review paths that already exist:
`/review` reads a code diff and `blue-pencil` edits changed prose
line by line. Neither judges the standing documentation as a whole
-- which files earn their keep, which duplicate each other, which
mislead an AI agent. That is this command.

## Usage

```
/docs-audit                 # the whole documentation set
/docs-audit docs/developer  # a subdirectory or glob only
/docs-audit --no-report     # skip the HTML report, print inline only
```

Every run prints the report inline **and** renders it through
`/html-report`, so the operator always gets a saved page to keep.
Pass `--no-report` when you want the inline text alone.

## The documentation set

Unless the argument narrows it, the set is:

- `README.md`, `CLAUDE.md`
- everything under `docs/`

`.claude/` command, skill and agent prose is canon too; include it
only when the argument asks for it, since it is reviewed under a
different discipline.

## Instructions

### 1. Inventory first

Enumerate the set with line counts, largest first, and say how
many files and how many lines are in range before rating anything.
`find ... -name '*.md'` and `wc -l` are enough; this step is
mechanical and needs no judgment.

A file over a few hundred lines is a signal in itself: length is
what forces a reader to sample rather than read, so note the big
ones -- they carry the highest AI-agent risk before a word is read.

### 2. Fan out read-only assessors

Group the files into three to six balanced groups and spawn one
`Agent` per group (`subagent_type` general-purpose), in a single
message so they run at once. Give every assessor the **same
rubric** (below) so the ratings are comparable.

Two rules for the assessors, stated in their prompt:

- **Read-only.** They rate; they do not edit.
- **Return the rating, not the file.** They must not dump file
  contents back -- the point of the fan-out is that the tokens of
  the corpus stay in the subagents. A very large file (a diary, a
  5000-line log) is *sampled*, not read whole; say so in its
  prompt.

### 3. The rubric -- one block per file

- **path**
- **purpose** -- one line
- **audience** -- human newcomer / operator / AI agent / maintainer
- **usefulness** -- High / Medium / Low
- **mannered prose** -- None / Some / Heavy. Mannered means ornate,
  aphoristic, anecdotal, self-referential prose ("what prompted
  writing this down", war-stories, a precious phrase) where a plain
  subject-verb-object sentence would carry the same instruction.
  `CLAUDE.md` under **Writing** is the standard the reviewers judge
  against.
- **too deep** -- Yes / No, with a short note on what over-explains
- **redundant with** -- the other files it overlaps, or none
- **AI-agent risk** -- how it could mislead or burden an agent
  working from it: length forcing partial reads, a binding rule
  buried in anecdote, a stale claim beside a current one, a
  superseded design read as the shipped one, a dangling reference
- **verdict** -- Keep / Trim / Merge into <file> / Archive / Delete
- **rationale** -- one or two sentences
- **mannered quote** -- one representative short verbatim quote, or
  none

Each assessor ends with a 3-4 sentence overall for its group: what
to keep, what to cut, and the single biggest problem.

### 4. Compile the report

Synthesise the assessor output into one report. It has four parts:

1. **The verdict** -- the one-paragraph recommendation, and the
   tally (how many Keep / Trim / Merge / Archive / Delete).
2. **The systemic problems** -- the patterns that cross files:
   mannered prose, duplication with no single owner (name the
   drifted copies), stale records read as current, length forcing
   partial reads. This is the part that survives; a per-file list
   the operator can get from the tables.
3. **The per-file tables** -- one row per file: usefulness,
   mannered, verdict, and the AI-agent-risk note.
4. **Models and worst offenders** -- name the two or three files
   that are the house style to copy, and the two or three that are
   actively misleading and should be fixed first.

Print the report inline, then render it through `/html-report` so
the operator keeps a saved page as well. Skip the render only when
the argument is `--no-report`.

## Rules

- **Read-only.** Never edit, commit, or run a gate. If a finding
  should be recorded, say so and let the operator send you to
  `/todo`; do not capture it yourself.
- **Recommend, do not survey.** Every file gets a verdict, and the
  report opens with one recommendation, not a menu. An audit that
  refuses to say what to cut has done half the job.
- **The AI-agent lens is the one that distinguishes this command.**
  Human readability matters, but the question that earns the audit
  is whether a file makes things worse for an agent that must work
  from it -- so weight length, buried rules, and stale-beside-live
  facts heavily.
- **Length is not a fault on its own.** A long document a reader
  follows is not a defect; flag density, duplication and staleness,
  not word count. Say which one a "too deep" verdict rests on.
- **Do not invent coverage.** If a durable fact would be lost by a
  Delete verdict, check where else it lives before recommending the
  delete, and say where it is covered -- a missing home is a reason
  to Trim or promote, not Delete.
- Voice rules from `CLAUDE.md` apply to the report itself: a docs
  audit written in mannered prose has failed its own test.
