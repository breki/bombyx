# The validate gates

`cargo xtask validate` runs eleven gates. `CLAUDE.md` under
**Definition of Done** carries the short list -- each gate's name
and the command that re-runs it alone. This page is the detail:
what each gate checks, why they run in the order they do, and why
the doc gate runs rustdoc twice.

## The eleven gates

Listed in the order they execute, so the numbers match what the
run prints:

1. **Dependency cooldown** (`cargo xtask dep-age-check`) --
   fails when a dependency added or bumped since `HEAD` was
   published within the 14-day window; an unchanged lockfile
   makes it a no-op
2. **Formatting**: auto-fixed in place by default; pass
   `cargo xtask validate --check` for the read-only
   `cargo fmt --all -- --check` (use in CI or before partial
   staging, so an in-place rewrite does not sweep unrelated
   drift into the working tree)
3. **Canon claims** (`cargo xtask canon-check`) -- reads
   `CLAUDE.md`, `.claude/commands/` and `.claude/agents/` for
   its checks, and additionally scans the reference docs under
   `docs/` for the dangling-ID citation check
   below, skipping the record files and the working issue docs,
   which cite IDs as provenance rather than as live pointers.
   `.claude/skills/` and the non-citation content of `docs/` stay
   unchecked. It fails on five kinds of claim the tree does not
   support: a bold cross-reference introduced by "under" that
   names no heading in canon, a backticked repo path that does
   not exist, a command file telling the agent to run a `git`
   subcommand its own `allowed-tools` does not grant, prose past
   80 columns, and a cited backlog ID that is in no backlog. It
   reads markdown only, so it needs no compilation and runs
   before every gate that does
4. **Record files** (`cargo xtask records-check`) -- also
   markdown-only, and runs right after Canon. Reads the record
   files (the three reviewer logs and `template-feedback.md`) and
   fails on four kinds of defect: an entry id used twice across
   the set, a `**Label:**` outside the known set, a heading id of
   the wrong shape, and a `Depends on` / `Supersedes` id that
   names no entry. Only ids in those two fields are resolved, so a
   durable id in prose stays unchecked provenance
5. **Code duplication <= 6%** (production code, tests excluded)
6. **Licences, bans and sources** (`cargo xtask deny`) -- runs
   offline against `deny.toml`; a licence outside the allow-list,
   a banned crate or a non-crates.io source fails, and a missing
   `cargo-deny` is an error rather than a warning because there
   is no network here to be down
7. **No warnings**: `cargo clippy --all-targets -- -D warnings`
8. **Documentation builds and every doc link resolves**
   (`cargo xtask doc`) -- see "Doc gate" below
9. **`xtask`'s own tests pass** -- this step runs `-p xtask`
   only, which is why the run prints `Test (xtask only)`
10. **Coverage >= 90% overall and >= 85% per module** -- one
   file below the per-module floor fails the run even when the
   workspace figure passes. `xtask/src/coverage.rs` owns both as
   `OVERALL_THRESHOLD` and `MODULE_THRESHOLD`. This is also where
   the *workspace* tests run, under
   `llvm-cov --workspace --exclude xtask`, so the same tests are
   not compiled and run twice
11. **Security audit** (RUSTSEC; `cargo xtask audit`) -- a
   positive vulnerability fails; an unreachable advisory DB
   degrades to a warning

## Why that order

The cooldown gate is first because it is a no-op on an unchanged
lockfile and fails fast on a within-cooldown dependency **before
anything compiles it or runs its build script**. After it the
cheap static gates run, then the expensive dynamic ones, and the
network audit last. A failed step prints the single command to
re-run just that gate.

## Doc gate: two rustdoc passes, not one

`cargo xtask doc` runs rustdoc **twice** under
`RUSTDOCFLAGS=-D warnings`: once normally, and once with
`--document-private-items`. That is not a redundant second pass:
a broken doc link fails in one of two ways, and neither pass
catches both.

- A link **inside a private module** naming something not in
  scope. The public pass never renders a private module's docs,
  so it reports nothing at all.
- A **public page linking to a private item**. This is an error
  in the public pass and legal in the private one -- rustdoc even
  suggests `--document-private-items` to make it resolve.

Both cases were live in this repo when the gate was added, and
each was invisible to the other pass. The `-D warnings` is what
makes it a gate: rustdoc's link lints are warnings by default, so
a broken link otherwise builds cleanly and the docs quietly stop
navigating.
