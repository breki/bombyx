# Artisan Findings -- Deferred backlog

Quality (Artisan) review findings. Newest first.

---

### aq-2026-09-02-commit-md-outgrown-shape

**Category:** Document size / abstraction boundary

`.claude/commands/commit.md` is ~375 lines and its step 9 spans
about 145 of them, carrying five separate contracts: which
reviewers to spawn, cross-confirmation matching, truncated-output
recovery, the auto-apply-versus-escalate thresholds, and the
deferred-backlog file format and ID scheme. One item that is 40%
of a numbered procedure has stopped reading as a step, which is
how the stale step numbers in the same review round survived a
sweep.

`code-reviewers.md` already declares itself the owner of which
reviewers run and how they are spawned, so the first two overlap
it outright. The fix is to move the cross-confirmation,
truncated-output and backlog-format blocks into that file and
leave step 9 at roughly 40 lines.

Deferred because the review order was restructured in the same
sitting that raised this. Moving the same file twice in one
session is the churn the reviewers flagged elsewhere on this
branch. Do it as its own change.

Found by the Artisan review (AQ-8), 2026-09-02.

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
