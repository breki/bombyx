# Fresh Reader Findings -- Deferred backlog

Comprehension review findings, from the reviewer that reads the
changed files cold. Newest first.

An entry here is a place where the code did not explain itself
and we chose not to fix it yet. A finding that *was* fixed
leaves no entry -- the comment it produced is the record.

---

### fr-2026-09-03-no-reviewer-emits-the-severity-field

**Category:** A judgement with no named source

`/review`'s fixing bar asks what would make a reader, the
operator or bombyx act wrongly. No reviewer emits that. `red-team`
emits **Why it matters** and **Example trigger**, `artisan` emits
**Why it matters: impact on maintainability**, and `fresh-reader`
emits **Where it left me**. Impact on maintainability is not the
same test, and "where it left me" states a question the reader
could not answer, which is close but not it.

So the caller with twenty findings in front of them has no
statement of which field carries the answer, or whether they
judge it themselves from the **What** field. `red-team`'s
**Example trigger** is probably the closest thing to a severity
statement in any of the three briefs, and nothing says so.

Deferred: naming the field per reviewer touches all three agent
briefs, and `/review` is frozen until a run against a real code
diff has exercised the bar.

Found by the Fresh Reader review (FR-4), 2026-09-03.

---

### fr-2026-09-03-artisan-500-line-rule-has-no-canon-counterpart

**Category:** A reviewer rule the project may have outgrown

`.claude/agents/artisan.md` says any source file over 500 lines
containing multiple structs or enums should be flagged for
splitting. `CLAUDE.md` **Coding Standards** states no such limit,
and `CLAUDE.md` **Environment Constraints** describes
`crates/bombyx/src/config.rs` as 1747 lines while advising how to
read it, with no suggestion that its length is a defect.

So a caller receiving "split `config.rs`" cannot tell whether it
is a project rule or a default from the brief that the project
has silently outgrown, and therefore whether to decline it every
round.

Deferred: either the limit belongs in `CLAUDE.md` with the brief
pointing at it, or the brief should say the number is a prompt to
look rather than a rule. Deciding that is not this run's work.

Found by the Fresh Reader review (FR-9), 2026-09-03.

---

### fr-2026-09-03-pointers-do-not-name-what-they-point-at

**Category:** A cross-reference a reader cannot follow

Two defects in the pointer network that replaced the duplicated
rules. `/review` step 1 says "`CLAUDE.md` gives the reason a
live tree is not one" without naming the section, while three
other pointers in the same file do name theirs -- so a reader
greps a 1000-line file for a heading they have to guess. And
six references call **Diff handoff** a section, `/review`
saying "what its **Diff handoff** section says", when the
target is an inline bold paragraph inside **How to spawn**. A
reader scans the headings, finds none, and concludes the
pointer is stale.

`cargo xtask canon-check` catches a bold reference that names
no heading at all. It cannot catch one that resolves to a
paragraph rather than a heading, or one that names no section
when it should.

Deferred: promoting **Diff handoff** to a heading and
qualifying the `CLAUDE.md` pointer is a sweep of this surface.

Found by the Fresh Reader review (FR-1, FR-11), 2026-09-03.

---

### fr-2026-09-03-count-the-note-has-no-destination

**Category:** A mechanic with no definition

`/review` says that a single isolated defect in an earlier
round's fix is not the breaking-fixes case: "fix it, note it,
and count the note against the next round." Where the note goes
and what the count decides are both unstated. Three readings
are available: an item in the run's report, a backlog entry
(but a fixed finding gets no entry, and this one was fixed), or
a tally that trips the "more than one defect in an earlier
round's fix" condition when the next round adds to it. The
third is probably meant, and only a guess gets you there.

Deferred: naming the destination is one clause, but it changes
what a stop condition counts, so it wants deciding rather than
guessing.

Found by the Fresh Reader review (FR-3), 2026-09-03.

---

### fr-2026-09-03-retrospect-writes-a-backlog-without-its-format

**Category:** An instruction that omits what the actor needs

