# Fresh Reader Findings -- Deferred backlog

Comprehension review findings, from the reviewer that reads the
changed files cold. Newest first. A finding that was fixed leaves
no entry -- the comment it produced is the record.

---

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
