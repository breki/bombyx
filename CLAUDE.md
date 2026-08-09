# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code)
when working with code in this repository.

**IMPORTANT: The working directory is already set to the
project root. NEVER use `cd` to the project root or
`git -C <dir>` -- blanket permission rules cannot be
set for commands starting with `cd` or `git -C`, so
they require manual approval every time.**

## Project Overview

bombyx drives isolated AI-agent VMs on a remote libvirt
host over SSH. It is the tooling half of the agent-VM
isolation strategy: the project repo holds the Vagrantfile
and is the source of truth, and bombyx pushes it to the VM
host and runs `vagrant` there.

- **Stack**: Rust CLI, no runtime services
- **Target platforms**: Windows (dev workstation), Linux
- **Control plane**: SSH wrapper around `vagrant` on the
  VM host. Deliberately thin -- see `README.md`.

### Workspace Crates

| Crate | Purpose |
|-------|---------|
| `crates/bombyx` | Core library and CLI binary |
| `xtask` | Build automation |

This is a CLI-only project: the template's optional web
crate, frontend, E2E suite and deploy subsystem have been
removed. `/template-sync` will default those paths to
"skip" on future syncs since they no longer exist locally.

## Build Commands

```bash
cargo xtask check             # fast compile check
cargo xtask validate          # fmt + clippy + tests + coverage
cargo xtask test [filter]     # tests only
cargo xtask test --ignored    # run #[ignore]-tagged tests
cargo xtask clippy            # lint only
cargo xtask coverage          # coverage only (>=90%)
cargo xtask fmt               # format code
cargo xtask dupes             # code duplication check
cargo xtask audit             # security-advisory audit (RUSTSEC)
cargo xtask dep-age cargo <pkg> [ver]  # one package's publish age
cargo xtask dep-age cargo <pkg> --latest-aged  # newest ver past cooldown
cargo xtask dep-age-check     # cooldown-gate changed deps (vs HEAD)
cargo xtask dep-preflight     # pin changed deps past cooldown pre-build
cargo xtask backfeed-diff <ds-path>      # downstream feedback since watermark
cargo xtask backfeed-record <ds-path> --watermark <date>  # advance watermark
cargo xtask feedback-add --section <s> --title <t>  # append feedback entry
cargo xtask sync-candidates <last-synced>  # categorized sync delta, filtered
cargo xtask changelog add --kind <k> [--breaking] "text"  # insert [Unreleased] bullet
cargo xtask todo <list|add|done> ...       # mechanical docs/todo.md edits
```

Never use raw `cargo test` or `cargo clippy` -- always
go through `xtask`.

### PowerShell Build Script

```powershell
.\build.ps1 validate      # cargo xtask validate
.\build.ps1 test          # tests only
.\build.ps1 build         # full build with all checks
.\build.ps1 clean         # clean artifacts
```

## Canon vs memory

Two places hold durable guidance, and they are not
interchangeable:

- **Canon** -- this `CLAUDE.md`, `.claude/` skills and
  commands. Tracked in git, reviewed, shared across machines
  and teammates and fresh clones.
- **Memory** -- per-user auto-memory (e.g.
  `~/.claude/.../memory/`). Per-machine, never committed,
  invisible to everyone else.

**Default to canon.** A rule others would benefit from --
a workflow convention, a project constraint, a lesson from
a review -- belongs in canon. Reserve memory for genuinely
user-specific items (one operator's preferences, their
role/background, freshly-captured corrections that have not
generalized yet). When a memory entry matures into a shared
rule, promote it to canon and delete the memory copy so the
two do not drift.

## Environment Constraints

Machine-level assumptions, so the assistant does not reach
for tools that are not present:

- **Node / npm / Playwright are not used.** This is a
  CLI-only project; the template's frontend and E2E suite
  were removed, and so was every piece of tooling that
  served them -- there are no `frontend-*` subcommands and
  nothing in `xtask` knows about npm. Do not invoke `npm`,
  `npx` or `playwright`.
- **`scripts/e2e.sh` does not exist.** The end-to-end check
  for this project is running bombyx against a real VM host
  (Definition of Done item 3), not a script.