`.claude/commands/retrospect.md` tells the actor to append a
real reviewer finding to the backlog for that reviewer, naming
all three files. It does not say what an entry looks like:
newest-first, immediately after the `---`, the
`<rt|aq|fr>-<date>-<slug>` ID, then a `**Category:**` line and
a description. That rule is in `/review` under **Log what you
defer**, and the same file already points there for the
logged-versus-fixed rule -- so the omission at the write site
is the odd one. Reading the existing entries is a workable
fallback, but the ID's date is not derivable from them with
confidence.

Deferred: one pointer, in a file outside the work under review.

Found by the Fresh Reader review (FR-4), 2026-09-03.

---

### fr-2026-09-03-sync-module-doc-narrates-the-past

**Category:** A comment that describes the behaviour it replaced

`xtask/src/sync.rs`'s module doc reads "`/template-sync` is
*already* SHA-delta based, but *it surfaced* template-internal
bookkeeping files ... as sync candidates. Those grow on every
commit, so *they became* pure review noise." A reader cannot
tell whether the noisy behaviour is live somewhere or is the
state this command replaced, and "already" implies a contrast
with knowledge they do not have. `CLAUDE.md` under **Code
comments** rules this out by name. The present-tense reason
survives the cut: bookkeeping files change on every commit and
each project owns its own, so an upstream change to one is
never worth pulling, and this command drops them before the LLM
sees the list.

Deferred: outside the work under review.

Found by the Fresh Reader review (FR-8), 2026-09-03.

---

### fr-2026-09-03-diff-filter-case-mechanism-unstated

**Category:** A mechanism the comment leans on without stating

`/review` explains its snapshot commands carefully, and stops
one clause short on this one: "`--diff-filter=d` drops deleted
paths, which `fresh-reader` can only fail to open." Uppercase
`D` *selects* deleted paths; a lowercase filter letter inverts
the selection. The comment states the effect and hides the
mechanism, so a reader adding another filter letter cannot
predict which case to use. `CLAUDE.md` asks for the mechanism
before the conclusion.

Deferred: one clause, in the loop prose `/review` now says to
sweep as its own change.

Found by the Fresh Reader review (FR-9), 2026-09-03.

---

### fr-2026-09-03-least-read-edit-reads-as-a-ranking

**Category:** Voice

`/review`'s re-snapshot paragraph ends "Otherwise they judge
the pre-fix version of the least-read edit in the run." The
superlative over "edits in the run" sends the reader looking
for a ranking of edits by how often they were read. The point
is simpler: a fix made during step 2 is the one edit no
reviewer has seen, so it is exactly the one they must be shown.

Deferred: phrasing, in prose written during the review that
raised it.

Found by the Fresh Reader review (FR-10), 2026-09-03.

---

### fr-2026-09-03-step-two-spawn-prohibition-unscoped

**Category:** An instruction that collides with a later step

`/review` step 2 says a workflow file should be walked "against
the current tree without spawning anything", and step 3 of the
same round spawns three agents. A reader cannot tell whether
the prohibition is scoped to step 2's walk-through -- do not
exercise the workflow by spawning the agents it describes -- or
is a claim about the round. The sentence after it, about agent
edits taking effect next session, suggests the former without
saying it.

Deferred: scoping it is one clause, in the loop prose.

Found by the Fresh Reader review (FR-13), 2026-09-03.

---

### fr-2026-09-03-three-logs-named-as-two

**Category:** Canon states a set incompletely

`docs/developer/fresh-reader-log.md` exists and `/review` names
all three backlogs. `.claude/commands/retrospect.md` and
`xtask/src/sync.rs` have been corrected to name all three.
Two files still name only two:
`.claude/skills/architect/SKILL.md:67-68` and
`.claude/commands/template-improve.md:75`.

Deferred: both remaining files are outside the work under
review, and neither states a rule -- `SKILL.md` draws a
directory tree and `template-improve.md` lists where feedback
goes.

Found by the Fresh Reader review (FR-7), 2026-09-03. Narrowed
2026-09-03 after `retrospect.md` and `sync.rs` were fixed.

---

