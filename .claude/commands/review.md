---
description: Review the work with the three reviewers in sequence, each in one lane -- artisan on the code, red-team on what is unsafe or wrong, fresh-reader on the prose. Commits nothing
argument-hint: "[base commit]"
---

Review the change with one reviewer at a time, fixing what each
finds before the next one reads. **This command never commits.**
It leaves the tree edited, reports what happened, and the
developer decides when the work becomes a commit. Nothing
requires this command; `CLAUDE.md` under **Reviewing is its own
process** says why.

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

## Which stages run

**A change containing source code runs all three stages.**
Source code is `.rs`, `.toml`, `.sh`, `.ps1`, a template under
`crates/bombyx/templates/`, or a workflow under `.github/`.
Such a change usually carries documents and canon along with
it, and those are reviewed here too.

**A change with no source code in it runs stage 3 alone.**
Hand `fresh-reader` the changed files and skip stages 1 and 2.
`code-reviewers.md` under **When to run** holds which files
count as prose and which two are exempt even then.

The reason for skipping the first two stages is measured. The
first run of this sequence against a canon-only change produced
eleven findings, because `artisan`'s and `red-team`'s lanes are
defined for code, and stage 2's stop rule needs a behaviour
defect to turn on -- a canon change produces none. That costs
us `red-team`'s eye for a document stating a rule the code does
not follow. `cargo xtask canon-check` catches part of that
class, and the rest is a gap we accept.

## What this file owns

`code-reviewers.md` owns the three reviewers, what each is
handed and how to spawn them. This file owns the sequence, the
lanes, the fixing rules and the stop rule.

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

**Why one at a time.** Spawning the three together makes a
defect in three places get reported three times and fixed once.
On the overlay-removal run, `5d4db0c` and `f827993`, the stale
word "overrides" was reported by all three reviewers, and the
duplicated test fixture by two. Sequence spends that work once,
and each reviewer reads the previous one's fixes, so a bad fix
is caught inside the run rather than a round later.

**Why this order.** Each stage's fixes are less dangerous than
the one before. `artisan` changes types, tests and signatures;
`red-team` changes logic; `fresh-reader` changes only prose. So
the stage that finds danger runs after the stage that creates
most of it, and the stage that cannot break the program runs
last.

This file carries no `allowed-tools` line on purpose. An
incomplete list is worse than none: a command telling the agent
to run something its own frontmatter does not grant simply
fails at that step, and two such gaps shipped in `/issue`
before anyone noticed.

## Before any stage

### Snapshot

Reviewers must read one immutable text; `CLAUDE.md` under
**Reviewing is its own process** gives the reason a live tree
is not one.

```bash
mkdir -p target
git ls-files --others --exclude-standard  # untracked: check first
git add -N <the untracked paths of this work>
EXCL=':(exclude)docs/developer/*-log.md'
OUT=target/review-1
git diff "$BASE" -- . "$EXCL" > "$OUT.diff"
git diff --name-only --diff-filter=d "$BASE" -- . "$EXCL" \
  > "$OUT.files"
```

That pair is stage 1's snapshot, so stage 1 does not take
another. Every later one is named
`target/review-<stage>-<round>.diff` and `.files`, and every
one of them uses the same `BASE`.

`git add -N` records a path in the index without its contents,
which is what makes `git diff` report an untracked file at all.
Name the paths; `git add -N .` sweeps in every untracked file,
and the developer's scratch notes then reach the reviewers and
land in their next `git commit -a`.

`:(exclude)<pattern>` is a git pathspec that subtracts matches
from the paths before it, which is why `.` comes first. The
backlogs are subtracted because the stages write to them, and
left in they would hand each round the previous round's own
report to find defects in. `--diff-filter=d` drops deleted
paths, which `fresh-reader` can only fail to open.

The index keeps the intent-to-add entries. Report that, and
report the undo with it: `git reset -- <the paths added with
-N>`. Until then any `git commit -a` commits those paths, even
the ones the run concluded should not ship. `/commit` stages by
name, so it will not sweep them, which is why they can sit in
the index unnoticed.

### Run it before anyone reads it

