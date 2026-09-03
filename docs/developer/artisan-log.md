# Artisan Findings -- Deferred backlog

Quality (Artisan) review findings. Newest first.

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
