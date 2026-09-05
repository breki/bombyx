---
description: Review source-code work with the three reviewers in sequence, each in one lane -- artisan on the code, red-team on what is unsafe or wrong, fresh-reader on the prose. Commits nothing
argument-hint: "[base commit]"
---

Review the change with one reviewer at a time, fixing
what each finds before the next one reads. Like `/review`, this
command **never commits**: it leaves the tree edited, reports
what happened, and the developer decides when the work becomes
a commit.

## What the reviewers read

The argument is the commit the working tree is compared
against, and it defaults to `HEAD`. With the default the
reviewers read what is not committed yet, which is what a run
from the developer's own shell wants.

Pass a base when the work is already committed. `/issue` does
that after it pushes the branch: it passes
`$(git merge-base main HEAD)`, so the diff holds every commit
on the branch plus anything still uncommitted, rather than the
empty diff `HEAD` produces the moment the work is committed.

Call the value `BASE` below. Everything else in this file is
the same either way.

**This command is for a change containing source code** --
`.rs`, `.toml`, `.sh`, `.ps1`, a template under
`crates/bombyx/templates/`, or a workflow under `.github/`.
Send anything else to `/review`, which handles all three of
`code-reviewers.md`'s cases under **When to run**, including
the conditions and exemptions on a prose-only change. Say so
and stop rather than reviewing a canon change here: the lanes
below are defined for code, the stop rule needs a behaviour
defect to turn on, and the first run of this file against a
canon-only change produced eleven findings for that reason.

A change with source code in it usually carries documents and
canon too, and those are reviewed here along with it. It is a
change with *no* code that belongs elsewhere.

## What this file owns

`code-reviewers.md` owns the three reviewers, what each is
handed and how to spawn them.

`/review` owns four things these stages reuse, and each stage
below names the one it needs: **Snapshot**, **Run it before
anyone reads it**, **Log what you defer** and **When it stops
converging**. From **Review, then fix** the stages inherit the
fixing bar, the count-the-copies rule, the escalate list and
the rule that a consolidation is never applied in the round
that found it. They do **not** inherit its round-three rule --
the stop rule below replaces it, and inheriting both would make
the later rounds unreachable.

This file owns the sequence, the lanes and the stop rule.

**Each stage narrows its reviewer to one lane**, and a lane is
a kind of defect rather than a kind of file:

| Stage | Reviewer | Lane |
|-|-|-|
| 1 | `artisan` | is the source code well made |
| 2 | `red-team` | is it unsafe, or is it wrong |
| 3 | `fresh-reader` | can somebody new follow it |

Nothing outside a stage's lane is fixed in that stage. A
reviewer will report outside it anyway -- tell it to say so in
one line and carry the item to the stage that owns it, rather
than dropping it.

**Why one at a time.** `/review` spawns all three at once, so a
defect in three places gets reported three times and fixed
once. On the run this file came from, the stale word
"overrides" was reported by all three reviewers, and the
duplicated test fixture by two. Sequence spends that work once,
and each reviewer reads the previous one's fixes, so a bad fix
is caught inside the run rather than a round later.

**Why this order.** Each stage's fixes are less dangerous than
the one before. `artisan` changes types, tests and signatures;
`red-team` changes logic; `fresh-reader` changes only prose. So
the stage that finds danger runs after the stage that creates
most of it, and the stage that cannot break the program runs
last.

This file carries no `allowed-tools` line, the same as
`/review`. An incomplete list is worse than none: a command
telling the agent to run something its own frontmatter does not
grant simply fails at that step, and two such gaps shipped in
`/issue` before anyone noticed.

## Before any stage

**Snapshot** to `target/review2-1.diff` and
`target/review2-1.files`, by the recipe in `/review` under
**Snapshot**. That pair is stage 1's snapshot, so stage 1 does
not take another; every later one is named
`target/review2-<stage>-<round>.diff` and `.files`. Every
warning in that recipe applies: name the untracked paths
rather than sweeping them, subtract the backlogs, and report
the intent-to-add entries left in the index.