An **artifact** here is one runnable thing the change produced
or touched: a build, a sample config, a quoted command, a
document somebody can follow. Label each one on its own. A
change touching `plan.rs` and `docs/vm-host-setup.md` has two
artifacts, not one.

Reading cannot find "this step assumes a remote no earlier step
created". Six rounds of reading `docs/tutorial.md` each found
the previous reading's blind spot; one run would have found all
of them.

Record every artifact as **run**, **could not run**, or **must
not run**. A run that fails is a finding, with the same
treatment as a reviewer's; a gate that aborts the pipeline
leaves the gates behind it *could not run*.

- **A code change** -- `cargo xtask validate --check`, so the
  formatter cannot rewrite the tree the snapshot just captured.
  When the change touches the commands bombyx emits, Definition
  of Done item 3 applies: the real run against the VM host, and
  *could not run* when the host is unreachable. `--dry-run`
  proves the argv and nothing else.
- **A sample config, a quoted command, a transcript** --
  execute it and compare. Do not eyeball it.
- **A procedure document** -- follow it from the state it
  names, unless following it mutates something you do not own.
  `docs/vm-host-setup.md` provisions a shared host as root:
  that is *must not run*, and a dry run or a disposable target
  is the substitute.
- **A workflow file** (`.claude/**`, `CLAUDE.md`) -- walk it
  against the current tree without spawning anything. An edited
  agent file only takes effect next session, so record that
  part as *could not run*.

Run the artifacts once here, and afterwards only for an
artifact a fix has touched. The walk of a changed workflow file
belongs to no single stage -- that walk found three defects in
this file before any reviewer was spawned.

A failure recorded here is normally left for the stage that
owns it. Some failures cannot be recorded without a fix first
-- a sample config that will not load, a quoted command with a
typo in it. **If you fix anything while running the artifacts,
write the snapshot again before spawning anyone.** Overwrite
the same name: a fix made here is the one edit no reviewer has
seen, so it is exactly the one they must be shown.

### Re-snapshot before each stage and each round

Write `target/review-<stage>-<n>.diff` and `.files`. A stage
reads the previous stage's fixes, so a stale snapshot hides
exactly what the sequence exists to catch. The `.files` list
matters as much as the diff: `fresh-reader` is handed the list,
and a file created by an earlier stage's fix reaches it only if
that re-snapshot ran `git add -N` on it.

### Each stage writes its findings

Write them to `target/review-<stage>-<n>.findings` before the
next spawn. One line per finding: its ID, the reviewer, the
category, the `file:line` it named, one sentence of what it
said, and its disposition.

Record the finding itself, not just its label. The consumer is
the handover the next stage makes, and a reviewer handed `AQ-7
| artisan | fixed` cannot tell whether a new finding is a
defect in that fix. Long sessions get their earlier messages
summarised away, and the round-one findings go with them, so a
file on disk survives where the conversation does not. Note
that `target/` is not committed, so anything that must outlive
the run goes in a backlog under **Log what you defer**.

This is not optional: the handover is the only thing that has
ever detected a loop here, and one run's round-two findings
were never written down, which left a claim in this very file
unverifiable.

## Fixing what a stage finds

Every stage fixes under these rules.

Check the replies before acting on them --
`code-reviewers.md` under **Reading the reports back** covers a
truncated reply and the test for two reviewers reaching one
defect.

**Count the copies first.** Say a rule is written in four
places. A round finds three, the fix corrects those three, and
the next round finds the fourth. That is the largest single
source of rounds that never end. So on a finding of the shape
"X is wrong in F", find every place that says X before touching
one.

**Two prose copies are allowed; a third is the defect.** What
counts is a copy that states the rule. A copy whose job is
helping a reader *find* the rule does not count -- a one-line
`description` in frontmatter, a row in a skills table, a clap
`///` help line -- though it must still agree with the rule it
summarizes, and a summary that contradicts it is a false claim
about the command. Above two, the repair is one authoritative
statement and pointers to it -- but do not make that repair
here. See the next paragraph.

**A consolidation is escalated, never applied in the round that
found it.** Collapsing N copies to one owner does not remove
prose, it converts it: N-1 pointers appear, and a pointer can
name the wrong section, fail to name one, chain two deep, or
explain that it is a pointer. One 4-to-1 consolidation done
inside a round carrying twenty-five other edits produced five
findings in the next round.

