# Artisan Findings -- Deferred backlog

Quality (Artisan) review findings. Newest first.

---

### aq-2026-09-03-finding-ids-do-not-persist-into-a-backlog

**Category:** A false claim in a reviewer's own brief

`.claude/agents/artisan.md` and `.claude/agents/red-team.md`
both tell the agent that "a deferred finding keeps its ID in
the backlog". It does not. `/review` under **Log what you
defer** mints `<rt|aq|fr>-<YYYY-MM-DD>-<kebab-slug>` for a
logged entry, so `AQ-3` reaches no backlog and a reader who
greps for it finds nothing. The reason the numbering exists is
sound -- `/review` cites the IDs when it reports what it fixed,
deferred and declined -- and only the second clause is wrong.

Deferred: it is a false statement and worth correcting, but it
sits in two agent files that have been rewritten five times in
two weeks, and `/review` now says a consolidation or a sweep of
this surface is its own change.

Found by the Artisan review (AQ-2), 2026-09-03.

---

### aq-2026-09-03-diary-exemption-states-no-mechanism

**Category:** An exemption nobody applies

`.claude/commands/code-reviewers.md` exempts
`docs/developer/DIARY.md` from `fresh-reader` and then explains
that `/review` subtracts the backlogs from the snapshot but not
the diary. Nothing says who drops the diary from the `.files`
list a reviewer is handed, and `/review`'s snapshot excludes
only `docs/developer/*-log.md`. The prose reads as though the
tooling handles it. The closing clause is also circular: "a
diary edit reaches a snapshot only when one is already sitting
in the tree" resolves "one" back to "a diary edit".

Deferred: the fix is either a second `:(exclude)` in `/review`
or a plainer sentence, and choosing between them is a decision
about who owns the exemption.

Found by the Artisan review (AQ-5), 2026-09-03.

---

### aq-2026-09-03-resnapshot-omits-the-files-list

**Category:** An incomplete instruction

`/review` says that if you fix anything during step 2 you
should write the snapshot again, overwriting the same `<n>`.
**Snapshot** writes two files, the diff and the `.files` list,
and a step-2 fix can add a file, which changes the second one.
`fresh-reader` is handed the `.files` list, so rewriting only
the diff leaves that one reviewer reading the pre-fix state --
which is the exact thing the paragraph exists to prevent.
Saying "re-run both commands in **Snapshot**" closes it.

Deferred: mechanical, but it lands in the loop prose that
`/review` now says to sweep as its own change.

Found by the Artisan review (AQ-6), 2026-09-03.

---

### aq-2026-09-03-sentences-that-explain-a-pointer

**Category:** Voice

Reducing duplicated rules to one owner left commentary about
document layout inside the rules themselves. `CLAUDE.md` says
"That is the reason; `/review` under **Snapshot** holds how it
does it, and is the only place that should" -- the final clause
has no verb. `code-reviewers.md` says "Neither argument is
repeated here." `/review` explains that a rule "lives here
rather than in step 4 because this is where the fixing
happens". None of the four is actionable, and each is one more
sentence to keep true the next time a section moves, which is
the maintenance the consolidation was meant to reduce. A
pointer needs no note explaining that it is a pointer.

`CLAUDE.md` also says "`git log 6055f93` is the arrangement we
backed out of". A command is not an arrangement, and
`git log <sha>` prints that commit and its ancestors. "Commit
`6055f93` is where that arrangement landed" says it.

Deferred: phrasing only, in the surface `/review` now says to
leave alone unless a sweep is its own change.

Found by the Artisan review (AQ-7, AQ-8), 2026-09-03.

---

### aq-2026-09-03-ragged-paragraphs-and-two-xref-styles

**Category:** Formatting left by single-line patches

Two paragraphs were patched a line at a time and left short
lines mid-paragraph, which `CLAUDE.md` under **Coding
Standards** says a reader takes for a paragraph break:
`CLAUDE.md` around the `/commit` bullet (one line of 42
characters), and `code-reviewers.md` where the diary sentence
was inserted. `cargo xtask canon-check` cannot see these -- it
checks the 80-column ceiling, and a too-short line is not a
claim about the tree.

Separately, `/review` refers to its own steps by number in
twelve places and by heading name in one ("the intent-to-add
entries from **Snapshot**"). A reader cannot tell whether those
are two schemes or two different things, and the next renumber
leaves half the file stale. Since the step headings are
numbered, either convention resolves; the file should pick one.

Deferred: reflowing and picking a convention is a sweep of this
surface, which `/review` now says to do as its own change.

Found by the Artisan review (AQ-9, AQ-12), 2026-09-03.

---

### aq-2026-09-02-writing-actions-listed-three-times

**Category:** A rule hand-listed in three places

Which actions write the generated files is spelled out in `plan()`,
in `every_other_action_writes_nothing`'s classifier, and in
`only_the_three_writing_actions_write`. The test comment claims the
set is derived; only the iteration is. One
`Action::writes_files(&self)` with an exhaustive match would make a
new action a compile error instead of a silent omission.


---

### aq-2026-09-02-build-recipes-seams

**Category:** Documentation consistency

`docs/developer/build-recipes.md` is text lifted out of
`CLAUDE.md`, and two seams survive the move.

The body wraps at roughly 48 columns while its new header wraps
at 80, so the join is visible and any later edit inherits the
wrong margin. Re-wrapping the whole body to 80 is cheap now and
gets more expensive once anything cites line numbers.

The body also still speaks as the template addressing a
downstream project -- "Real projects routinely have I/O paths
that can't", "If a derived project needs", "The template ships
on Rust edition 2024" -- and uses contractions the rest of
`docs/` does not.

Deferred because the re-wrap is a large mechanical diff, better
kept away from a round of substantive fixes.

Found by the Artisan review (AQ-16), 2026-09-02. The
edition-2024 placement question is split out below, because a
later commit citing this slug could not otherwise say which of
the three actions it did. Two actions remain here: re-wrap the
body to 80 columns, and change the four template-voiced
openers to speak to this project.

---

### aq-2026-09-02-build-recipes-edition-section-placement

**Category:** Abstraction boundary between documents

`docs/developer/build-recipes.md` groups three recipes. Two
share a theme: how to scope an exception to a quality gate
without weakening it for production code. The edition-2024
section is not that. It is a one-time migration checklist, it
is already done for bombyx, and it speaks to a project
inheriting an older snapshot of the template rather than to
anyone working here.

It probably belongs in `docs/developer/template-feedback.md`,
or under an explicit appendix heading in the same file.

Deferred because it wants a decision from the operator rather
than a fix. Found by the Artisan review (AQ-16), 2026-09-02.