That recipe writes `HEAD` into both of its `git diff` commands.
Put `BASE` there instead, and everything else in it stands:

```bash
OUT=target/review2-1
git diff "$BASE" -- . "$EXCL" > "$OUT.diff"
git diff --name-only --diff-filter=d "$BASE" -- . "$EXCL" \
  > "$OUT.files"
```

**Run the artifacts**, per `/review` under **Run it before
anyone reads it**. Once here, and afterwards only for an
artifact a fix has touched. This is where a walk of a changed
workflow file happens, and it belongs to no single stage --
that walk found three defects in this file before any reviewer
was spawned.

**Re-snapshot before each stage and each round**, to
`target/review2-<stage>-<n>.diff` and `.files`. A stage reads
the previous stage's fixes, so a stale snapshot hides exactly
what the sequence exists to catch. The `.files` list matters as
much as the diff: `fresh-reader` is handed the list, and a file
created by an earlier stage's fix reaches it only if that
re-snapshot ran `git add -N` on it. Every re-snapshot uses the
same `BASE` as the first one.

**Each stage writes its findings** to
`target/review2-<stage>-<n>.findings` before the next spawn,
in the shape `/review` under **Review, then fix** describes.
Not optional: the handover below is the only thing that has
ever detected a loop here, a long session summarises the
earlier stages' messages away, and the previous run's
round-two findings were never written down -- which left a
claim in this very file unverifiable.

## Stage 1 -- `artisan`, once

Spawn `artisan` alone and fix what it finds.

**Ask it for the source code only.** Its `Error Handling &
Messages`, `API Design`, `Abstraction Boundaries`, `Type
Safety` and `Module Size` categories are this stage. Its
`Canon and documentation` category is **not**: tell it that
documentation and canon belong to stages 2 and 3, so a finding
it has there arrives as one line rather than a full entry.

On the run this file came from, artisan's documentation
findings were over half its output, and the later stages
reached the same places. Its code findings are the ones nothing
else produces: a guard covering one field and not its sibling,
a fixture duplicated across crates, a comment stating a
mechanism the code contradicts.

One stage, not a loop. `red-team` reads these fixes next, which
is what a second artisan round would otherwise be for.

## Stage 2 -- `red-team`, until behaviour settles

Each round: re-snapshot, spawn `red-team` alone, fix, write the
findings.

**Hand over every earlier stage's and round's findings** and
ask outright: *is anything here a defect in the fix for an
earlier finding?* A reviewer shown only the current state
cannot see a loop.

**Ask it whether the change is unsafe or wrong.** Its
`Security` and `Correctness` categories, plus `CI/CD`,
`Project Configuration` and **The files bombyx writes onto the
VM host** where the diff reaches them.

Wrong includes wrong prose wherever the prose states a rule or
a fact: a document claiming what the code does not do, a step
nobody can follow, a cross-reference to something absent, a
CHANGELOG recording half a change. Prose that is merely
unclear, badly placed or duplicated is stage 3's, and goes
there as one line.

**A finding whose fix changes what bombyx does gets its failing
test first.** Write it, watch it fail, then fix, per `CLAUDE.md`
under **Test-Driven Development**. The suite was green when the
two defects on the previous run were live: 286 tests, ten
gates, 98.1% coverage. A test asserts the property somebody
thought of, and this stage exists to find the property nobody
thought of. One of that run's findings was itself a missing
test -- disabling the branch that prints the winning host left
every test passing.

A finding whose fix changes only prose gets no test. Asserting
against a document has been tried here and deleted, and
`CLAUDE.md` under **Test-Driven Development** holds why: a test
whose assertions need their own parser is testing the parser.

### What earns another round

