# Fresh Reader Findings -- Deferred backlog

Comprehension review findings, from the reviewer that reads the
changed files cold. Newest first. A finding that was fixed leaves
no entry -- the comment it produced is the record.

---

### fr-2026-09-13-comparatives-without-their-comparison

**Category:** Comprehension

`CLAUDE.md` under **Voice** names "limits what stealing it is
worth" as the example of a comparative with no comparison. The
phrase and its kin -- "narrower than it first looks", "narrower
than it looks", "the tighter choice", "Read-only is tighter" --
are still in `README.md`, `config.toml.sample` and
`docs/trust-boundary.md`, and a reader deciding a token's scope
gets nothing actionable from any of them. The repair is to name
the reach instead, the way the trust-boundary passage that lists
what each token type reaches already does. Deferred as out of
scope for the `repo_token` work.

### fr-2026-09-13-trust-boundary-opening-qualifies-early

**Category:** Structure

The blockquote at the top of `docs/trust-boundary.md` qualifies
"Statement one", "The boundary" and "Where project code lives
today" -- a statement and two headings 20 to 60 lines below, so
the caveat arrives before the thing it caveats. The repair is to
move it below **The boundary**, keeping at the top only that
neither statement is confirmed against a remote VM host and that
statement one is a property of a machine, not of bombyx. Deferred:
restructuring a document's opening is out of scope for a
one-paragraph change.

### fr-2026-09-11-named-rather-than-linked-is-explained-everywhere

**Category:** Duplicated explanation deferred for its own commit

Five doc comments each explain that a public rustdoc page may not
link to a private item, so the comment names it in backticks --
`crates/bombyx/src/listing.rs`, `config/host.rs`, `config.rs`,
`config/registry.rs` and `config/vm.rs` -- and a sixth in
`term.rs`'s module header. Deferred: `/review` forbids collapsing
copies in the round that finds them. State the rule once (in
`CLAUDE.md` or `docs/architecture.md`) and trim the six, a commit
of its own.

### fr-2026-09-11-doctor-transcript-omits-project

**Category:** False claim about the tool

`docs/vm-host-setup.md` under **Checking that it worked** tells
the reader to run `bombyx --project <name> doctor`, and the
transcript below opens with `$ bombyx doctor`, which `main.rs`
rejects without `--project`. The fix is to spell the transcript's
prompt line the way the sentence above it does. Deferred as out of
scope for the change that found it.

### fr-2026-09-11-page-has-no-date-keeping-rule

**Category:** Staleness cannot be judged

`docs/vm-host-setup.md` states its age two ways: the header stamps
Steps 1 and 2 as verified in August 2026, and the fog section
quotes gem versions "in August 2026, which is what a fresh host
set up from this page gets today", where "today" carries no date.
A reader cannot tell which facts the stamp covers, and an undated
"today" ages without showing it. The repair is one rule: the
header carries the check date, and a separately-checked section
says so rather than saying "today".

### fr-2026-09-11-step-3-verification-status-unstated

**Category:** Unmarked verification status

`docs/vm-host-setup.md` promises that unverified steps carry an
inline marker and stamps Steps 1 and 2 verified. Step 3, the
libvirt provider plugin, carries no marker either way, while its
body reports a real run and its `CONFIGURE_ARGS` fallback looks
like the half that was not exercised. Either extend the header to
Step 3 or mark the fallback unverified.

### fr-2026-09-06-round-local-finding-ids-promise-a-record

**Category:** A citation with nothing behind it

`xtask/src/feedback.rs` cites `(RT-3)` and `(RT-4)`, and thirteen
more instances sit in `coverage.rs`, `dep_age.rs`, `backfeed.rs`
and `dep_age/preflight.rs`. These are round-local numbers, not
backlog ids -- `canon.rs`'s `is_backlog_id` requires
`rt-<date>-<slug>` -- so a reader who looks one up finds nothing.
Each comment already states its property in full, so the tag can
go, or cite the durable `rt-<date>-<slug>` id. Deferred: the files
are outside the change that found it, and no gate reads `.rs` for
the shape.