### fr-2026-09-03-implement-md-stale-tool-grants

**Category:** Command definition

`.claude/commands/implement.md:3` grants
`Bash(scripts/e2e.sh*)`, and `CLAUDE.md` states that
`scripts/e2e.sh` does not exist -- `implement.md:96-98` says so
itself. The same frontmatter grants `Skill(commit)` but not
`Skill(review)`, while step 6 tells the actor to "Optionally run
`/review`", so a reader cannot tell whether the command invokes
it or hands off to the developer.

Deferred: outside the diff of the commit under review.

Found by the Fresh Reader review (FR-8, FR-9), 2026-09-03.

---

### fr-2026-09-03-gate-numbers-copied-out-of-xtask

**Category:** A number owned by code, restated in prose

Two figures live in `xtask` and are re-typed into canon, where
they have drifted. `llms.txt:125` lists seven items for
`validate` and `llms.txt:136` says it has nine steps; the two
missing are the dependency cooldown and `deny`, and the cooldown
is the gate `CLAUDE.md` says fires when you were not expecting
it. Separately `xtask/src/coverage.rs:18` enforces
`MODULE_THRESHOLD = 85.0`, which `CLAUDE.md` never mentions --
it states only the 90% workspace floor, so whether a module at
86% passes is answerable only from the source.

Deferred: outside the diff of the commit under review.

Found by the Fresh Reader review (FR-12, FR-13), 2026-09-03.

---

### fr-2026-09-03-retrospect-examples-name-absent-tools

**Category:** An example that is itself the defect it illustrates

`.claude/commands/retrospect.md:95-96` and `:179-185` illustrate
a Cleanup finding -- "a skill/command referencing a tool, file
or workflow that no longer exists" -- with the `web-dev` skill
and `playwright.config.js`. Neither exists here, and `CLAUDE.md`
states Playwright is not used. The live instance of that shape
is `implement.md`'s `scripts/e2e.sh` grant, which would make the
example real.

Deferred: outside the diff of the commit under review.

Found by the Fresh Reader review (FR-15), 2026-09-03.

---

### fr-2026-09-03-code-reviewers-does-not-say-what-it-is

**Category:** A file whose kind is unclear from its content

`.claude/commands/code-reviewers.md` has no frontmatter, unlike
every sibling in that directory, so a reader cannot tell whether
`/code-reviewers` is invokable or whether the file is reference
material `/review` reads. It is in fact registered as a skill,
which the file itself never says.

Deferred: outside the diff of the commit under review.

Found by the Fresh Reader review (FR-5), 2026-09-03.

---

### fr-2026-09-03-simplify-row-not-marked-global

**Category:** Skills table does not distinguish global from project

`CLAUDE.md`'s skills table lists `/simplify` with no in-repo
definition: there is no `.claude/commands/simplify.md`, and
`.claude/skills.json` declares only `architect`. Both `red-team`
and the Fresh Reader read that as a dangling row and asked for
its deletion. **They were wrong about the cause** -- `/simplify`
is a live global skill, which neither reviewer could see from
the repo. The real gap is that the table mixes project skills
with global ones and never marks which is which, so a reader
deciding how to harden work before a commit is offered a third
option they cannot locate.

Deferred: outside the diff of the commit under review.

Found by the Fresh Reader review (FR-14) and the red team review
(RT-12), 2026-09-03.

---

### fr-2026-09-02-two-more-files-describe-the-push

**Category:** Files no sweep opened

`bombyx.toml.sample` and `llms.txt` both describe the push as current
behaviour, and `README.md` and `docs/vm-host-setup.md` point readers
at them. `llms.txt` is the file whose name promises a machine can
read it first; it says bombyx "pushes a project's `vagrant/`
directory" and that "the host holds a cache refreshed on every `up`".
It also says `validate` has eight steps where `CLAUDE.md` says nine.

### fr-2026-09-02-host-setup-tells-you-to-write-a-vagrantfile

**Category:** Two documents giving opposite instructions