- **The VM host is remote and not always reachable.** Any
  command that actually talks to it (`ssh`, `scp`, `vagrant`)
  may fail for reasons unrelated to the change under test.
  Prefer `--dry-run` for argv-level checks, and say so
  explicitly when a claim rests on a dry run rather than a
  real push.
- **Scripting**: use PowerShell, Bash, or Rust (`xtask`).
  Keep non-trivial logic in `xtask` -- see "Shell wrappers".

## Collaboration

- **Write plainly.** One idea per sentence; lead with the
  concrete example, then the rule; prefer plain words
  ("reminder" over "forcing function", "try again" over
  "iterate"); name the subject rather than leaning on "the
  first"/"the latter". Showy phrasing looks crisp but slows
  the reader.
- **Narrate the work as it happens.** Before each meaningful
  tool call or step, say in one short sentence what is about
  to happen and why. Do not batch silently and only speak at
  the end -- a run of silent tool calls reads as "lost".
  This holds regardless of the active output style.
- **Lead with context before a decision-making question,
  and show concrete artifacts** -- for a technical choice
  (grammar, API shape, data layout), write out what each
  option looks like (side-by-side snippets / diffs) *before*
  the `AskUserQuestion`. Option labels summarize choices the
  user has already seen, not the first encounter.
- **`AskUserQuestion`: explain in layman's terms, short.**
  The lead prose must be readable by a non-expert: no
  internal type names, file paths, or API names in the
  problem statement (save those for the option
  descriptions). It states *what the decision means*, not
  *how it is implemented*.
- **Recommend, do not survey.** When you have a defensible
  preference among the options, put it first and label it
  "(Recommended)", and give the one-line reason. An evenly
  weighted menu pushes the judgement back onto the user and
  usually costs a round-trip ("what do you recommend?").
  Ask without a recommendation only when the choice genuinely
  turns on preference or context you do not have.

## Coding Standards

- Rust edition 2024
- `#[deny(warnings)]` and `#[forbid(unsafe_code)]` via
  workspace lints
- Clippy pedantic where practical
- Error handling: `thiserror` for library errors,
  `anyhow` for CLI errors
- Prefer `&str` over `String` in function signatures
- All public items must have doc comments
- Wrap markdown at 80 characters per line
- Maximum code line width: 80 characters (`rustfmt.toml`)

## Test-Driven Development

TDD is the default discipline for functional changes,
but the strict red/green ceremony applies only where
it actually produces signal. Distinguish two cases:

**Behaviour change** -- new logic in existing code, a
bug fix in shipped code, a new state transition, an
edge-case branch in a function whose other branches
already have tests:

1. **Red** -- write a failing test that describes
   the expected behaviour
2. **Green** -- write the minimal code to make the
   test pass
3. **Refactor** -- clean up while keeping tests
   green

Here the pre-implementation test failure is real
signal: it proves the test actually exercises the
new path and that the surrounding code was indeed
not already covering it. Run `cargo xtask test`
after each step to confirm the cycle.

**Structural addition** -- a new self-contained
module, a new helper function, a new enum variant
with no callers yet, a new xtask subcommand with
embedded unit tests:

Write test and implementation together as a single
unit. The whole unit lands or doesn't. Strict
red/green here is theatre: the test and impl get
written together regardless, because the unit is
too small to meaningfully fail-then-pass, and the
`unimplemented!()`-stub-first dance adds no signal.

Scope this carve-out narrowly to **pure data
declarations** -- enums/structs with derived traits
and no behaviour. The moment a "new module" or
"new helper" carries real logic (an `apply`/`inverse`,
a branch, a match), it is a behaviour change: write
the failing test first, or you will ship uncovered
branches and miss cases the after-the-fact test would
have caught.

If you're unsure which case applies, default to the
behaviour-change discipline. The cost of an
unnecessary red step is low; the cost of skipping a
real red step (and shipping a test that always
passed) is high.

## Commits and releases

**All commits must go through the `/commit` skill.**
Never use `git commit` directly. No "Co-Authored-By",
no emoji. (The sole exception is `/release`, which makes
one direct bookkeeping commit for the version bump.)

Committing and releasing are separate:

- **`/commit`** is a save-point. It reviews, updates the
  diary and the `CHANGELOG.md` `[Unreleased]` block, and
  commits. It does **not** bump the version, touch
  `Cargo.lock`, or run `cargo xtask validate` -- multiple
  commits land between releases, and forcing each one to
  make a SemVer decision turns the version field into
  accounting rather than a description of what users run.
  `/commit` never runs `cargo xtask validate`; run it
  manually at your own shell when you want the full gate on a
  work-in-progress.
- **`/release`** is the sole version-bumper. It infers the
  bump from the accumulated `[Unreleased]` entries
  (`**BREAKING:**` or a non-empty `### Removed` -> major,
  `### Added` -> minor, else patch; override available),
  bumps `crates/bombyx/Cargo.toml`, promotes
  `[Unreleased]` to a dated section, runs
  `cargo xtask validate` as the **release gate**, commits
  the bookkeeping, and creates an **annotated** tag
  (`git tag -a vX.Y.Z`; a lightweight tag is invisible to
  the deploy guard's `git describe --exact-match`).
- **`cargo xtask deploy`** refuses to ship unless `HEAD`
  is on a `vX.Y.Z` annotated tag matching
  `crates/bombyx/Cargo.toml` and the working tree is
  clean -- so "publish to production" is tied to "cut a
  release" by the build, not by memory. Run `/release`
  first.

## Definition of Done

A task is done only when all of the following hold -- not
just when the code compiles:

1. **Targeted tests** for the change are written and pass.
2. **Type-check** passes (`cargo xtask check`).
3. **Verify against a real VM host** for anything that
   changes the commands bombyx emits. `--dry-run` proves
   the argv; it does not prove the remote side accepts it.
   "tests pass" is not "the command works".
4. **Self-review the diff** before committing.
5. **`cargo xtask validate`** passes (the umbrella gate).

`cargo xtask validate` checks:

1. **Formatting**: auto-fixed in place by default; pass
   `cargo xtask validate --check` for the read-only
   `cargo fmt --all -- --check` (use in CI or before
   partial staging, so an in-place rewrite does not sweep
   unrelated drift into the working tree)
2. **No warnings**:
   `cargo clippy --all-targets -- -D warnings`
3. **All tests pass**: `cargo test`
4. **Coverage >= 90%**
5. **Code duplication <= 6%** (production code, tests
   excluded)
6. **Security audit** (RUSTSEC; `cargo xtask audit`) --
   a positive vulnerability fails; an unreachable advisory
   DB degrades to a warning
7. **Dependency cooldown** (`cargo xtask dep-age-check`) --
   fails when a dependency added or bumped since `HEAD` was
   published within the 14-day window; an unchanged
   lockfile makes it a no-op

The dependency-cooldown gate runs **first** (it is a no-op
on an unchanged lockfile, and fails fast on a within-cooldown
dependency before anything compiles it); after it the gates
run cheapest-first (Fmt, Duplication, Clippy) before the
expensive dynamic gates (Tests, Coverage, then the network
Audit), and a failed step prints the single command to
re-run just that gate.

## Semantic Versioning

Follow [Semantic Versioning 2.0.0](https://semver.org/):

- **MAJOR** -- breaking changes
- **MINOR** -- new features, backwards-compatible
- **PATCH** -- bug fixes, documentation, internal refactors

The version lives in `crates/bombyx/Cargo.toml` and is
the **single source of truth**. `/release` is the only thing
that changes it; it computes the bump from the accumulated
`[Unreleased]` CHANGELOG entries (see "Commits and
releases").

## Release Notes

Maintain `CHANGELOG.md` using the
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
format. Group changes under: **Added**, **Changed**,
**Fixed**, **Removed**.

Always keep an `[Unreleased]` section at the top. `/commit`
appends bullets there (marking breaking changes with a
leading `**BREAKING:**`); `/release` promotes the whole
block to a dated `## [X.Y.Z] - YYYY-MM-DD` section and opens
a fresh empty `[Unreleased]` above it.

## Skills

| Skill | Purpose |
|-------|---------|
| `/check` | Fast compilation check (no tests) |
| `/test` | Run tests with agent-friendly output |
| `/validate` | Full quality pipeline with stepwise progress |
| `/commit` | Save-point commit with diary, CHANGELOG, and code review (no version bump) |
| `/release` | Cut a SemVer release: bump the version, promote `[Unreleased]`, validate, commit, and tag |
| `/retrospect` | Workflow retrospective (Efficiency / Quality / Speed / Cleanup). Invoked automatically by `/commit`; also callable manually mid-session |
| `/todo` | Capture a work item into `docs/todo.md` (no implementation) |
| `/implement` | Plan + implement a captured item; writes `docs/issues/<slug>.md` |
| `/update-deps` | Upgrade third-party deps to the newest versions outside the 14-day cooldown |
| `/simplify` | Review changed code for quality |
| `/architect` | Project overview and architecture guide |
| `/html-report` | Produce a self-contained local HTML report from the in-repo template (never a cloud Artifact) |
| `/template-improve` | Log feedback for the rustbase template |
| `/template-sync` | Sync upstream template changes |

## Template tooling: determinism vs judgment

The template-maintenance workflows (`/template-sync`,
`/template-backfeed`, `/template-improve`) split their work
into two kinds, and the split is load-bearing:

- **Determinism -- belongs in `cargo xtask`.** Delta
  determination (what changed since a watermark / SHA), log
  bookkeeping (appending an entry, minting an ID, dedup), and
  exclude-set filtering are mechanical. They must run as
  unit-tested `cargo xtask` commands, never as an LLM scan of
  a growing markdown file. An LLM re-reading a 2000-line log
  on every run is unbounded cost and drifts on format.
- **Judgment -- belongs to the LLM.** Categorizing a change,
  deciding apply/skip, merging code, writing prose. This is
  what the commands hand back to the agent.

Concretely: `backfeed-diff` (delta since the ledger
watermark), `backfeed-record` (advance the watermark),
`feedback-add` (append with a `tf-<date>-<slug>` ID), and
`sync-candidates` (categorized diff minus the never-sync set)
own the determinism; the slash commands own the judgment. When
extending these workflows, keep new mechanical work in xtask
with tests -- do not push it back into the prompt.

## Template Sync

This project tracks its template origin in
`.template-sync.toml`. Use `/template-sync` to pull
improvements from the upstream
[rustbase](https://github.com/breki/rustbase) template.
The command fetches upstream changes, then calls
`cargo xtask sync-candidates` to get a categorized file
delta with template-internal bookkeeping files already
filtered out, and helps you selectively apply relevant
updates while preserving your project's customizations.

## Template Feedback

This project was generated from the
[rustbase](https://github.com/breki/rustbase) template.
When you notice anything in the template-provided files
that is suboptimal, incorrect, outdated, or could be
improved, log it in `docs/developer/template-feedback.md`.

Examples of what to log:
- Dependency versions that needed immediate updating
- Config that didn't work out of the box
- Patterns that had to be reworked early on
- Missing features that every project ends up adding
- Conventions that turned out to be impractical
- Unnecessary boilerplate that was deleted

This feedback will be used to improve the template for
future projects.

The file uses three sections (see its header for
section semantics): **Open divergences** (gaps the
project intentionally keeps), **Resolved** (gaps closed
by retrofit work), and **Suggestions to flow back to
the template**. `/template-improve` routes new entries
into the appropriate section by calling
`cargo xtask feedback-add`, which mints a stable
`tf-<yyyy-mm-dd>-<slug>` ID, inserts at the section top,
and dedups -- the file is never hand-edited.

`/template-backfeed` (template repo only) pulls a
downstream's feedback back upstream. It uses a watermark in
`docs/developer/backfeed-ledger.toml` (one table per
downstream, machine-owned by `cargo xtask backfeed-record`)
so each run evaluates only feedback newer than the last, via
`cargo xtask backfeed-diff` -- it never re-scans the whole
downstream file.

## Workspace lints and xtask overrides

The workspace forbids `unsafe_code` via
`[workspace.lints.rust]` so production crates inherit
the policy by default. If a derived project needs OS-
specific code in `xtask/` (for example, calling Win32
APIs for process management on Windows -- the canonical
case being `OpenProcess` / `TerminateProcess` /
`CreateToolhelp32Snapshot` for stale-server cleanup),
the recipe is to redefine the lints block locally for
`xtask` only rather than weakening the workspace policy:

```toml
# xtask/Cargo.toml
[lints.rust]
warnings = "deny"
unsafe_code = "allow"   # xtask is build tooling, scoped exception

[lints.clippy]
# inherit the workspace clippy block by re-declaring
# or by overriding selectively
```

Production crates keep `[lints] workspace = true` and
remain `unsafe`-forbidden. Document the scoped
exception with a comment near the use site so reviewers
can verify the unsafe block is genuinely necessary.

## Coverage exceptions for hardware-bound code

The 90% coverage gate (see Definition of Done) assumes
every code path can run under `cargo llvm-cov` in CI.
Real projects routinely have I/O paths that can't:
audio playback, network calls against external
services, native API calls (Win32, CoreAudio, ALSA),
GPIO on embedded targets. The recipe for keeping the
gate honest without weakening it:

1. **Extract the hardware-bound code into a sibling
   submodule.** Given `foo.rs` that contains both
   business logic and an I/O call, split into `foo.rs`
   (the orchestrator) and `foo/bar.rs` (the I/O leaf).
   The leaf module should be as small as possible --
   ideally just the unmockable call plus its
   immediate error mapping.
2. **Exclude the leaf submodule via manifest config.**
   Add its path (a regex fragment) to
   `[workspace.metadata.coverage] ignore` in the **root
   `Cargo.toml`** -- no need to fork `xtask`:

   ```toml
   [workspace.metadata.coverage]
   # Each entry is a regex fragment merged into the coverage
   # --ignore-filename-regex baseline. Use single-quoted TOML
   # literal strings so backslashes reach the regex verbatim
   # (no doubling).
   ignore = ['src[/\\]audio[/\\]playback\.rs']
   ```

   `cargo xtask coverage` merges these with its built-in
   baseline (`src/main.rs`, `src/bin/`); the leaf module is
   exempted from the gate, the orchestrator is not. An absent
   section leaves the baseline unchanged, and a
   missing/unreadable manifest degrades to the baseline rather
   than failing. A pattern that would match *every* file
   (empty, `.`, `.*`, `.+`) is rejected -- it would silently
   neuter the gate. Only the `[workspace.metadata.coverage]` +
   line-leading `ignore = [...]` shape is read; the dotted-key
   (`coverage.ignore = ...`) and inline-table spellings are
   not.
3. **Add a `*_TEST_*` env-var escape hatch in the
   excluded module.** For example, `RUSTBASE_TEST_AUDIO`
   short-circuits the real native call and returns a
   fixed `Ok`/`Err` shape. This keeps the parent
   module's post-call success and error branches
   testable -- they're the parts that actually carry
   business logic, and they remain inside the 90% gate.

What this gets you: the orchestrator is fully covered
(including both branches of its `match
play_audio_native() { Ok => ..., Err => ... }`), the
leaf is honestly acknowledged as untested in CI, and
there's no `#[cfg(test)]` test-only branch leaking into
production code paths.

When NOT to use this recipe: if the I/O can be faked
with a trait + dependency injection at the call site
without contortions, do that instead. The submodule-
plus-ignore-regex pattern is for cases where the
indirection itself would obscure the code more than it
reveals.

## Shell wrappers: bash and PowerShell twins

This template targets Windows, Linux, and macOS as
first-class platforms. The convention for cross-shell
tooling is: **non-trivial logic lives in `cargo
xtask`; shell files (`scripts/*.sh`, `*.ps1`) are
thin wrappers only.** This keeps a bugfix from having
to land twice in two languages whose semantics drift
(quoting, exit codes, error handling).

The canonical wrapper shapes are:

```bash
# scripts/foo.sh
#!/usr/bin/env bash
set -euo pipefail
exec cargo xtask foo -- "$@"
```

```powershell
# scripts/foo.ps1
$ErrorActionPreference = 'Stop'
& cargo xtask foo -- @args
exit $LASTEXITCODE
```

Exceptions are allowed where the logic genuinely
can't live in Rust without contortion -- e.g.
process-cleanup that pokes `Get-CimInstance` or
`pkill` directly, or bootstrap scripts that run
*before* `cargo` is available. Document such
exceptions inline so the next reader knows why the
file is not a wrapper.

## Long-running scripts

For any script that runs more than ~30 seconds
(`scripts/e2e.sh`, dogfood/deploy helpers):

- **Author side** -- tee stdout to `target/<name>.log` so
  the output is durable (a captured caller, CI, or a closed
  terminal otherwise loses it). With the
  `exec > >(tee "$LOG") 2>&1` idiom you must also capture
  `TEE_PID=$!` and `wait "$TEE_PID"` in the `EXIT` trap --
  bash does not synchronize with `>(...)` process
  substitution on exit, so the trailing trap output (often
  the most important lines) is silently truncated without
  the wait.
- **Caller side** -- **never pipe a long-running command
  through `tail -N` under a tight timeout.** `tail -N` says
  "give me the end"; the timeout says "there will be no
  end" -- it buffers until EOF that never comes within the
  window, so the pipeline shows nothing and reads as a
  stall. Use `run_in_background` for the completion
  notification, or a `Monitor` with a line-buffered grep for
  progress; reserve `| tail -N` for already-finished
  commands.

## Lints: `doc_markdown` allowlist via `clippy.toml`

The workspace runs clippy with pedantic lints enabled
where practical. `clippy::doc_markdown` flags
identifiers like `PowerShell`, `JSON`, `FFI`,
`WebSocket`, `macOS`, `GitHub` in doc comments,
forcing every occurrence to be backticked even when
the prose reads naturally without backticks.

The template ships a `clippy.toml` at workspace root
with a curated `doc-valid-idents` allowlist of
infrastructure terms. The list extends clippy's
defaults (via the `".."` sentinel as the first entry)
rather than replacing them. Derived projects should
**append** their own domain-specific identifiers
(product names, acronyms, external systems) to that
file rather than redefining the list.

## Edition-2024 migration notes

The template ships on Rust edition 2024. Projects
inheriting from an older snapshot of the template (or
upgrading from edition 2021) routinely hit a small set
of mechanical fixes that `cargo fix --edition` either
applies automatically or flags:

- **Unsafe extern blocks**: `extern "C" { fn foo(); }`
  must become `unsafe extern "C" { fn foo(); }`. Each
  declaration inside is still individually `unsafe fn`.
- **Match ergonomics tightening**: bare `ref` patterns
  inside a binding that already implies a reference
  must be dropped. `match x { Some(ref y) => ... }`
  becomes `match x { Some(y) => ... }` when the outer
  match already produces a reference.
- **`gen` is reserved**: any identifier called `gen`
  (variables, function names, struct fields) needs the
  raw-identifier form `r#gen` or a rename.
- **Nested `if let` -> let chains**: clippy's autofix
  collapses `if x { if y { ... } }` into
  `if x && y { ... }` once `let`-chains are stable.
  This is a clippy fix rather than an edition fix, but
  it lands at the same time and is worth running in the
  same pass.

Run `cargo fix --edition --workspace` followed by
`cargo xtask validate` and expect a small follow-up
pass for the items above.

## Version source of truth

The project version lives in
`crates/<name>/Cargo.toml`. Avoid putting the version
number in README body text or other markdown — those
copies drift silently from `Cargo.toml`. If a version
mention is unavoidable in user-facing prose, embed it
as a sentinel comment (`<!-- version: 0.5.0 -->`) so a
script can rewrite both on release, or pull the value
from `Cargo.toml` via the build -- a CLI binary can use
`env!("CARGO_PKG_VERSION")`.

## Supply-chain hygiene

Four `cargo xtask` commands guard the dependency tree:

- **`cargo xtask audit`** runs `cargo audit` (RUSTSEC) over
  `Cargo.lock`, failing on any vulnerability (advisory
  *warnings* -- unsound / unmaintained / yanked -- are
  reported, not fatal). It runs late in `validate`, so
  **`validate` needs `cargo-audit` installed
  (`cargo install cargo-audit`) and network access** to the
  advisory DB.
- **`cargo xtask dep-age cargo <package> [version]`**
  reports how many days ago a version was published (on-demand,
  a single package). Add **`--latest-aged`** to instead print
  the **highest** version that has cleared the cooldown
  (selected by version, not publish date) -- the pin target the
  `/update-deps` workflow feeds to `cargo update --precise`.
  The `cargo` argument names the registry; it is the only one
  supported, and is kept so adding a second later needs no
  change to the command line.
- **`cargo xtask dep-age-check`** enforces the cooldown as the
  **first** `validate` step, so a dependency adopted within the
  cooldown fails the gate before the compile steps (Clippy,
  Test, Coverage) build and run its build script. It checks
  **only the dependencies added or version-bumped in the working
  tree versus `HEAD`**, so it fires exactly when a dependency is
  adopted and costs nothing -- no network -- on a commit that
  leaves `Cargo.lock` untouched. A *whole-tree* gate is
  deliberately avoided: it would flag every already-locked
  version on every routine update. Like `audit`, an
  unreachable registry / missing `HEAD` baseline degrades to a
  warning, not a hard failure.
- **`cargo xtask dep-preflight`** is the *pre-compile* twin of
  `dep-age-check`. Where the gate reports a cooldown breach
  *after* the fact, preflight *remediates* it *before* you
  build: it reads the changed Rust crates (same `HEAD` diff as
  the gate) and, for each one still inside the cooldown, pins
  it down to its newest aged version with
  `cargo update --precise`, looping until the whole changed set
  is aged or no aged version fits the resolved requirements.
  Every step touches only the registry index and the lockfile,
  so no crate tarball is fetched and no build script runs until
  the tree is clean. Use it as a front door: `cargo add <dep>`
  (updates the lockfile, no compile) -> `cargo xtask
  dep-preflight` -> `cargo build`. Rust / crates.io only.

**Why both a gate *and* a preflight?** `dep-age-check` is a
*post-resolution* check -- by the time it fails, `cargo` has
already downloaded *and compiled* the fresh crates, running
their build scripts (`cc`, `ring`, ...) on your machine. The
gate protects the committed lockfile (and everyone who builds
from it); it does not protect the build host during the window.
`dep-preflight` closes that host-side gap, but only when you
run it *instead of* going straight to `cargo build` -- it
cannot intercept a bare `cargo build` that resolves and
compiles in one shot. The only thing that protects *every*
invocation automatically is cargo's in-resolver
**`-Zmin-publish-age`** (RFC 3923), which refuses to *select* a
too-new version so it is never fetched or built. That flag's
client side is nightly-only as of now; once it stabilizes on
stable, layer it in front of (or in place of) these xtask
commands. Until then, `dep-age-check` is the CI-enforced gate
(runs on stable) and `dep-preflight` is the opt-in host-side
hardening.

**Dependency-version cooldown.** Do not adopt a dependency
version published fewer than 14 days ago without a stated
justification -- that window is when a compromised or
malicious release is most likely still live. Security fixes
are exempt (the fix's urgency outweighs the cooldown). Check
a candidate before adding it:
`cargo xtask dep-age cargo <crate> <version>`; it exits
non-zero when the version is within the cooldown.

`validate` enforces this automatically for changed deps via
the `dep-age-check` step above. When you *do* adopt a
fresh version with justification (or a security fix), name it
in the **`RUSTBASE_DEP_AGE_ALLOW`** env var
(`name@version`, comma-separated) so the gate passes while
leaving an auditable record of what was waved through --
e.g. `RUSTBASE_DEP_AGE_ALLOW=serde@1.0.999 cargo xtask
validate`.

**`cargo update` interaction.** The gate checks *every*
newly-locked registry dependency, **transitive ones included**
-- so it is a no-op only on commits that leave `Cargo.lock`
untouched, not on every "routine" commit. A lockfile-churning
update (`cargo update`) can bump many transitive crates to
versions published within the cooldown, and the gate
will fail listing all of them. That is intended -- a bulk
update is exactly when a freshly-published (possibly
compromised) transitive release slips in. The recommended
workflow: run the update, then either wait out the cooldown
before committing, or, once you've reviewed the flagged
versions, bulk-approve them with
`RUSTBASE_DEP_AGE_ALLOW=a@1.2.3,b@4.5.6,... cargo xtask
validate`. Prefer scoped updates (`cargo update -p <crate>`)
over a blanket `cargo update` so the flagged set stays small
and reviewable.