### fr-2026-09-06-usage-dates-a-case-by-unstated-behaviour

**Category:** A condition the reader cannot locate

`docs/usage.md` says "The first is a VM you created before this
behaviour existed, and which branch you are in depends on whether
you have run `up` since." bombyx is pre-release and names no
version, so "before this behaviour existed" gives no test, and
"which branch you are in" reads as a git branch in a passage about
`ref` and checkouts. The repair is to state the observable
condition -- a VM whose `fresh-install` snapshot records something
other than a fresh install, or has none -- and say "which of the
two cases applies to you". Deferred: pre-existing prose outside the
change.

### fr-2026-09-06-tutorial-transcripts-dated-by-release

**Category:** A qualification the reader cannot apply

`docs/tutorial.md` warns that the Part 3 and 4 transcripts show
behaviour "none of which 0.4.1 could produce", but Part 1 installs
from a clone, so the reader's binary is whatever the checkout
builds rather than 0.4.1, and the warning gives no way to tell
whether the transcripts match. Tie the qualification to what the
reader has: the transcripts were written from current source, and
a binary from a published 0.4.1 archive prints something
different. Deferred: pre-existing prose, and the version figure is
the kind `/release` moves.

### fr-2026-09-05-provider-argument-lives-in-a-comment

**Category:** reasoning in a comment rather than in `docs/`

`remote::PROVIDER_ENV`'s doc comment holds the whole case for how
bombyx selects a provider: the mechanism, the environment variable
over `vagrant up --provider`, three measured facts, why every
project call but the teardown carries it, the WSL2 inversion, and
the known limit. Four places defer to it, so the argument is owned
by a comment and pointed at from documents, which is backwards.
`docs/architecture.md` now carries the three measurements (the
first half of the fix); what remains is a subsection there holding
the mechanism and consequence, with the constant cut back to the
local fact and a pointer. Deferred per `/review`: a consolidation
is not applied in the round that finds it.

### fr-2026-09-05-field-rules-filed-under-a-traps-heading

**Category:** Structure

In `docs/architecture.md`, the subsection **Two traps a reader
cannot see from the code** ends with the clap trap, and then
seventy more lines continue under it -- the library-consumer
paragraph, the `remote_root` newtype paragraph, and the whole
refusal table. The block opens "Three things keep that survivable
meanwhile", and "that" refers to a sentence a hundred lines and
two headings earlier. The fix is to move the block beside the gap
it qualifies, or give it a heading and replace "that" with the
noun. Deferred: a structural move in a 550-line document, outside
the change's scope.

### fr-2026-09-05-registry-named-in-clap-help-without-introduction