A **behaviour defect** is a finding whose fix changes what
bombyx does: what it prints, what commands it emits, what
input it accepts or refuses, what it writes or deletes, or what
a caller of the library can compile against. A finding about a
comment, a document, a test, a name or a record is not one,
however right it is.

**Another round is earned only when the round found a behaviour
defect and we fixed it.** So:

- A round returning only comment, document and test findings
  ends the stage. Fix them, then go to stage 3.
- A round whose behaviour defects were all **deferred or
  declined** also ends it. The tree did not move, so the next
  round reads the same code and returns the same findings. On
  the previous run both behaviour defects were deferred
  deliberately, because the step that deletes the file deletes
  them with it.

**Stop early when earlier fixes are breaking**, and go to
`/review` under **When it stops converging**: more than one
defect in an earlier round's fix, or one landing where an
earlier round already fixed something.

**Three rounds is the ceiling**, the same as `/review`'s cap
and for its reason -- one branch ran five rounds at 60, 42, 36,
37 and 33 findings, which is a flat tail rather than
convergence. Reaching the ceiling means the rule above failed,
so say so rather than reporting the run as finished.

**This stop rule has never been exercised.** It replaced
`/review`'s round-three rule without a run to measure it
against, and the previous version of it could only be satisfied
by a code finding, which is the mirror of a condition reverted
in `bd52dcd` a day earlier. Until a code review has run it,
take a finding against it to a backlog rather than editing it
here -- the discipline `/review` was held to for the same
reason.

## Stage 3 -- `fresh-reader`, prose only

Re-snapshot after stage 2's last fix, and hand it the
changed-file list rather than a diff -- `code-reviewers.md`
under **Diff handoff** says why -- with every earlier stage's
findings, the one-line hand-offs included. Read those before
spawning.

**Its lane is whether somebody new can follow the files.**
Stage 2 already owns prose that is false or unfollowable, so
what is left here is prose that is true and still does not
land: an explanation missing, a term never introduced, an order
that makes the reader work backwards, a comment narrating
history instead of giving the reason.

**Its fixes touch prose that only a person reads.** Code
comments, doc comments, files under `docs/`, `README.md` and
`llms.txt`. Three kinds of prose are outside that, because the
program reads them too:

- **clap `///` help.** It is what `bombyx --help` prints, so
  editing it changes the program's output.
- **`allowed-tools` frontmatter in `.claude/`.** The harness
  executes it.
- **Any doc-comment edit adding, removing or retargeting a
  rustdoc link.** `cargo xtask doc` fails on a link that does
  not resolve, and it failed twice on that account during the
  previous run.

A finding against one of those is real and gets **logged, not
fixed**, per `/review` under **Log what you defer**. Say in the
report that this stage's limit excluded it rather than that
nobody thought it mattered.

`fresh-reader`'s **What worked** section is not a finding and
needs no action. Carry it into the report anyway, so the
passages it named are known to carry a reason the next time
somebody trims comments.

## Reporting

Per stage: findings raised, fixed, logged, escalated -- and for
stage 2 the round count with the reason it stopped, naming the
behaviour defect that earned the last round or saying that none
did.

Then the closing account `/review` under **Reporting** asks
for: every artifact with its label, each non-converging area,
and what the run left behind for the developer's next commit.
Nothing was committed.

## Rules

- **Never commit, never amend, never push.**
- Source-code changes only. Send a prose-or-canon-only change
  to `/review`.
- One reviewer at a time. Spawning two defeats the point.
- Each stage asks for one lane. `artisan` is never asked about
  documentation.
- A finding outside the current stage's lane is carried to the
  stage that owns it, never dropped and never fixed early.
- Re-snapshot before every stage and round, and write each
  stage's findings before the next spawn.
- Never skip the artifact run because the change obviously
  works.
- A behaviour defect that was deferred does not earn another
  round.
- `fresh-reader` never edits clap help, `allowed-tools`, or a
  rustdoc link.
- Surface every finding, whatever its disposition. Never drop
  one silently.
