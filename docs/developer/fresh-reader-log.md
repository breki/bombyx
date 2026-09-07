# Fresh Reader Findings -- Deferred backlog

Comprehension review findings, from the reviewer that reads the
changed files cold. Newest first.

An entry here is a place where the code did not explain itself
and we chose not to fix it yet. A finding that *was* fixed
leaves no entry -- the comment it produced is the record.

---

### fr-2026-09-06-round-local-finding-ids-promise-a-record

**Category:** A citation with nothing behind it

`xtask/src/feedback.rs` cites `(RT-3)` at line 92 and `(RT-4)`
at line 109, and thirteen more instances sit in
`xtask/src/coverage.rs`, `dep_age.rs`, `backfeed.rs` and
`dep_age/preflight.rs`. These are round-local numbers from a
review run, not backlog IDs: `xtask/src/canon.rs`'s
`is_backlog_id` requires `rt-<ISO date>-<slug>`, and the
reviewer logs hold only that shape. So a reader who tries to
look one up finds nothing.

Each comment already states its property in full, so the tag
adds no information and can simply go. The alternative is
citing the durable `rt-<date>-<slug>` ID, which greps.

Deferred: the files are outside the change that found it, and
`canon-check` does not read `.rs`, so nothing gates the shape
either way. Found as FR-10 in the `/review2` on the backlog
sweep, 2026-09-06.

### fr-2026-09-06-usage-dates-a-case-by-unstated-behaviour

**Category:** A condition the reader cannot locate

`docs/usage.md` around line 141 says "The first is a VM you
created before this behaviour existed, and which branch you are
in depends on whether you have run `up` since." Two snags.
bombyx is pre-release and the sentence names no version, so
"before this behaviour existed" gives a reader no way to tell
whether it applies to them. And "which branch you are in" reads
as a git branch, in a document whose next paragraphs are about
`ref`, `repo` and checkouts.

The repair is to state the observable condition instead of the
history -- a VM whose `fresh-install` snapshot records
something other than a fresh install, or that has none -- and
to say "which of the two cases applies to you".

Deferred: pre-existing prose outside the change. Found as FR-17
in the `/review2` on the backlog sweep, 2026-09-06.

### fr-2026-09-06-tutorial-transcripts-dated-by-release

**Category:** A qualification the reader cannot apply

`docs/tutorial.md` around line 19 warns that the transcripts in
Parts 3 and 4 "show behaviour that is unreleased at the time of
writing ... none of which 0.4.1 could produce". But Part 1
installs with `cargo install --path crates/bombyx` from a
clone, so the reader's binary is whatever the checkout builds
rather than 0.4.1, and the warning gives them no way to tell
whether the transcripts match what they will see.

Tie the qualification to what the reader has: the transcripts
were written from the current source rather than captured from
a run, and a binary installed from a published 0.4.1 archive
prints something different.

Deferred: pre-existing prose outside the change, and the
version figure is the kind `/release` moves. Found as FR-20 in
the `/review2` on the backlog sweep, 2026-09-06.

### fr-2026-09-05-provider-argument-lives-in-a-comment

**Category:** reasoning in a comment rather than in `docs/`

`remote::PROVIDER_ENV`'s doc comment holds the whole case for
how bombyx selects a provider: what rendering a provider block
does, why the environment variable rather than
`vagrant up --provider`, three facts measured on frosti, why
every project call but the teardown carries it, why the
teardown is exempt, how a WSL2 host inverts that, and the known
limit with its backlog ID. Four other places defer to it --
`docs/architecture.md`, `plan.rs`'s test, `probe.rs` and
`vagrantfile.rs` -- so the argument is owned by a comment and
pointed at from a document, which is backwards. `CLAUDE.md`
under **Code comments** says reasoning belongs in `docs/` and
that a shared explanation is not owned by a comment.

**Updated 2026-09-07**, during the review on issue #48. The
inventory above is what the comment holds now; the entry was
filed when it held "why only the boot carries it" instead. The
comment grew by two paragraphs in that work, so the case for
moving it is stronger rather than weaker. `docs/architecture.md`
now carries the three measurements as well, which is the first
half of the fix below -- what remains is cutting the constant
back to the local fact.

The fix is a subsection in `docs/architecture.md` holding the
mechanism, the measurements and the consequence, with the
constant cut back to the local fact and a pointer. Not applied
in the round that found it, per `/review` under **Review, then
fix**: a consolidation is never applied in its own round, and
this prose had already failed to converge over three red-team
rounds.

Found by `fresh-reader` in stage 3 of the review on issue #45.

---

### fr-2026-09-05-field-rules-filed-under-a-traps-heading

**Category:** Structure