**Category:** Terminology (clap help -- outside a prose reviewer's lane)

`bombyx --help` prints "Path to your registry" for `--config`, and
the word is introduced nowhere the operator reads before that. #18
added an introduction to `README.md`, which leaves the help text
itself: a reader starting at `--help` meets "your registry" cold.
Logged rather than fixed because editing clap `///` help changes
the program's output, which is outside a prose reviewer's lane.

### fr-2026-09-04-canon-rs-assumes-the-review-vocabulary

**Category:** Terms used before they are introduced

Four findings from one read of `xtask/src/canon.rs`, all the same
shape: the module explains its mechanisms and not its words.
"canon" carries the module and is never defined in it; `unknown_
ids` and `is_backlog_id` never say what `rt-`, `aq-` and `fr-`
stand for or where a backlog lives; "the id scheme exists so an id
greps" has no subject doing the searching; and in `ungranted_git`,
"the declaration may wrap across lines" sits beside the
`allowed-tools:` read but means the prose grant instead, so a
reader concludes a wrapped `allowed-tools:` is tolerated. Deferred:
all four are prose the change did not touch.

### fr-2026-09-03-no-reviewer-emits-the-severity-field

**Category:** A judgement with no named source

`/review`'s fixing bar asks what would make a reader, the operator
or bombyx act wrongly, and no reviewer emits that field. `red-team`
emits **Example trigger** (the closest thing), `artisan`
**Why it matters: impact on maintainability**, `fresh-reader`
**Where it left me** -- none the same test. So a caller sorting
findings has no statement of which field carries the answer.
Deferred: naming the field per reviewer touches all three agent
briefs.

### fr-2026-09-03-count-the-note-has-no-destination

**Category:** A mechanic with no definition

`/review` says a single isolated defect in an earlier round's fix
is "fix it, note it, and count the note against the next round",
and where the note goes and what the count decides are both
unstated. The likely reading is a tally that trips the
"more than one defect in an earlier round's fix" stop condition,
but only a guess gets there. Deferred: naming the destination
changes what a stop condition counts, so it wants deciding.

### fr-2026-09-03-retrospect-writes-a-backlog-without-its-format

**Category:** An instruction that omits what the actor needs

`.claude/commands/retrospect.md` tells the actor to append a
reviewer finding to the backlog but not what an entry looks like
(newest-first, after the `---`, the `<rt|aq|fr>-<date>-<slug>` id,
a `**Category:**` line, a description). That rule is in `/review`
under **Log what you defer**, and the same file already points
there for another rule, so the omission at the write site is the
odd one. Deferred: one pointer, in a file outside the review.

### fr-2026-09-03-diff-filter-case-mechanism-unstated

**Category:** A mechanism the comment leans on without stating

`/review` says "`--diff-filter=d` drops deleted paths, which
`fresh-reader` can only fail to open." Uppercase `D` selects
deleted paths; a lowercase filter letter inverts the selection.
The comment states the effect and hides the mechanism, so a reader
adding another filter letter cannot predict which case to use, and
`CLAUDE.md` asks for the mechanism before the conclusion. Deferred:
one clause, in the loop prose `/review` sweeps as its own change.

### fr-2026-09-03-step-two-spawn-prohibition-unscoped

**Category:** An instruction that collides with a later step

`/review` step 2 says a workflow file should be walked "against the
current tree without spawning anything", and step 3 of the same
round spawns three agents. A reader cannot tell whether the
prohibition is scoped to step 2's walk-through or is a claim about
the round. Deferred: scoping it is one clause, in the loop prose.

### fr-2026-09-03-implement-md-stale-tool-grants

**Category:** Command definition

`.claude/commands/implement.md` grants `Bash(scripts/e2e.sh*)`,
and `CLAUDE.md` states `scripts/e2e.sh` does not exist -- the file
says so itself. The frontmatter also grants `Skill(commit)` but
not `Skill(review)`, while step 6 says to "Optionally run
`/review`", so a reader cannot tell whether the command invokes it
or hands off. Deferred: outside the diff of the change that found
it.

### fr-2026-09-03-retrospect-examples-name-absent-tools

**Category:** An example that is itself the defect it illustrates

`.claude/commands/retrospect.md` illustrates a Cleanup finding --
"a skill/command referencing a tool, file or workflow that no
longer exists" -- with the `web-dev` skill and
`playwright.config.js`, neither of which exists here. The live
instance of that shape is `implement.md`'s `scripts/e2e.sh` grant,
which would make the example real. Deferred: outside the diff of
the change that found it.

### fr-2026-09-03-code-reviewers-does-not-say-what-it-is

**Category:** A file whose kind is unclear from its content

`.claude/commands/code-reviewers.md` has no frontmatter, unlike
every sibling in that directory, so a reader cannot tell whether
`/code-reviewers` is invokable or whether the file is reference
material `/review` reads. It is registered as a skill, which the
file never says. Deferred: outside the diff of the change that
found it.

### fr-2026-09-03-simplify-row-not-marked-global

**Category:** Skills table does not distinguish global from project

`CLAUDE.md`'s skills table lists `/simplify` with no in-repo
definition, and both reviewers read that as a dangling row.
`/simplify` is a live global skill neither could see from the repo.
The real gap is that the table mixes project skills with global
ones and never marks which is which, so a reader is offered an
option they cannot locate. Deferred: outside the diff of the change
that found it.