This command commits nothing, so it has no change of its own to
put a consolidation in, and every later snapshot is cumulative
-- so a consolidation applied in stage 1 is reviewed together
with everything else the run touched. Name the copies, apply
none of them, and hand the developer a consolidation to make as
its own commit after the run.

**Enumerate before you claim a set is done.** Read the list
back and count it. Four defects in one round of this command's
own review were a stated count that did not match its list, or
a set fixed in some of its members.

**Do not fix everything.** Every edit is new text for the next
reviewer. Apply the mechanical ones directly -- a stale doc, a
tightened regex, a renamed local -- and announce the set so the
developer can interrupt. Fix what is wrong or false, and what
would make someone act on it wrongly -- a reader, the operator,
or bombyx itself. That last one matters because most of what
this loop guards is not prose: a config value interpolated into
Ruby without quoting misleads no reader and still hands the VM
host a command nobody wrote. Leave what would merely read
better.

**Escalate rather than apply** when a finding crosses one of
these: large rework (over five files, over a hundred lines, or
churn outside the diff); two findings conflicting; a genuine
design tradeoff; a public-surface or breaking change; a new
dependency; a consolidation of three or more copies; out of
scope for the work in hand. Escalation matters more here than
on a landed commit, because the work is uncommitted and there
is no boundary to revert a bad rework to. Present the finding
in the fields its reviewer emitted and ask: fix it now, defer
it, decline it, or leave it and let the developer decide before
committing.

**Read the artifact back before claiming a fix landed.** "The
help now says Y" needs the grep that shows it.

### Log what you defer

A fixed finding gets no entry; only a deferred one, in
`docs/developer/redteam-log.md`, `artisan-log.md` or
`fresh-reader-log.md` by reviewer. All three are newest-first,
with new entries right after the `---`. The ID is
`<rt|aq|fr>-<YYYY-MM-DD>-<kebab-slug>`, so there is no counter
to keep and the ID greps. Each entry is that heading, a
`**Category:**` line, and a short description.

**Closing one is the other half of the rule.** When a stage
acts on or reverses a logged item, name its ID and either
delete the entry or annotate it "superseded by ...". Without
that, the backlog fills with items somebody already fixed, and
the alarm below stops meaning anything. When ten or more sit
open in one backlog, say so: the backlog has become the
problem.

## Stage 1 -- `artisan`, once

Spawn `artisan` alone and fix what it finds.

**Ask it for the source code only.** Its `Error Handling &
Messages`, `API Design`, `Abstraction Boundaries`, `Type
Safety` and `Module Size` categories are this stage. Its
`Canon and documentation` category is **not**: tell it that
documentation and canon belong to stages 2 and 3, so a finding
it has there arrives as one line rather than a full entry.

On the run this sequence came from, artisan's documentation
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
two defects on one earlier run were live: 286 tests, ten gates,
98.1% coverage. A test asserts the property somebody thought
of, and this stage exists to find the property nobody thought
of. One of that run's findings was itself a missing test --
disabling the branch that prints the winning host left every
test passing.

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
  round reads the same code and returns the same findings. A
  defect in code a later step of the same plan deletes is the
  usual reason to defer one deliberately.

**Stop early when earlier fixes are breaking**, and go to
**When it stops converging** below: more than one defect in an
earlier round's fix, or one landing where an earlier round
already fixed something. A single isolated defect in a fix is
not that: fix it, note it, and count the note against the next
round.

**Three rounds is the ceiling.** One branch ran five rounds at
60, 42, 36, 37 and 33 findings, which is a flat tail rather
than convergence. Reaching the ceiling means the rule above
failed, so say so rather than reporting the run as finished.

**A run has now exercised this stop rule.** The 2026-09-06
backlog sweep: stage 1 raised ten findings and fixed all ten,
and stage 2's first round raised thirteen with **no behaviour
defect among them** -- six were defects in stage 1's own fixes.
So both stopping conditions fired on the same round. What it
showed:

