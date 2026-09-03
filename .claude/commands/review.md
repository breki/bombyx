---
description: Review and fix uncommitted work over up to three rounds, committing nothing; stop and report when an area stops converging
---

Review the working tree and fix what the reviewers find.
**This command never commits.** It leaves the tree edited and
reports what happened, and the developer decides when the work
becomes a commit. Nothing requires this command; `CLAUDE.md`
under **Reviewing is its own process** says why.

It takes no arguments and always reviews the whole tree against
`HEAD`.

`code-reviewers.md` owns which reviewers run, how to spawn them
and what each one is handed. The loop, the fixing rules and the
reporting are here.

## The loop

Up to three rounds. Step 4 usually stops it sooner.

Three rounds is the cap because the findings stop dropping off.
One branch ran five rounds and found 60, 42, 36, 37, 33. A
converging process has no flat tail like that one; it had one
because each round's fixes were making the next round's
findings.

Every round repeats steps 1, 3 and 4. Step 2 runs in full once,
and afterwards only for artifacts a fix has touched.

Step 2 normally records a failure and leaves the fixing to step
3. Some failures cannot be recorded without a fix first -- a
sample config that will not load, a quoted command with a typo
in it. **If you fix anything during step 2, write the snapshot
again before spawning anyone.** Overwrite the same `<n>`: the
round has one snapshot, and it has to match the text the
reviewers read. Otherwise they judge the pre-fix version of the
least-read edit in the run.

### 1. Snapshot

Reviewers must read one immutable text; `CLAUDE.md` gives the
reason a live tree is not one.

```bash
mkdir -p target
git ls-files --others --exclude-standard      # untracked: check before adding
git add -N <the untracked paths of this work>
EXCL=':(exclude)docs/developer/*-log.md'
git diff HEAD -- . "$EXCL"                        > target/review-<n>.diff
git diff --name-only --diff-filter=d HEAD -- . "$EXCL" > target/review-<n>.files
```

`<n>` is the round number, so each round leaves its own pair of
files and an earlier round's snapshot stays readable when a
later finding is about the fix for an earlier one.

`git add -N` records a path in the index without its contents,
which is what makes `git diff` report an untracked file at all.
Name the paths; `git add -N .` sweeps in every untracked file,
and the developer's scratch notes then reach the reviewers and
land in their next `git commit -a`.

`:(exclude)<pattern>` is a git pathspec that subtracts matches
from the paths before it, which is why `.` comes first. The
backlogs are subtracted because step 3 writes to them, and left
in they would hand each round the previous round's own report
to find defects in. `--diff-filter=d` drops deleted paths, which
`fresh-reader` can only fail to open.

The index keeps the intent-to-add entries. Report that, and
report the undo with it: `git reset -- <the paths added with
-N>`. Until then any `git commit -a` commits those paths, even
the ones the round concluded should not ship. `/commit` stages
by name, so it will not sweep them, which is why they can sit
in the index unnoticed.

### 2. Run it before anyone reads it

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
not run**. A run that fails is a finding for this round, with
the same treatment as a reviewer's; a gate that aborts the
pipeline leaves the gates behind it *could not run*.

- **A code change** -- `cargo xtask validate --check`, so the
  formatter cannot rewrite the tree the snapshot just captured.
  When the change touches the commands bombyx emits, Definition
  of Done item 3 applies: the real run against the VM host, and
  *could not run* when the host is unreachable. `--dry-run`
  proves the argv and nothing else.
- **A sample config, a quoted command, a transcript** --
  execute it and compare. Do not eyeball it.
- **A procedure document** -- follow it from the state it names,
  unless following it mutates something you do not own.
  `docs/vm-host-setup.md` provisions a shared host as root:
  that is *must not run*, and a dry run or a disposable target
  is the substitute.
- **A workflow file** (`.claude/**`, `CLAUDE.md`) -- walk it
  against the current tree without spawning anything. An edited
  agent file only takes effect next session, so record that part
  as *could not run*.

### 3. Review, then fix

**On round three, report the findings and apply nothing.**
Otherwise the run ends on edits nobody read, in a tree with no
commit behind them. The rule lives here rather than in step 4
because this is where the fixing happens, and a rule about not
fixing is no use one step downstream of it.

`code-reviewers.md` under **When to run** specifies which
reviewers this change needs. Spawn those in one parallel
message, and hand each one what its **Diff handoff** section
says -- the snapshot path this round wrote, or the `.files`
list. This file owns one more instruction:

