# documentation-overhaul

**Status:** Move 3 complete (2026-09-17); moves 4-5 remain
**Captured:** 2026-09-17

A program of work to cut the documentation from a large, mannered,
duplicated corpus to a lean one that reads plainly and does not
mislead an AI agent. Run one move at a time, each move its own commit.
A `/docs-audit` run is the before-and-after measure.

This is a shared plan a fresh agent can execute end to end. Read it
top to bottom, then start at the first unfinished move.

## Why -- the four problems this fixes

From the 2026-09-17 documentation audit (`/docs-audit`):

1. **Mannered prose** -- anecdote and aphorism where a plain sentence
   carries the same instruction. Heaviest in `CLAUDE.md`,
   `docs/todo.md`, `docs/trust-boundary.md`, `docs/tutorial.md`.
2. **Duplication with no single owner, which drifts.** Live symptom:
   the install version reads `0.6.0` in `README.md` and `0.5.0` in
   `docs/quickstart.md`.
3. **Stale records read as current.** The diary removal and the
   ephemeral-issue-doc change addressed the biggest instances.
4. **Length forcing partial reads.** `CLAUDE.md` is 1000 lines and is
   the agent's mandatory manual, so an agent samples it.

## The house style to match

`docs/developer/supply-chain.md` and `docs/quickstart.md`: concrete,
mechanism-first, plain words, no war-stories. `CLAUDE.md` under
**Writing** is the standard. A change in this program that itself
reads as mannered has failed its own test -- read the `blue-pencil`
agent before editing prose.

## Already done (2026-09-17)

- Auto-diary removed: step cut from `/commit`, `DIARY.md` deleted,
  canon scrubbed. Kept in the sync never-sync set so a template-sync
  cannot resurrect it.
- Issue docs made ephemeral: `/implement` and `/issue` promote durable
  decisions into the reference docs, then remove the working doc.
  `docs/issues/` pruned to the in-progress `project-config-off-repo.md`
  after a coverage check confirmed the durable reasoning already lives
  in `architecture.md`, `trust-boundary.md`, and `vm-host-wsl2.md`.
- `/docs-audit` command added -- re-run it to measure each move.
- `record-files-typed-header` captured in `docs/todo.md`.

## The moves, in order of leverage

Run one per `/commit`. Do not batch; each is a review-sized change.

### 1. De-manner the canon (highest leverage)

Targets: `CLAUDE.md`, `llms.txt`. Convert each anecdote to its
one-line rule and drop the incident -- git history keeps it. Cut the
"what prompted writing this down" framing. Remove the build-command
and skills tables that are duplicated between the two files; leave one
owner. Set a length budget so the mandatory manual reads in one pass.
Acceptance: a `/docs-audit` run rates `CLAUDE.md` and `llms.txt`
mannered Some or None, not Heavy; `canon-check` green.

### 2. One home per topic

Give each duplicated subject a single owning file; every other file
links to it. Known duplications and their captured items:

- Install version -- fix `docs/quickstart.md` (`0.5.0` -> `0.6.0`) and
  make one file the owner (see the version-source rule in `CLAUDE.md`).
- `env_file` / `deploy_key` rules -- item
  `env-file-rules-stated-five-times`.
- Config-check reasoning -- `docs/usage.md` restates
  `docs/architecture.md`; keep the "why" in `architecture.md`, the
  operator-facing "what" in `usage.md`.

Do this outside a review round: `/review` forbids the consolidation,
because the N-1 pointers that replace the copies become the next
round's findings (see `env-file-rules-stated-five-times`).

### 3. Split reference out of the guides

- Lift the roughly 400-line firewall/nftables section out of
  `docs/vm-host-setup.md` into its own doc; keep `vm-host-setup.md` to
  install plus verify.
- Move `docs/tutorial.md`'s reference-grade digressions into
  `docs/usage.md`; keep the tutorial to the happy path. Related items:
  `tutorial-box-lacks-git`, `tutorial-debian-box-warning`,
  `tutorial-provision-git-warning`, `readme-vagrantfile-pointer`.