- **The two conditions are not independent.** A stage whose
  findings are all prose is also a stage whose fixes are all
  prose, and rewriting a comment is how the next round's
  findings get made. Expect them together.
- **Sharpening prose is what makes it falsifiable.** Stage 1
  asked for vague and historical comments to be rewritten
  precisely. Each precise sentence was then checkable, and six
  were wrong. None of the six was reachable before the rewrite,
  so the second round was not re-reading the first -- it was
  reading claims the first round created.
- **Verify each claim with a command as you write it.** That is
  what the operator chose after the stop, and it is the cheap
  half of the lesson. `docs/todo.md` carries
  `comment-claims-have-no-gate` for the expensive half.

So a finding against this rule can now be acted on rather than
logged. Change it only against a run like that one, and say
which run.

## Stage 3 -- `fresh-reader`, prose only

Re-snapshot after stage 2's last fix, and hand it the
changed-file list rather than a diff -- `code-reviewers.md`
under **Diff handoff** says why -- with every earlier stage's
findings, the one-line hand-offs included. Read those before
spawning.

On a change with no source code in it this is the only stage,
so there are no earlier findings to hand over and the first
snapshot is the one it reads.

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
  not resolve, and it failed twice on that account during one
  earlier run.

A finding against one of those is real and gets **logged, not
fixed**, per **Log what you defer** above. Say in the report
that this stage's limit excluded it rather than that nobody
thought it mattered.

`fresh-reader`'s **What worked** section is not a finding and
needs no action. Carry it into the report anyway, so the
passages it named are known to carry a reason the next time
somebody trims comments.

## When it stops converging

Findings landing on earlier fixes are not saying the fixes were
careless. They are saying something about the work. Stop fixing
and tell the developer: name the file or fact, give the chain
(round N said X, the fix did Y, round N+1 found Z, quoted), and
say which of these it looks like.

- **The rule has no single home.** One rule in several places,
  and none says which copy wins. Commonest, cheapest. A
  `remote_root` rule once sat in four documents, each stating a
  different subset.
- **The assertion has no contract behind it.** A test or
  document checks something with no stable shape -- rendered
  output, prose, a number that is really a file's length.
  Usually delete it rather than parse harder. Three tests that
  scanned the docs produced nine findings of their own over
  four rounds.
- **Two owners decide, and neither states the invariant.**
  Which `doctor` rows exist, what each outcome does to the exit
  code, and what the summary says: three files, five rounds.
- **Only running it finds the gaps.** For artifacts recorded as
  *could not run* or *must not run* under **Run it before
  anyone reads it**. If we ran it, this is not the category.
- **The work fights the design.** Every fix is correct and it
  still does not settle, and none of the above explains why.

Give at least two options with the trade-off named, say which
you would pick and why, and use `AskUserQuestion` when the
choice changes what happens next.

## Reporting

Per stage: findings raised, fixed, logged, escalated -- and for
stage 2 the round count with the reason it stopped, naming the
behaviour defect that earned the last round or saying that none
did.

Then the closing account: every artifact with its label and,
where it failed, what failed; each non-converging area in the
shape above; and what the run left behind, because the
developer's next commit sees all of it -- the edited files, the
backlog files written this run, and the intent-to-add entries
from the snapshot. Nothing was committed.

## Rules

- **Never commit, never amend, never push.**
- One reviewer at a time. Spawning two defeats the point.
- Each stage asks for one lane. `artisan` is never asked about
  documentation.
- A finding outside the current stage's lane is carried to the
  stage that owns it, never dropped and never fixed early.
- A change with no source code in it runs stage 3 alone.
- Re-snapshot before every stage and round, and write each
  stage's findings before the next spawn.
- Never skip the artifact run because the change obviously
  works.
- Never correct one of several copies without counting them.
- A behaviour defect that was deferred does not earn another
  round.
- `fresh-reader` never edits clap help, `allowed-tools`, or a
  rustdoc link.
- Surface every finding, whatever its disposition -- applied,
  escalated, deferred or declined. Never drop one silently.
- If a finding is wrong, say so with the evidence and move on.
  Reviewers are wrong sometimes, and arguing it in the report
  beats fixing something that was right.