In `docs/architecture.md`, the subsection **Two traps a reader
cannot see from the code** ends with the clap trap, and then
seventy more lines continue under it: the library-consumer
paragraph, the `remote_root` newtype paragraph, the whole
`| Field | Refused | Because |` table and every `remote_root`
rule. The block opens "Three things keep that survivable
meanwhile", and "that" refers to a sentence a hundred lines and
two headings earlier.

The fix is to move the block back beside the gap it qualifies,
or give it a heading naming its subject and replace "that" with
the noun. Predates #18 and is untouched by it.

Deferred during the `/review2` on #18: it is a structural move
in a 550-line document, outside that change's scope. Found as
FR-1.

### fr-2026-09-05-registry-named-in-clap-help-without-introduction

**Category:** Terminology (clap help -- outside a prose reviewer's lane)

`bombyx --help` prints "Path to your registry" for `--config`,
and the word is not introduced anywhere the operator reads
before that. #18 added an introduction to `README.md` under
**Configure**, which leaves the help text itself: a reader who
starts at `--help` still meets "your registry" cold, with no way
to tell it from "your config file".

Logged rather than fixed because `/review2` under **Stage 3**
keeps `fresh-reader` out of clap `///` help: editing it changes
the program's output. Found as FR-14.

### fr-2026-09-04-todo-help-hides-four-of-five-doc-rules

**Category:** The program's own help is thinner than its rules

`cargo xtask todo done --help` describes `--doc` as "A path
naming no file is an error". That is one of five rules. The
allowed character set is the one that produces the most
surprising refusal -- a perfectly ordinary-looking
`issues/plan.md#step-3` is rejected -- and `--help` says nothing
about it, so an operator has to read `xtask/src/todo.rs` to find
out why their path was refused.

The same block still says `add --issue` renders "a link to
`issues/<slug>.md>`", which is now the only place in the tool
that derives a path from a slug. Nothing in the help says
`done` deliberately stopped doing that, so the two subcommands
look inconsistent for no stated reason.

Not fixed here: this is clap `///` help, which is program
output rather than documentation, and `/review2` bars stage 3
from editing it. The limit is why it was not fixed, not a
judgement that it does not matter.

Raised by fresh-reader in the `/review2` on #7.

### fr-2026-09-04-canon-rs-assumes-the-review-vocabulary

**Category:** Terms used before they are introduced

Four findings from one read of `xtask/src/canon.rs`, all the
same shape: the module explains its mechanisms and not its
words.

- FR-3. "canon" carries the whole module and is never defined
  in it. Which files it covers is only decidable 460 lines
  down, in `canon_files`, and a reader cannot tell whether
  `docs/` is in scope. One clause naming the four kinds of
  file, and saying `docs/` is deliberately out, would settle
  it.
- FR-4. `unknown_ids` and `is_backlog_id` never say what
  `rt-`, `aq-` and `fr-` stand for, what a backlog is, or
  where one lives. The reader worked the prefixes out from
  `code-reviewers.md`, not from the file.
- FR-5. "The ID scheme exists so an ID greps" and "the
  declaration is in the prose, so it greps" have no subject
  doing anything, and in this repo a third reading is live:
  `CLAUDE.md` warns that a wrapped phrase defeats grep. Say
  who searches for what.
- FR-6. In `ungranted_git`, "The declaration may wrap across
  lines" sits right after `grants` is read from the single
  `allowed-tools:` line, and means the *other* declaration --
  the prose sentence ``no `git <sub>` grant``. The reader
  concluded a wrapped `allowed-tools:` was tolerated, then
  found it was not.

Deferred: all four are prose the wrapped-bold change did not
touch, and fixing them is churn outside its diff.

---

### fr-2026-09-04-todo-md-header-documents-one-entry-shape

**Category:** A convention the file does not state

FR-14. `docs/todo.md`'s header describes only the linked entry
shape, while the file holds three: linked
(`[**slug**](issues/slug.md)`), bare bold, and backticked.
Somebody completing an item by hand cannot tell which to
write, or that a bare bold slug is correct for an item with no
planning document rather than one somebody forgot to link. The
pending `todo-done-link` item explains the absence 200 lines
below, and reads as a bug report rather than as the file's
conventions.

Deferred: `todo-done-link` is likely to change which shapes
are legal, so documenting all three now would be written
twice.

---

### fr-2026-09-04-open-questions-count-does-not-match-its-list

**Category:** A count that disagrees with the list under it

`docs/issues/project-config-off-repo.md`'s progress log says
"Two of the three **Open questions** above are now answered",
and what follows answers one: `remote_root` stays per-project.
The other two sentences say `destroy`'s positional is step 7
and still open, and that the `.git/config` question stays
parked. A reader cannot tell which second question was meant,
or whether an answer was decided and never written down.

Predates this branch -- the text came in with commit 03f7528,
the seven-step re-split -- so it is logged rather than fixed
here. Either say "One of the three", or, if assigning
`destroy`'s positional to step 7 counts as closing that
question, say so in those words.

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
