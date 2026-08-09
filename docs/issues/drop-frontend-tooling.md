# drop-frontend-tooling

**Status:** Done
**Captured:** 2026-08-09
**Started:** 2026-08-09
**Completed:** 2026-08-09

## Problem

bombyx was derived from the rustbase template and pruned to
a CLI-only project: the web crate, frontend, E2E suite and
deploy subsystem were removed. The *tooling* for those parts
was not. Five `xtask` modules, two shell scripts and three
`.gitignore` blocks still serve a frontend that does not
exist and cannot be made to exist in this repo.

Nothing in the project can exercise any of it, so it is dead
weight that every future `/template-sync` has to reconcile.

## Context

### What is actually dead

| Path | Lines | Why dead |
|------|-------|----------|
| `xtask/src/frontend.rs` | 140 | npm helper; only the four below use it |
| `xtask/src/frontend_check.rs` | 17 | `svelte-check` wrapper |
| `xtask/src/frontend_fmt.rs` | 21 | Prettier wrapper |
| `xtask/src/frontend_dupes.rs` | 24 | `jscpd` wrapper |
| `xtask/src/frontend_test.rs` | 20 | `vitest` wrapper |
| `scripts/kill-servers.sh` | 22 | frees ports 3000/5173; no servers here |
| `scripts/lib/port-utils.sh` | 70 | only consumer is `kill-servers.sh` |

`xtask/src/main.rs` carries the wiring: five `mod`
declarations (lines 12-16), four `XCommand` variants
(lines 92-108) and four dispatch arms (lines 210-215).

### What only *looks* dead

- **`xtask/src/sync.rs:57`** lists `crates/`, `frontend/`
  and `e2e/` as `BOILERPLATE_PREFIXES`, and its tests assert
  on `frontend/src/App.svelte` and `scripts/e2e.sh`. These
  classify paths in the **upstream rustbase** diff, which
  still has a frontend. They must stay, or `/template-sync`
  mis-categorises upstream changes. Deleting them would be
  the obvious-looking mistake here.
- **`xtask/src/audit.rs`** runs `npm audit` only when
  `frontend/package.json` exists (line 137), so it already
  skips cleanly.
- **`xtask/src/dep_age*.rs`** support an `npm` ecosystem
  argument. `cargo xtask dep-age npm <pkg>` is a documented
  interface in `CLAUDE.md`, and the cooldown gate is a no-op
  without a `package-lock.json`.

### Verified as unreferenced

Nothing outside the files themselves mentions
`kill-servers`, `port-utils`, or any `frontend-*` subcommand
-- not `CLAUDE.md`, `llms.txt`, `build.ps1`,
`.claude/settings.json`, nor `.claude/hooks/stop-check.sh`.
The only hit is this item's own entry in `docs/todo.md`.

`cargo xtask validate` does **not** call any frontend step;
`validate.rs` mentions npm only in a doc comment.

### Constraints

- `xtask` is build tooling; its unit tests run under
  `cargo xtask test`. Deleting `frontend.rs` removes three
  passing tests, which is expected, not a regression.
- The 90% coverage gate measures `crates/bombyx`, so
  removing `xtask` files does not move it.

## Open questions

Both resolved -- see Decisions.

## Plan

**Part A -- the dead modules**

1. Delete the five `xtask/src/frontend*.rs` modules.
2. In `xtask/src/main.rs`, remove the five `mod`
   declarations, the four `XCommand` variants
   (`FrontendCheck`, `FrontendFmt`, `FrontendDupes`,
   `FrontendTest`) and their four dispatch arms.
3. Delete `scripts/kill-servers.sh` and
   `scripts/lib/port-utils.sh`; remove `scripts/lib/` if it
   is then empty.
4. Prune the frontend-only `.gitignore` blocks: Playwright
   (`/test-results/`, `/playwright-report/`,
   `/playwright/.cache/`), E2E test data (`/test-data/`) and
   root-level Node (`/node_modules/`, `/package-lock.json`).

**Part B -- purge npm from the surviving tooling**

5. `xtask/src/audit.rs`: remove `NpmAudit`,
   `parse_npm_audit`, `run_npm_audit` and the npm arm of
   `classify_audit`, plus their tests. The step becomes
   RUSTSEC-only; keep the existing degrade-to-warning
   behaviour for an unreachable advisory DB.
6. `xtask/src/dep_age.rs`: remove `Ecosystem::Npm`,
   `npm_version_date`, `npm_versions`, the npm registry URL
   arm, the scoped-name (`@scope/name`) URL encoding that
   exists only for npm, and their tests. `Ecosystem` keeps
   its `Cargo` variant (see decision 2).
7. `xtask/src/dep_age/gate.rs`: remove `parse_npm_lock`, the
   `frontend/package-lock.json` lockfile entry, the
   `Ecosystem::Npm` match arm and their tests. The gate then
   watches `Cargo.lock` only.
8. `xtask/src/dep_age/preflight.rs`: update the doc comment
   that describes the npm tree as out of scope -- it is now
   the only tree.

**Part C -- docs and guard rails**

9. Add a comment at `sync.rs:57` recording *why*
   `frontend/` and `e2e/` stay in `BOILERPLATE_PREFIXES`
   (they classify the **upstream** template diff), so the
   next cleanup pass does not remove them.
10. `CLAUDE.md`: change `dep-age <eco> <pkg>` usages to
    `dep-age cargo <pkg>`, drop npm from the `audit` and
    `dep-age-check` descriptions, and drop the npm/frontend
    references from "Supply-chain hygiene".
11. `llms.txt`: same treatment if it enumerates the
    commands.

## Test strategy

This is a deletion of unreachable code, so the test work is
proving nothing else depended on it rather than adding cases.

