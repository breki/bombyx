# Artisan Findings -- Deferred backlog

Quality (Artisan) review findings. Newest first.

---

### aq-2026-09-05-check-segment-runs-three-times-per-load

**Category:** Type Safety

One project name is checked three times in one
`Config::load_project`: by `check_segment` in that function
before the file is opened, by `check_segment` again inside
`Registry::project`, and by `Config::validate` over the
assembled value. `crate::name::ProjectName` already enforces
exactly that rule, and `HostOrigin::ProjectEntry` already
carries one -- so the pair `load_project` returns holds the
same name as both a checked type and an unchecked `String`.

The wanted shape is `load_project(name: &ProjectName, ..)` with
`Config::project` a `ProjectName`, which also makes a bad
`--project` fail at the argument rather than inside the loader.

Deferred by the operator during the `/review2` on
`project-selection-flag` (#18): it is a public-surface change,
and `newtype-remaining-config-fields` (#17) owns the
`Config::project` half. All three call sites have written-down
reasons, so deleting one is not obviously safe either. Found as
AQ-2.

### aq-2026-09-05-host-missing-carries-a-string-not-a-path

**Category:** Type Safety

`ConfigError::HostMissing`'s `place` field is a `String`, and
after #18 it has one construction site: `config::host::rank`,
which always builds it from `read::path_display(registry.path())`.
A `Registry` cannot exist without a path, so the value is always
a path and the `String` loses that. `RegistryNotFound` is the
variant that genuinely needs prose, because it describes a file
bombyx never opened.

Deferred by the operator during the `/review2` on
`project-selection-flag` (#18): it is a public-surface change to
an error variant, on a branch already carrying several. Found as
AQ-5.

### aq-2026-09-05-four-fixture-builders-for-two-shapes

**Category:** API Design

`crates/bombyx/src/config.rs` builds a test registry through
four helpers covering two shapes: `test_registry` and
`test_entry` at module scope, `test_entry_with` beneath them,
and `registry_with` inside the `tests` module, which is
`test_registry` with a `keys` string in place of a
`project_host`. One module-scope
`test_registry_with(name, host, keys)` with the others as thin
wrappers would replace all four.

Deferred by the operator during the `/review2` on
`project-selection-flag` (#18). `/review` under **Review, then
fix** forbids applying a three-or-more consolidation in the
round that finds it, and the one plain copy -- an inline
`format!` reimplementing `test_registry` -- was fixed in that
round instead. Found as AQ-8.

### aq-2026-09-04-project-remote-root-stays-a-string

**Category:** Type safety

`Project::remote_root` in `crates/bombyx/src/config/registry.rs`
is a `String`, and so is `Config::remote_root`. The value has
six rules attached in `config::root`, and it is the value bombyx
builds the directory it deletes with `rm -rf` from. A newtype
built through `TryFrom<String>` would make holding one the proof
the rules ran, the way `RepoUrl` does.

Deferred: the two fields have to gain a type together, or one of
them ends up unwrapping the other's. That work is
`newtype-remaining-config-fields` in `docs/todo.md`, GitHub #17,
which covers all five checked fields. Raised again while
reviewing #24; `Project::validate` now runs `root::check` on the
value at lookup, so the rules do run on the one path that hands
a `Project` out.

---

### aq-2026-09-04-blocks-rebuilt-per-check

**Category:** Efficiency

`collect` in `xtask/src/canon.rs` calls `reference_targets`
over every canon file and then `unresolved_xrefs` over every
canon file, and each call rebuilds the paragraph blocks from
scratch, so each file is copied into `String`s twice.
`reference_targets` also walks the content twice on its own,
once for headings and once for blocks. Building the blocks
once per file in `collect` and passing `&[Block]` to both
would end that, but `Block` is private and the checks are
`pub`, so it waits on the entry above.

Deferred: 23 small markdown files, so the cost is invisible
today.

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
