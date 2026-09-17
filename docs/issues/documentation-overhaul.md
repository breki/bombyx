# documentation-overhaul

**Status:** Planning
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
