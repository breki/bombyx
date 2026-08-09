---
description: Upgrade third-party Rust dependencies within the cooldown
allowed-tools: Bash(cargo update:*), Bash(cargo xtask:*), Bash(git status:*), Bash(git diff:*), Read, Edit, AskUserQuestion
---

Upgrade third-party Rust libraries while respecting the
14-day publish cooldown (see the **Supply-chain hygiene**
section of `CLAUDE.md`). The goal each pass: adopt the
newest version of each dependency that has **cleared the
cooldown**, so the `dep-age-check` gate passes with zero
overrides.

This is a Rust-only project -- there is no second ecosystem
to update, and `xtask` has no npm support.

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

Read-only: `cargo update --dry-run` lists within-semver
bumps. Major bumps need a manifest edit and are not shown.

Group the results into **safe** (same major version) and
**major** (a major-version jump, potentially breaking).

### 3. Decide scope (ask the user)

Use `AskUserQuestion`. Lead with the concrete lists from
step 2 (safe vs major). Offer:

- **Safe only** -- within-semver bumps.
- **Safe + majors** -- also the major-version jumps, each
  verified; call out the riskiest.

Do not proceed until the scope is chosen.

### 4. Upgrade

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
   loop -- it reads the changed crates and pins each fresh
   one to its newest aged version until the set converges --
   so you can run it in place of the manual steps 2-4 and
   inspect what it pinned.
5. Major bumps (a new major in `Cargo.toml`) are a
   deliberate, separate effort -- do them one crate at a
   time with the same cooldown discipline, not via
   `cargo update`.

### 5. Held-back (too-fresh) versions

Whatever the cooldown held back (e.g. a same-day release, a
brand-new major), **do not** silently drop it:

- Report each held-back `pkg@version`, its age, and the
  date it clears the cooldown (published date + 14 days).
- Adopt a still-fresh version **only** with an explicit,
  user-stated justification (or a security fix). When the
  user approves, record it in the commit and pass it through
  the gate via `RUSTBASE_DEP_AGE_ALLOW=pkg@ver[,...]`. Never
  add an allow entry without that stated reason.

### 6. Verify

Run `cargo xtask validate` (all gates, including `audit` and
`dep-age-check`). If a bump broke a gate, either back that
one bump out to its previous version or surface the failure
to the user -- do not force-commit a red gate.

### 7. Hand off to /commit

Invoke `/commit`. The change type is **`chore`** (no version
bump, no diary). In the summary the commit body should list:
the majors adopted, the count of crates advanced, any
`audit` warning cleared, and everything held back as too
fresh with its age-out date.

## Rules

- **The cooldown gate is authoritative.** The pass target is
  "newest version outside the 14-day window", computed by
  `cargo xtask dep-age cargo ... --latest-aged`, not
  "absolute latest".
- **Prefer scoped updates.** Advance named packages; avoid a
  blanket `cargo update` that churns the whole lockfile and
  floods the gate with fresh transitive bumps.
- **Never allow-list a fresh version without a stated
  justification.** Security fixes are the standing
  exception.
- **Never commit a red `validate`.** Back out the offending
  bump or escalate instead.
