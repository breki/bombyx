# Artisan Findings -- Deferred backlog

Quality (Artisan) review findings. Newest first.

---

### aq-2026-09-13-plan-secrets-can-disagree-with-the-config

**Category:** Type Safety / API Design

`crate::plan::plan` takes `Option<&Secrets>` alongside the
`Config` with nothing tying them: `plan` stages `bombyx.env` from
the argument while `vagrantfile::render` renders the upload block
and `BOMBYX_ENV_FILE_PRESENT=1` from `cfg.source.env_file`, and
both mismatches compile silently (a Vagrantfile claiming a secrets
file with none staged, or a staged file no Vagrantfile uploads).
`Config::read_secrets` is the one supported builder and `main.rs`
uses it, which closes the in-repo path but does not make the
mismatch impossible. Wanted: one value pairing the borrowed
`Config` with the secrets read from it, taken by both `plan` and
`render`. Deferred by the operator on #78: a design change for its
own commit.

### aq-2026-09-08-two-bootstrap-tests-carry-no-assertion-of-their-own

**Category:** Module Size

Two of the eighteen tests in
`crates/bombyx/src/vagrantfile/bootstrap_tests.rs` each duplicate a
single assertion another test already makes
(`the_clone_is_told_which_key_to_push_with` and
`the_bootstrap_script_deletes_a_key_no_upload_replaced`), so
deleting one needle from `bootstrap.sh` fails four tests naming
four rules. Wanted: delete the two and move each comment's reason
into the richer test that owns the needle. Deferred per `/review`
(a consolidation is not applied in the round that found it).

### aq-2026-09-05-host-missing-carries-a-string-not-a-path

**Category:** Type Safety

`ConfigError::HostMissing`'s `place` field is a `String` with one
construction site (`config::host::rank`, always built from
`registry.path()`), so the value is always a path and the `String`
loses that. `RegistryNotFound` is the variant that genuinely needs
prose, because it describes a file bombyx never opened. Deferred by
the operator on #18: a public-surface change to an error variant.

### aq-2026-09-05-four-fixture-builders-for-two-shapes

**Category:** API Design

`crates/bombyx/src/config.rs` builds a test registry through four
helpers for two shapes (`test_registry` and `test_entry` at module
scope, `test_entry_with`, and `registry_with` in the `tests`
module); one module-scope `test_registry_with(name, host, keys)`
with the others as thin wrappers would replace all four. Deferred
per `/review` (a three-or-more consolidation is its own change);
the one inline `format!` copy was fixed in that round.

### aq-2026-09-04-blocks-rebuilt-per-check

**Category:** Efficiency

`collect` in `xtask/src/canon.rs` runs `reference_targets` over
every canon file and then `unresolved_xrefs` over every canon
file, each rebuilding the paragraph blocks, so each file is parsed
into `String`s twice (and `reference_targets` walks it twice more).
Building the blocks once and passing `&[Block]` to both would end
that, but `Block` is private and the checks are `pub`. Deferred: 23
small markdown files, so the cost is invisible today.

### aq-2026-09-03-finding-ids-do-not-persist-into-a-backlog

**Category:** A false claim in a reviewer's own brief

`.claude/agents/artisan.md` and `.claude/agents/red-team.md` both
tell the agent that "a deferred finding keeps its ID in the
backlog". It does not: `/review` mints a fresh
`<rt|aq|fr>-<date>-<slug>` for a logged entry, so a round-local
`AQ-3` reaches no backlog. Only that clause is wrong. Deferred: it
sits in two often-rewritten agent files, and a sweep of them is
its own change.

### aq-2026-09-03-resnapshot-omits-the-files-list

**Category:** An incomplete instruction

`/review` says to write the snapshot again if you fix anything in
step 2, but **Snapshot** writes two files -- the diff and the
`.files` list -- and `fresh-reader` is handed the `.files` list, so
rewriting only the diff leaves that reviewer reading the pre-fix
state. "Re-run both commands in **Snapshot**" closes it. Deferred:
mechanical, but in the loop prose `/review` sweeps as its own
change.

### aq-2026-09-02-build-recipes-seams

**Category:** Documentation consistency

`docs/developer/build-recipes.md` was lifted out of `CLAUDE.md`
and two seams survive: the body wraps at roughly 48 columns while
its header wraps at 80, and it still speaks as the template to a
downstream ("If a derived project needs", "The template ships on
Rust edition 2024"). Two actions: re-wrap the body to 80, and
reword the four template-voiced openers to speak to this project.
Deferred: a large mechanical re-wrap, kept away from a round of
substantive fixes.

### aq-2026-09-02-build-recipes-edition-section-placement

**Category:** Abstraction boundary between documents

`docs/developer/build-recipes.md`'s edition-2024 section is a
one-time migration checklist, already done for bombyx, that speaks
to a project inheriting an older template -- not the theme the
other two recipes share (scoping a gate exception without
weakening it for production). It probably belongs in
`docs/developer/template-feedback.md` or under an explicit
appendix heading. Deferred: it wants an operator decision rather
than a fix.
