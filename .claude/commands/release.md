---
description: Cut a SemVer release from accumulated [Unreleased] CHANGELOG entries
allowed-tools: Bash(git status:*), Bash(git diff:*), Bash(git log:*), Bash(git add:*), Bash(git commit:*), Bash(git tag:*), Bash(git describe:*), Bash(cargo xtask validate*), Bash(cargo update:*), Read, Edit, AskUserQuestion
---

Cut a SemVer release: bump the version, promote the
`[Unreleased]` block in `CHANGELOG.md` to a dated release
section, validate, commit, and tag.

A release here is the tag and nothing more. bombyx is a
CLI installed with `cargo install`, so there is no deploy
step for a release to gate.

## Usage

```
/release                # infer bump from CHANGELOG, then ask
/release patch          # force patch bump
/release minor          # force minor bump
/release major          # force major bump
```

## Instructions

1. **Check working tree is clean** -- Run `git status`.
   Refuse if there are unstaged or uncommitted changes
   other than ones this skill is about to introduce
   (`Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`). Reason:
   a release commit should contain only the release
   bookkeeping; mixing it with substantive code makes
   the tag dishonest about what was reviewed.

2. **Check `[Unreleased]` has content** -- Read
   `CHANGELOG.md`. If the `[Unreleased]` section is empty
   or contains only empty subheadings, abort with
   "nothing to release". Reason: a version number is a
   description of what changed, and bumping it when
   nothing did makes it bookkeeping instead. The existing
   tag already names this code.

3. **Determine the bump:**
   - If the user passed `major` / `minor` / `patch`, use
     that. Skip to step 4.
   - Otherwise infer from the `[Unreleased]` headings:
     - Any bullet starting with `**BREAKING:**` -> **major**
     - Any non-empty `### Removed` section -> **major**
     - Any non-empty `### Added` section -> **minor**
     - Otherwise -> **patch**
   - Show the user the inference (with the bullets that
     drove it) and ask via `AskUserQuestion` whether to
     accept the inferred bump or override it. Options:
     the inferred bump (recommended), the other two
     levels, and "Abort".

4. **Compute the new version:**
   - Read the current version from
     `crates/bombyx/Cargo.toml`.
   - Apply the bump (major: `X+1.0.0`; minor: `X.Y+1.0`;
     patch: `X.Y.Z+1`).
   - Today's date in ISO format (`YYYY-MM-DD`) -- use
     the date the system provides; do not hardcode.

5. **Edit `crates/bombyx/Cargo.toml`** -- update the
   `version = "X.Y.Z"` line.

6. **Sync `Cargo.lock`** -- run `cargo update -p bombyx`
   (updates only that package's entry). Do **not** use
   `cargo generate-lockfile` -- it refreshes every
   transitive dependency, folding an unrelated
   workspace-wide bump into the release commit and
   tripping the `dep-age-check` cooldown gate on freshly
   published transitive versions.

7. **Rewrite `CHANGELOG.md`:**
   - Rename the existing `## [Unreleased]` heading to
     `## [X.Y.Z] - YYYY-MM-DD`.
   - Insert a fresh empty `[Unreleased]` skeleton above
     it:
     ```
     ## [Unreleased]

     ### Added

     ### Changed

     ### Fixed

     ### Removed

     ## [X.Y.Z] - YYYY-MM-DD
     ```
     (Subheadings with no bullets can be left in place;
     they signal the categories for future entries.)

8. **Validate** -- run `cargo xtask validate`. This is
   the release gate. If it fails, abort and tell the
   user what failed; do not commit a broken release.

9. **Stage and commit:**
   - Stage `crates/bombyx/Cargo.toml`, `Cargo.lock`,
     `CHANGELOG.md` (and nothing else).
   - Commit directly with `git commit` (do **not** route
     through `/commit` -- the underlying changes were
     reviewed at their own commit time, and a release
     commit is a single-purpose bookkeeping commit; this
     is the documented exception to the "all commits go
     through `/commit`" rule).
   - Use this message format (HEREDOC):
     ```bash
     git commit -m "$(cat <<'EOF'
     release: vX.Y.Z

     <one-line summary derived from the [Unreleased]
     bullets being released>

     AI-Generated: Claude Code (<ModelName> <YYYY-MM-DD>)
     EOF
     )"
     ```

10. **Tag** -- create an **annotated** tag:
    `git tag -a vX.Y.Z -m "Release vX.Y.Z"`. Do **not**
    use a lightweight tag (`git tag vX.Y.Z`) -- the
    deploy guard runs `git describe --exact-match
    --match 'v*' HEAD`, which only sees annotated tags
    by default. Do not push; the user pushes when ready.

11. **Tell the user what to do next** -- print:
    - The new version and tag name
    - The CHANGELOG bullets that were released
    - "Push with `git push && git push --tags`" -- the
      tag is the release, so an unpushed one has shipped
      nothing

## Rules

- One release per commit. Never bundle a release with
  unrelated code changes.
- Never push tags automatically.
- Never edit closed (already-dated) release sections of
  `CHANGELOG.md`; only the `[Unreleased]` block is
  mutable.
- If `cargo xtask validate` fails after the version
  bump, leave `Cargo.toml`, `Cargo.lock`, and
  `CHANGELOG.md` modified on disk so the user can see
  the broken state, and do not commit or tag.