### 4. Tag and collapse the backlogs

- Give each review-log and `template-feedback.md` entry an explicit
  status field -- this is the `record-files-typed-header` work; do
  that item first, then this move rides on it.
- Fold or delete `docs/developer/fresh-reader-log.md` (a log about the
  prose of other logs).
- Trim `template-feedback.md` entries to the fact, not the re-derived
  rationale.

### 5. Adopt the house style and a length budget

Write the style rule down once (mechanism-first, plain, no war-stories;
`supply-chain.md` is the model) and a length cap on the agent manual,
so the corpus does not regrow the same way.

## Constraints for the executing agent

- Read `CLAUDE.md` under **Writing** and the `blue-pencil` agent before
  editing prose. The plan is to remove mannered prose; do not add it.
- One move per `/commit`. These are docs-only: no CHANGELOG entry, and
  no real-VM verification is needed, because nothing here changes the
  commands bombyx emits.
- Run `canon-check` after each canon edit; run `/docs-audit` at the end
  of each move to measure.
- When reflowing an over-long line, reflow its whole paragraph, per
  `CLAUDE.md` coding standards.

## Acceptance

Done when a `/docs-audit` run shows no file rated mannered Heavy, the
duplication list is empty (each topic has one home), and `canon-check`
and `cargo xtask validate` are green. Remove this plan doc then, per
the ephemeral-issue-doc convention.

## Move 1 decisions (2026-09-17)

- **Cut depth: rule plus one-line why.** Each anecdote becomes its
  rule plus a single clause of reasoning where the reasoning prevents
  a real mistake; the incident itself is dropped (git history keeps
  it). Target: `CLAUDE.md` around 600-650 lines, down from 1001.
- **Table ownership: `CLAUDE.md` owns both.** The build-command and
  skills tables stay in the always-loaded manual (the agent needs the
  command list at hand, and canon-check reads `CLAUDE.md` but not
  `docs/`). `llms.txt` drops its shorter copies and links to
  `CLAUDE.md`. A third reference file was considered and declined: it
  would split the command list out of the only auto-loaded file.
- **Split of concerns confirmed:** `CLAUDE.md` = how to work here
  (workflow, standards, commands, skills); `llms.txt` = what the
  project is (structure, model, config, conventions).

## Move 1 progress log

- 2026-09-17: decisions recorded above; starting the `CLAUDE.md` and
  `llms.txt` rewrite.
- 2026-09-17: move 1 done. `CLAUDE.md` de-mannered 1001 -> 866 lines;
  every war-story cut to its rule plus one illustrative clause, no
  rule dropped (verified by diffing the bolded rule inventory).
  Duplicated build-command and skills tables removed from `llms.txt`
  (447 lines, down from 474), which now points to `CLAUDE.md`.
  `canon-check` and `cargo xtask validate` green. Length landed above
  the 600-650 estimate; the operator accepted 866, since the residue
  is reference material and mechanism, not manner. Moves 2-5 remain;
  this plan doc stays until the whole program is done.

## Move 2 decisions (2026-09-17)

Move 2 is split into review-sized commits rather than one, because
the three duplications differ wildly in size.

- **Commit 1 (this one): install version.** `docs/quickstart.md`
  drifted to 0.5.0 while `Cargo.toml` and README were 0.6.0. Fixed
  quickstart to 0.6.0 and made it the single owner of the install
  snippet: README dropped its version-pinned block for a one-line
  pointer to quickstart, so only one file now carries the version.
  Root cause captured as `sync-version-sentinels` in `docs/todo.md`:
  `/release` never rewrites the `<!-- version: -->` sentinels, so the
  docs-only "one owner" fix is a stopgap until an xtask step syncs
  them. That mechanism fix is out of this docs-only move's scope.
