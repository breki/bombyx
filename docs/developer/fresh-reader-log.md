# Fresh Reader Findings -- Deferred backlog

Comprehension review findings, from the reviewer that reads the
changed files cold. Newest first. A finding that was fixed leaves
no entry -- the comment it produced is the record.

---

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

### fr-2026-09-03-implement-md-stale-tool-grants

**Category:** Command definition

`.claude/commands/implement.md`'s frontmatter grants `Skill(commit)`
but not `Skill(review)`, while step 6 says to "Optionally run
`/review`", so a reader cannot tell whether the command invokes it
or hands off. (The stale `Bash(scripts/e2e.sh*)` grant this entry
also named was removed on 2026-09-18.) Deferred: settling
invoke-vs-hand-off is a decision, not a scope call.
