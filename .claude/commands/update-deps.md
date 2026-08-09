---
description: Upgrade third-party dependencies (Rust + frontend) within the cooldown
allowed-tools: Bash(cargo update:*), Bash(cargo xtask:*), Bash(npm:*), Bash(npm --prefix:*), Bash(git status:*), Bash(git diff:*), Read, Edit, AskUserQuestion
---

Upgrade third-party libraries across both ecosystems
(Rust / `cargo`, frontend / `npm`) while respecting the
14-day publish cooldown (see the **Supply-chain hygiene**
section of `CLAUDE.md`). The goal each pass: adopt the
newest version of each dependency that has **cleared the
cooldown**, so the `dep-age-check` gate passes with zero
overrides.

This command upgrades dependencies and hands off to
`/commit`. It never bumps the project's own version.

## Instructions

### 1. Preconditions

- Start from a clean working tree (`git status`). A
  dependency upgrade should be its own commit(s), not mixed
  with unrelated edits.
- Note the date. The cooldown cutoff is **14 days ago**: a
  version is adoptable only if published on or before that
  date.

### 2. Assess what is outdated

Read-only, both ecosystems:

- Rust: `cargo update --dry-run` (lists within-semver bumps;
  major bumps need a manifest edit and are not shown).
- Frontend: `npm --prefix frontend outdated` (columns:
  Current / Wanted / Latest). `Wanted` is within-semver;
  `Latest` may be a major bump.

Group the results into **safe** (same major version) and
**major** (a major-version jump, potentially breaking).

### 3. Decide scope (ask the user)

Use `AskUserQuestion`. Lead with the concrete lists from
step 2 (safe vs major, per ecosystem). Offer:

- **Safe only** -- within-semver bumps, both ecosystems.
- **Safe + majors** -- also the major-version jumps (each
  verified; call out the riskiest, e.g. a compiler/toolchain
  major like TypeScript).
- **One ecosystem only** -- Rust or frontend.

Do not proceed until the scope is chosen.

### 4. Rust phase

1. `cargo update` for the within-semver set (respects
   semver; stays same-major).
2. `cargo xtask dep-age-check`. It reports every changed
   registry crate as `aged` / `fresh`. If any are **fresh**
   (within the cooldown), do not adopt them as-is.
3. For each fresh crate, find its pin target and apply it:
   ```
   V=$(cargo xtask dep-age cargo <crate> --latest-aged)
   cargo update -p <crate> --precise "$V"
   ```
   `--latest-aged` prints the **highest** version that has
   cleared the cooldown (selected by version, not publish
   date, so it never targets a recent backport on an older
   line). For a crate `cargo update` just moved into the
   cooldown, this is the newest safe version at or above the
   pre-update lock, so the crate still advances where it can.
4. Re-run `cargo xtask dep-age-check` until it is
   `0 fresh`. (Pinning one crate can pull a fresh transitive
   dep; repeat for any new arrivals.) `cargo xtask
   dep-preflight` automates this exact pin-and-re-resolve
   loop for the Rust tree -- it reads the changed crates and
   pins each fresh one to its newest aged version until the
   set converges -- so you can run it in place of the manual
   steps 2-4 and inspect what it pinned.
5. Major Rust bumps (a new major in `Cargo.toml`) are a
   deliberate, separate effort -- do them one crate at a
   time with the same cooldown discipline, not via
   `cargo update`.

### 5. Frontend phase

For each frontend package in scope:

1. Resolve the pin target:
   `cargo xtask dep-age npm <pkg> --latest-aged`. For a
   **major** bump this may return a lower major than the
   absolute latest (the latest is often too fresh) -- adopt
   the newest aged one and note what was held back.
2. Install it, **from inside `frontend/`** (not
   `npm --prefix` -- see the `file:`-dependency trap in
   `CLAUDE.md`):
   ```
   cd frontend && npm install <pkg>@<ver> [<pkg2>@<ver2> ...]
   ```
   A targeted install writes a caret range to
   `package.json` and the exact version to the lockfile,
   with minimal lockfile churn.
3. **Only if `npm install` reports `ERESOLVE`** on a major
   bump: delete `frontend/node_modules` +
   `frontend/package-lock.json`, then re-run
   `npm install` from inside `frontend/` (the documented
   major-bump reset).
4. **Restore the working directory to the repo root**
   before any `cargo xtask` call -- xtask's frontend
   detection is cwd-relative and a lingering `cd frontend`
   makes every frontend gate falsely report "no frontend".
5. `cargo xtask dep-age-check` again; re-pin any fresh
   transitive additions as in step 4.3.

### 6. Held-back (too-fresh) versions

Whatever the cooldown held back (e.g. a same-day release, a
brand-new major), **do not** silently drop it:

- Report each held-back `pkg@version`, its age, and the
  date it clears the cooldown (published date + 14 days).
- Adopt a still-fresh version **only** with an explicit,
  user-stated justification (or a security fix). When the
  user approves, record it in the commit and pass it through
  the gate via `RUSTBASE_DEP_AGE_ALLOW=pkg@ver[,...]`. Never
  add an allow entry without that stated reason.

### 7. Verify

Run `cargo xtask validate` (all gates, including `audit` and
`dep-age-check`). If a major bump broke a gate
(`svelte-check` under a TypeScript major is the usual
suspect), either back that one bump out to its previous
version or surface the failure to the user -- do not
force-commit a red gate.

### 8. Hand off to /commit

Invoke `/commit`. The change type is **`chore`** (no version
bump, no diary). In the summary the commit body should list:
the majors adopted, the count of Rust crates advanced, any
`audit` warning cleared, and everything held back as too
fresh with its age-out date.

## Rules

- **The cooldown gate is authoritative.** The pass target is
  "newest version outside the 14-day window", computed by
  `cargo xtask dep-age ... --latest-aged`, not "absolute
  latest".
- **Prefer scoped updates.** Advance named packages; avoid a
  blanket `cargo update` / `npm update` that churns the
  whole lockfile and floods the gate with fresh transitive
  bumps.
- **Never allow-list a fresh version without a stated
  justification.** Security fixes are the standing
  exception.
- **Restore cwd to the repo root after any in-`frontend/`
  npm command** before running `cargo xtask`.
- **Never commit a red `validate`.** Back out the offending
  bump or escalate instead.