`docs/vm-host-setup.md`'s "Configuration for each project" says every
project needs "a `vagrant/` directory containing a Vagrantfile" and
that bombyx does not ship one "because the project repository is
meant to be the source of truth". `docs/tutorial.md` says the
opposite in as many words: bombyx renders it, a committed one is read
by nothing, delete it. The same page also says bombyx does not
control the synced folder, which the generated Vagrantfile disables
unconditionally, and names "the jutro VM" with no definition.

### fr-2026-09-02-boundary-claim-unqualified-in-five-places

**Category:** One rule, two strengths

`docs/trust-boundary.md` qualifies "the VM host holds no project
code" once -- the guest's disk image is a file on the host -- and
repeats it unqualified at three other points in the same file, plus
`CLAUDE.md`, `.claude/skills/architect/SKILL.md` and
`crates/bombyx/README.md`. A reader meets the strong form first and
may never reach the qualification.

### fr-2026-09-02-main-narrates-its-own-history

**Category:** Voice

Six comments in `main.rs` explain the code by saying what it used to
be: "It used to say 'thin by design'", "the first cut of this fix",
"An earlier version answered a non-zero `curl` with ...". Two of them
describe a `matches!` "four hundred lines away" that no longer
exists, which costs a search of the file. `CLAUDE.md` rules the shape
out by name.

### fr-2026-09-02-architect-skill-calls-main-thin

**Category:** Canon disagreeing with the code it describes

The architect skill calls `main.rs` "(thin)" and says to keep logic
out of it. `main.rs` opens by refusing that description: "It used to
say 'thin by design', and that is worth not claiming", and the
self-update sequence lives there. The rule the skill wants is "keep
new decisions out", not "it is thin".

### fr-2026-09-02-project-field-unaccounted

**Category:** Gap in the plan

The document opens a ledger of five values read from the
repository. Chunk 1 accounts for `vagrant_dir`, chunk 2 for
`remote_root`, `[vm]` and `[source]`. `project` is never mentioned
again, and it is load-bearing: `remote_project_dir()` builds the
path `destroy` runs `rm -rf` against from it.

### fr-2026-09-02-chunk-two-has-no-caller

**Category:** Gap in the plan

Chunk 2 changes `Config::load` to take a project name, and chunk 3
introduces the `--project` that supplies one. Each chunk is its own
commit, so chunk 2 as described leaves `main.rs` with no name to
pass and no registry path to read.

### fr-2026-09-02-project-flag-clap-shape

**Category:** Unclear specification

"a required global argument for every `VmCmd` variant" does not
describe a shape clap can build. A `global = true` argument cannot
also be required, `VmCmd` is a flattened enum with no shared field,
and the next sentence says `self-update` must work without it.
Three lines of argv and a note on where the check happens would
settle it.

### fr-2026-09-02-chunk-two-test-inventory-missing

**Category:** Asymmetry that misleads

Chunk 1 gets nine tests named by line, all of them accurate. Chunk
2 gets one sentence, which reads as "nothing else breaks". Chunk 2
deletes the overlay, and `integration_test.rs:224` and `:241` are
built on `bombyx.local.toml`; the second becomes a test of nothing
rather than a failure.

### fr-2026-09-02-rules-versus-statements

**Category:** Two names for one thing

The document calls the boundary's two halves "rules" in the Problem
section and "statements" in the Plan. `docs/trust-boundary.md` uses
only "statements".

### fr-2026-09-02-chunk-used-before-defined

**Category:** Term used before introduction

"chunk 1" and "chunk 2" appear in the Problem section; "Three
chunks, in this order. Each is its own commit." is 80 lines later.
The opening paragraph already lists the three items and could say
they are the three chunks. The same document introduces "the
registry" correctly, which is the pattern to copy.

### fr-2026-09-02-three-phrasings

**Category:** Voice

Three stumbles. "Rust unit tests for the config loading and
lookup." is a verbless fragment of the shape `CLAUDE.md` rules out.
"It names the registry file and the keys the entry needs" reads
"It" as the entry, not the error message. "which reads the
filesystem here" uses "here" for "on the workstation", right after
a file:line reference, where it first reads as "at that line".
