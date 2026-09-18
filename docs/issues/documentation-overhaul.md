# documentation-overhaul

**Status:** All five moves complete (2026-09-17), but the
acceptance `/docs-audit` ran and **acceptance is not met**: five
files still rate mannered Heavy. Remaining work is a punch-list,
not a move -- see "Audit findings and remaining work" at the end.
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
  `docs/issues/` pruned to the working docs; `project-config-off-repo.md`
  was kept while in progress and removed on 2026-09-17 once a coverage
  check confirmed its durable reasoning lives in `architecture.md`,
  `trust-boundary.md`, and `llms.txt`.
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
stale-record cleanup, not the firewall split. Captured as
`wsl2-doc-diary-ref` in `docs/todo.md`.

## Move 4 note (2026-09-17)

Move 4 ("tag and collapse the backlogs") turned out to hinge on
`record-files-typed-header`, which is a code project, not a docs edit.
It has its own working doc, `docs/issues/record-files-typed-header.md`,
and two operator decisions reshaped it: the record files hold LIVE
work only (dropping done/closed entries -- which IS most of "collapse
the backlogs"), and the cross-reference gate checks durable IDs only.
The design and a four-increment plan are committed there; the code was
deliberately banked for a fresh, focused session rather than rushed at
the end of this one. Move 5 (house style + length budget) still
remains, and the move-1 length-estimate clause lands there.

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

## Move 5 decisions and progress (2026-09-17)

- **House style stated once, in `CLAUDE.md` under Writing.** The
  Writing section already carried the mechanism-first, plain,
  reason-not-history rules. Move 5 added the missing principle
  explicitly: state the rule, not the war-story that taught it --
  one clause of reasoning where it prevents a mistake, the incident
  dropped (git history keeps it), with
  `docs/developer/supply-chain.md` named as the model. It is stated
  in the always-loaded manual rather than a new file, so a fresh
  agent meets it every session; it governs the whole corpus, though
  a file whose job is carrying an argument upstream
  (`template-feedback.md`) keeps more of its rationale by nature.
- **Length budget on the manual: near 900 lines.** `CLAUDE.md` is
  always loaded, so it pays for every session. The budget is a
  documented soft cap, not a gate -- the manual reminds the agent to
  move reference detail or a retold incident to `docs/` / `llms.txt`
  and leave the rule. `wc -l CLAUDE.md` is the check. Chosen near
  the current size (876 -> ~888 after this move); move 1 targeted
  600-650 and landed at 866, which the operator accepted as
  mechanism and reference, not manner, so the cap protects that
  level rather than the original estimate.
- Move 5 done. `canon-check` and `cargo xtask validate` green.
  Remaining before the program's acceptance and the removal of both
  plan docs: the deferred broad `template-feedback.md` rationale
  trim (move 4), then a final `/docs-audit` showing no file rated
  mannered Heavy.

## Audit findings and remaining work (2026-09-17)

A `/docs-audit` fan-out (six read-only assessors over all 22 files)
ran as the acceptance measure. Tally: 9 Keep, 10 Trim, 3 Delete.
Structure is where the program aimed -- one home per topic, the
quickstart -> tutorial -> usage ladder, reference split out of the
guides. But **acceptance is not met**: it asks for no file rated
mannered Heavy, and four still are -- `docs/architecture.md`,
`docs/developer/template-feedback.md`, `docs/developer/redteam-log.md`
and `docs/developer/artisan-log.md`. (A fifth,
`docs/issues/project-config-off-repo.md`, also rated Heavy and has
since been deleted -- item 1 below.) So the deferred
template-feedback trim alone does not close the program.

The remaining work, in value order. Each is a docs edit and its own
commit; capture the untracked ones in `docs/todo.md` before
starting.

1. **Delete `docs/issues/project-config-off-repo.md`.** DONE
   2026-09-17: a coverage check confirmed all 11 durable facts live
   in `architecture.md`, `trust-boundary.md` and `llms.txt`, and
   the one still-open question is preserved as
   `destroy-confirmation-shape`; the 827-line doc was removed and
   the one reviewer finding about it dropped.

2. **Refresh `docs/tutorial.md`.** It runs on `0.4.1` while the
   shipped version is `0.6.0`, and its header caveat calls the
   Vagrantfile-generation design "unreleased" -- now false, so an
   agent reads current behaviour as not-yet-shipped. Also collapse
   its dry-run and provision/snapshot passages, which re-explain
   `usage.md`, to cross-links. (Not yet tracked.)

3. **Broad `template-feedback.md` rationale trim** -- the deferred
   move-4 item. ~50-60% cut: keep the upstream suggestion plus a
   short rationale, drop the bombyx-internal incident narration and
   the superseded design bodies.

4. **Trim the reviewer-log bodies to the fact.** `redteam-log.md`
   and `artisan-log.md` still re-derive each finding with
   review-provenance war-stories; cut each to defect + repro +
   one-line deferral, the shape `fresh-reader-log.md` already has,
   and de-duplicate the artisan findings that merely restate
   `docs/todo.md` items. (Not yet tracked.)

5. **Trim the long `docs/todo.md` bodies** (e.g.
   `self-update-resolves-tar-late`, `config-home-env-provenance`)
   to the fact and the open decision, and fix the wrapped
   `**Summary:**` line in `config-tests-own-file`. DONE
   2026-09-18: seven entries trimmed (the two named, plus
   `config-tests-own-file`, `validate-resume-from-step`,
   `backlog-ids-dangle-in-docs`, `bootstrap-harness-runs-the-
   script`, `deploy-key-path-names-vagrant`), keeping every
   file/line detail and dropping review-provenance narration to a
   clause; the wrapped Summary fixed; a header clause added noting
   a body may be edited directly.

6. **Trim `architecture.md` and CLAUDE.md's mannered residue.**
   DONE 2026-09-18: the two `architecture.md` anecdotes (the
   `/root` toolchain run and the `homedirtest` provision) cut to a
   one-clause verification note, and six counting/throat-clear
   openers rewritten to lead with the concrete subject; CLAUDE.md's
   Environment Constraints lost the canon-check "once broke"
   incident and the "each sibling produced a false statement"
   clause. Mechanism kept throughout. `validate` green.

7. **Drop the drifted commands table in
   `docs/ai-agents/guidelines.md`** and point at CLAUDE.md's Skills
   table; it already disagrees with canon (wrong `/todo`
   description). DONE 2026-09-18: the five-row table replaced with
   a one-sentence pointer to the `## Skills` table, the heading
   kept; the Stop hook reference in the same file was verified
   accurate and left.

Lower-priority, already tracked: the `DIARY.md` dangle in
`vm-host-wsl2.md` (`wsl2-doc-diary-ref`) and the version-sentinel
drift (`sync-version-sentinels`). Also repeat the dated and
`*(unverified)*` markers next to the claims they qualify in the
vm-host and trust docs, not only in their headers.

The style to match: `quickstart.md`, `fresh-reader-log.md`,
`supply-chain.md`, `build-recipes.md`, `README.md`.

Captured 2026-09-17: the untracked items above are now queue
entries -- `delete-project-config-off-repo-doc` (1),
`tutorial-version-drift` (2), `trim-reviewer-log-bodies` (4),
`trim-todo-entry-bodies` (5), `trim-architecture-claude-mannered-
prose` (6) and `guidelines-commands-table-drifts` (7). Item 3 (the
broad `template-feedback.md` trim) stays tracked in this plan and
in `record-files-typed-header.md`.

The program closes -- and both plan docs are removed -- when a
re-run `/docs-audit` shows no file rated Heavy.