- **No new unit tests.** The deleted modules take their own
  tests with them; there is no surviving behaviour to cover.
- **`cargo xtask validate`** is the real gate: it must still
  pass all seven steps, which proves `main.rs` still
  compiles, clippy is clean with no dead-code or unused-
  import warnings, and the `xtask` suite is green.
- **`cargo xtask sync-candidates`** must still categorise an
  upstream `frontend/...` path as Boilerplate -- this is the
  regression the plan's step 5 guards against, and it is
  already covered by the existing tests in `sync.rs`.
- Confirm `cargo xtask --help` no longer lists the four
  `frontend-*` subcommands.

Part B touches code that *is* live, so it needs more than a
compile:

- The surviving `audit.rs` and `dep_age` tests must still
  pass unchanged -- only the npm-specific cases are deleted.
  Any Rust-path test that needs editing is a signal the
  change reached further than intended.
- Exercise both surviving commands for real, since their
  behaviour is what the removal could silently break:
  `cargo xtask dep-age cargo serde` and
  `cargo xtask dep-age cargo serde --latest-aged` (network),
  and `cargo xtask audit`.
- `cargo xtask dep-age-check` must stay a clean no-op on an
  unchanged `Cargo.lock` and must not error now that it
  watches a single lockfile.

## Decisions

- **2026-08-09 -- Purge npm everywhere, not just the dead
  modules.** Asked whether `audit.rs`'s npm path and the
  `dep_age` npm ecosystem should survive, given both already
  degrade cleanly (the audit is gated on
  `frontend/package.json` existing; the gate is a no-op with
  no `package-lock.json`). Recommended keeping them for the
  smaller diff. **Chosen: remove all npm traces**, for a
  Rust-only toolchain with no half-supported second
  ecosystem. Accepts a larger diff into working, tested
  code.
- **2026-08-09 -- Keep the ecosystem argument on
  `dep-age`.** With npm gone, `Ecosystem` has one variant
  and the CLI argument only accepts `cargo`. Asked whether
  to drop the argument (`dep-age serde 1.0.200`) or keep it
  (`dep-age cargo serde 1.0.200`). Recommended dropping it,
  to avoid a vestigial one-variant type. **Chosen: keep the
  argument**, so the command line does not change and adding
  a second ecosystem later needs no CLI change. `Ecosystem`
  therefore stays as a single-variant enum.

## Progress log

- **2026-08-09** -- Part A (dead modules, scripts,
  `.gitignore`) landed; `cargo xtask check` clean.
- **2026-08-09** -- Part B (npm purge from `audit.rs`,
  `dep_age.rs`, `gate.rs`, `preflight.rs`) landed.
- **2026-08-09** -- Part C (docs, `sync.rs` guard comment)
  landed; `cargo xtask validate` green; surviving commands
  exercised against the live registry.

## Outcome

**784 lines deleted, 328 added** (the additions are almost
entirely this document). `cargo xtask validate` passes all
seven steps: clippy clean, 100% coverage, 0% duplication.

Removed outright:

- `xtask/src/frontend.rs`, `frontend_check.rs`,
  `frontend_dupes.rs`, `frontend_fmt.rs`, `frontend_test.rs`
- `scripts/kill-servers.sh`, `scripts/lib/port-utils.sh`
  (`scripts/` is now gone entirely)
- `xtask/src/main.rs`: 5 `mod` decls, 4 `XCommand` variants,
  4 dispatch arms
- `.gitignore`: the Playwright, E2E-test-data, Node and
  `frontend/` blocks, plus `.ports` -- which only
  `port-utils.sh` ever read

npm purged from the surviving tooling:

- `xtask/src/audit.rs:1` -- `NpmAudit`, `parse_npm_audit`,
  `run_npm_audit` and the npm arm of `classify_audit` gone;
  the function now takes one argument instead of two. Added
  `classify_advisory_warnings_are_not_fatal` to keep the
  advisory-warning branch covered after the npm tests went.
- `xtask/src/dep_age.rs:54` -- `Ecosystem` is now a
  single-variant enum (decision 2); `npm_version_date` and
  `npm_versions` deleted.
- `xtask/src/dep_age/gate.rs` -- `parse_npm_lock` and the
  `frontend/package-lock.json` entry gone; the gate watches
  `Cargo.lock` only.

**Guard rail added** at `xtask/src/sync.rs:57`: a comment
recording that `frontend/` and `e2e/` must stay in
`BOILERPLATE_PREFIXES` because they classify the *upstream*
rustbase diff, not local paths. This was the one real trap
in the change -- deleting them looks correct and would
silently mis-bucket upstream frontend changes.

**Verified live**, not just by tests:

- `cargo xtask --help` no longer lists any `frontend-*`
  command.
- `cargo xtask dep-age cargo serde` -> `1.0.229`, published
  22 days ago, past cooldown.
- `cargo xtask dep-age cargo serde --latest-aged` ->
  `1.0.229`.
- `cargo xtask dep-age npm vite` -> `error: invalid value
  'npm' for '<ECOSYSTEM>' [possible values: cargo]`, so the
  removal is a clean CLI rejection rather than a panic.
- `cargo xtask audit` -> `cargo: 0 vuln, 0 warn` (no npm
  segment).
- `cargo xtask dep-age-check` -> clean no-op, exit 0.

### Follow-ups

- `.gitignore` still carries `.deploy`, and `CLAUDE.md`
  still documents `cargo xtask deploy` as the thing that
  gates releases -- but no `deploy` subcommand exists in
  `xtask`. That is fallout from the template's deploy
  subsystem being pruned, not from this change, so it was
  left alone. Worth its own `/todo`.