- **From round two, hand over the earlier rounds' findings and
  what was done with each**, and ask outright: *is anything here
  a defect in the fix for an earlier finding?* A reviewer shown
  only the current state cannot see a loop. This is the only
  thing that has ever detected non-convergence here.

Check the replies before acting on them --
`code-reviewers.md` under **Reading the reports back** covers
a truncated reply and the test for two reviewers reaching one
defect.

Then fix:

**Count the copies first.** Say a rule is written in four
places. A round finds three, the fix corrects those three, and
the next round finds the fourth. That is the largest single
source of rounds that never end. So on a finding of the shape
"X is wrong in F", find every place that says X before touching
one. Above two, the fix is one authoritative statement and
pointers to it.

**Enumerate before you claim a set is done.** Read the list back
and count it. Four defects in one round of this command's own
review were a stated count that did not match its list, or a set
fixed in some of its members.

**Do not fix everything.** Every edit is new text for the next
round. Apply the mechanical ones directly -- a stale doc, a
tightened regex, a renamed local -- and announce the set so the
developer can interrupt. Fix what is wrong, false, or would
mislead a reader into an error.

**Escalate rather than apply** when a finding crosses one of
these: large rework (over five files, over a hundred lines, or
churn outside the diff); two findings conflicting; a genuine
design tradeoff; a public-surface or breaking change; a new
dependency; out of scope for the work in hand. Escalation
matters more here than on a landed commit, because the work is
uncommitted and there is no boundary to revert a bad rework to.
Present the finding in the fields its reviewer emitted and ask:
fix it now, defer it, decline it, or leave it and let the
developer decide before committing. Surface every finding in
the report -- applied, escalated, deferred or declined -- and
never drop one silently.

**Log what you defer.** A fixed finding gets no entry; only a
deferred one, in `docs/developer/redteam-log.md`,
`artisan-log.md` or `fresh-reader-log.md` by reviewer. All three
are newest-first, with new entries right after the `---`. The ID
is `<rt|aq|fr>-<YYYY-MM-DD>-<kebab-slug>`, so there is no
counter to keep and the ID greps. Each entry is that heading, a
`**Category:**` line, and a short description.

**Closing one is the other half of the rule.** When a round
acts on or reverses a logged item, name its ID and either
delete the entry or annotate it "superseded by ...". Without
that, the backlog fills with items somebody already fixed, and
the alarm below stops meaning anything. When ten or more sit
open in one backlog, say so: the backlog has become the
problem.

**Read the artifact back before claiming a fix landed.** "The
help now says Y" needs the grep that shows it.

### 4. Stop, or go again

Stop when any holds:

- **The round fixed nothing** -- every finding deferred or
  declined. The canon rule, and usually the first to fire.
- **Earlier fixes are breaking.** More than one defect in an
  earlier round's fix, or one landing in an area an earlier
  round already fixed. Go to **When it stops converging**. A
  single isolated defect in a fix is not that: fix it, note it,
  and count the note against the next round.
- **Three rounds are done.** Step 3 says what the third round
  does instead of fixing, and why.

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
  scanned the docs produced nine findings of their own over four
  rounds.
- **Two owners decide, and neither states the invariant.** Which
  `doctor` rows exist, what each outcome does to the exit code,
  and what the summary says: three files, five rounds.
- **Only running it finds the gaps.** For artifacts step 2
  recorded as *could not run* or *must not run*. If step 2 ran
  it, this is not the category.
- **The work fights the design.** Every fix is correct and it
  still does not settle, and none of the above explains why.

Give at least two options with the trade-off named, say which
you would pick and why, and use `AskUserQuestion` when the
choice changes what happens next.

## Reporting

Per round: findings raised, fixed, logged, escalated. Every
step-2 artifact with its label and, where it failed, what
failed. Each non-converging area in the shape above. Then what
the run left behind, because the developer's next commit sees
all of it: the edited files, the backlog files written this
run, and the intent-to-add entries from **Snapshot**. Nothing
was committed.

`fresh-reader`'s **What worked** section is not a finding and
needs no action. Do not drop it either: carry it into the
report, so the passages it named are known to carry a reason
the next time somebody trims comments.

## Rules

- **Never commit, never amend, never push.**
- Never skip step 2 because the change obviously works.
- Never correct one of several copies without counting them.
- A round that fixes nothing still reports.
- If a finding is wrong, say so with the evidence and move on.
  Reviewers are wrong sometimes, and arguing it in the report
  beats fixing something that was right.