- **Commit 2 (next): env_file/config-check consolidation.** The
  `env-file-rules-stated-five-times` item plus the config-check
  reasoning in `docs/usage.md` that restates `docs/architecture.md`.
  Left for its own commit -- it spans README, usage, architecture,
  trust-boundary, tutorial and code comments, and drifted twice in
  one review, so it earns an isolated, carefully-reviewed change.

## Move 2 progress log

- 2026-09-17: commit 1 done. quickstart -> 0.6.0, README delegates
  install to quickstart, `sync-version-sentinels` todo captured.
  env_file/config-check consolidation still pending.
- 2026-09-17: commit 2 done. Consolidated the env_file/deploy_key
  rules onto owners. An Explore-agent map found the copies split
  into two kinds: self-documenting artifacts (`config.toml.sample`
  inline comments, the Rust `# Errors` blocks) that must state their
  own rule locally and are kept, and prose docs where the real
  duplication lived. Owners: `architecture.md` for the exact path
  rule and the "why"; `trust-boundary.md` for retention (how long
  the host holds the staged secrets, and that an interrupted run
  leaves it). `docs/usage.md` was the doc re-deriving both, so it
  now gives a short operator-facing version and links out (three
  edits). `architecture.md` needed no change -- it already deferred
  retention to `trust-boundary.md` and its wording was consistent.
  Correction to the plan: README does not state the retention fact
  (it points to usage.md), and the `deploy_key` "trailing slash" vs
  `env_file` "trailing separator" wording is a deliberate, correct
  distinction (POSIX path on the host vs the operator's own machine
  where `\` counts), not drift -- preserved. Move 2 complete.

## Move 3 decisions (2026-09-17)

Split into commits like move 2. Part A (firewall doc) is clean and
prescribed; part B (tutorial cleanup) is a cluster of judgment calls
and gets its own commit and its own decisions.

- **Commit 1 (this one): firewall doc split.** Lifted the ~440-line
  nftables/network-isolation section (was `vm-host-setup.md` lines
  352-791) into a new `docs/vm-host-firewall.md`, promoting its
  heading levels and adding a one-line back-link to
  `vm-host-setup.md`. Left a short stub + pointer in
  `vm-host-setup.md` (835 -> 401 lines). Repointed the seven inbound
  references that named the old section: `trust-boundary.md` (x2),
  `tutorial.md` (x3, including a new "Where to go next" bullet), and
  `vm-host-wsl2.md` (x2).
- **Commit 2 (next): tutorial cleanup.** The four related items --
  `tutorial-box-lacks-git` (a correctness fix: two passages still
  assume the git-less Debian box), `tutorial-debian-box-warning`
  (keep/reword/cut a digression), `tutorial-provision-git-warning`
  (reword one awkward sentence), and `readme-vagrantfile-pointer`
  (a stale README pointer). These are not a clean "move digressions
  to usage.md"; they need their own decisions.

Noted for a later move (out of scope here): `vm-host-wsl2.md` still
says "The diary does not say which probe either run used" -- a
dangling reference to the removed `DIARY.md`. Belongs with the
stale-record cleanup, not the firewall split.

## Move 3 progress log

- 2026-09-17: commit 1 (firewall doc split) done. New
  `docs/vm-host-firewall.md`; `vm-host-setup.md` down to install +
  verify plus a stub; all inbound links repointed;
  `cargo xtask validate` green. Tutorial cleanup still pending.
- 2026-09-17: commit 2 (tutorial cleanup) done. Modernized the
  tutorial off its Debian-box legacy: trimmed the ~40-line
  why-not-Debian aside (was lines 445-483) to a six-line happy-path
  note, reworded the `chsh` comment and the arrow-key troubleshooting
  entry to be box-agnostic (the `chsh` code was already a safe
  conditional), and the awkward "provision.sh cannot save you"
  sentence went with the trim. `readme-vagrantfile-pointer` needed no
  edit -- the stale pointer was already gone (removed when the README
  was trimmed in `d0df54e`), verified now. All four related items
  moved to Done. Move 3 complete.
