# Template feedback

Issues, improvements, and observations about the
[rustbase](https://github.com/breki/rustbase) template.

This file uses three lifecycle sections, the same shape
adopted by Ledgerstone (a downstream project) and now
shipped with the template itself:

- **Open divergences** -- things the project knows are
  suboptimal, missing, or differently-shaped than the
  ideal template baseline. In a derived project these
  are intentional or pending differences from the
  template; in this template repo they are known
  template issues awaiting fix.
- **Resolved** -- entries closed out by a retrofit /
  fix commit. Keeps the history visible without
  cluttering the open list.
- **Suggestions to flow back to the template** -- in a
  derived project, this is where ideas live that the
  project wants to push upstream. In this template repo
  the section is informational (there is no upstream),
  but the structure is preserved so new entries route
  identically across template and derived projects.

`/template-improve` adds new entries by asking which
section they belong to.

---

## Open divergences

_None yet._

## Resolved

_None yet._

## Suggestions to flow back to the template

### tf-2026-08-13-commit-marks-breaking-before-checking-for-a-rele -- commit marks breaking before checking for a release

`/release` infers the version bump from the accumulated
`[Unreleased]` entries: a `**BREAKING:**` prefix or a non-empty
`### Removed` section means major. `/commit` step 5 explains how to add
those entries and says nothing about when *not* to.

That gap bites hardest on a pre-1.0 project, which is what every fresh
scaffold is. Here, a change removed a config key that had been
introduced two days earlier -- so it had never been released, there were
no tags at all, and the behaviour existed only inside the current
`[Unreleased]` block. Marking it breaking would have forced the first
ever release to 1.0.0 to protect users of a version that was never
published. The honest record was to correct the existing bullets
describing the old design, and the mistake was caught only because
someone thought to run `git tag`.

Three parts to the suggested clause, in `/commit`'s CHANGELOG step:

1. **Confirm the behaviour shipped before feeding the major-bump
   inference.** The check is `git tag` (no tags at all means nothing has
   ever been released) and, when tags do exist,
   `git log <latest-tag>..HEAD` or the `[Unreleased]` block -- behaviour
   introduced since the last release never shipped either.
2. **Guard both inputs, not just the flag.** `--breaking` and
   `--kind removed` both reach the major-bump inference, so a rule
   covering only the first is silently defeated by the second: avoid
   `--breaking`, file `--kind removed` for the same never-shipped
   behaviour, and the spurious major bump arrives anyway. The rule
   protects the inference, not the flag.
3. **Say that correcting a bullet is a hand edit.** The step above it
   tells the committer *not* to hand-edit `[Unreleased]` because the
   subsections sit far apart, and `cargo xtask changelog` offers only
   `add` -- no edit, no remove. So the instruction "correct the existing
   bullet" mandates exactly the operation the same step warns against,
   with no tooling for it. Either acknowledge it as the one sanctioned
   hand edit (and tell the author to re-check for a split subsection or
   a duplicated `### <kind>` heading afterwards), or add a
   `changelog amend` / `changelog remove` subcommand so it is
   mechanical like the rest.

Part 3 is the one worth weighing upstream: a `changelog` command that
can only append will keep pushing authors into unguarded edits of the
file it exists to protect.

### tf-2026-08-13-powershell-expands-dollar-vars-in-xtask-args -- powershell expands dollar vars in xtask args

The template targets Windows as a first-class platform and ships several
`cargo xtask` commands that take prose as an argument -- `changelog
add`, `feedback-add --title`, `todo add`. On Windows those arguments are
usually typed into a double-quoted PowerShell string, which PowerShell
expands before the program ever sees it.

The two halves fail differently, and the quieter one is worse. A
variable PowerShell knows, such as `$HOME`, expands to a path, so a real
home directory lands in a tracked file. One it does not know, such as
`$XDG_CONFIG_HOME`, expands to the empty string *silently*, leaving
plausible-looking text behind. Documenting where a config directory
lives, this project ran roughly:

```powershell
cargo xtask changelog add --kind added "... $XDG_CONFIG_HOME/bombyx
  or $HOME/.config/bombyx ..."
```

`CHANGELOG.md` received a bare `/bombyx` for the first and the
operator's real home path for the second -- so the entry was wrong and
carried a small privacy leak into a committed file, and needed a
follow-up edit to repair. Backslash does not escape `$` in PowerShell;
backtick does, which is not the reflex a Unix-trained author has.

Two suggestions, in order of value:

1. Add a line to the template's `CLAUDE.md` under **Environment
   Constraints**: text containing `$` that is passed to an xtask command
   must use a single-quoted PowerShell string, or a Bash heredoc. State
   both mechanisms -- known variable becomes a path, unknown variable
   becomes nothing -- because a reader told only "avoid `$HOME`" is
   still bitten by every undefined variable, which is the majority of
   cases when quoting POSIX paths on Windows. The expansion is invisible
   in the diff you are about to commit, so the rule has to be known in
   advance rather than learned from the symptom.
2. Consider having the xtask commands that write prose into tracked
   files warn when the text contains something shaped like a user home
   directory (`/home/<name>`, `/Users/<name>`, `C:\Users\<name>`). That
   catches this class of accident whatever shell produced it, and a
   changelog entry naming a developer's home path is almost never
   intended.

### tf-2026-08-13-xtask-check-skips-the-test-targets -- xtask check skips the test targets

`cargo xtask check` runs `cargo check --workspace
--message-format=short`, with no `--all-targets`. Cargo therefore
compiles the library and binary targets only, and skips tests, benches
and examples.

That makes the command silent about the most common way a change breaks
a workspace: an altered function signature. Here, changing
`Config::load` to take an extra parameter and return a tuple produced
`Check OK` from `cargo xtask check`, and five `E0061` errors from
`cargo xtask test` moments later. The template's `CLAUDE.md` advertises
`check` as the "fast compile check" and the Definition of Done lists
"Type-check passes (`cargo xtask check`)" as its second item, so the
gate does less than both places claim.

The fix is one argument, hoisted into a named const to match the
`clippy_cmd.rs` shape next door:

```rust
const CHECK_ARGS: &[&str] = &[
    "check",
    "--workspace",
    "--all-targets",
    "--message-format=short",
];
```

Verified by reintroducing the breakage in a test call site: `check` now
fails and names the file and line, where before it printed `Check OK`.
A unit test asserts `CHECK_ARGS` contains `--all-targets`, so a later
edit trimming the argv "to make check faster" cannot quietly undo it.

Two things worth carrying upstream with the flag itself:

1. **The wording has to change with it.** Six places in this project
   described `check` purely on speed -- `CLAUDE.md` twice, `llms.txt`
   twice, the clap help in `xtask/src/main.rs`, and
   `docs/ai-agents/guidelines.md` -- and "Fast compilation check (no
   tests)" now misleads twice over: the compile is no longer cheap, and
   "no tests" reads as "tests are ignored" when it means "tests are
   compiled but not run". The template ships all of those strings.
2. **`check` and `clippy` now overlap, and that is fine if stated.**
   `clippy` already passes `--all-targets` and type-checks as a
   prerequisite of linting, so `check` becomes a strict subset over the
   same target set. It is still worth keeping -- it skips the lint
   passes, which is where the time goes -- but the honest split is
   "`check` = does everything still type-check" versus "`clippy` = that
   plus pedantic lints, and the one `validate` runs". Describing one as
   "fast" and the other as "lint only" no longer maps to what either
   does. Skipping *targets* was never the right way to be quick: it
   bought a few seconds by not answering the question.

### tf-2026-08-10-frontend-content-not-separable-for-cli-only-prun -- frontend content not separable for CLI-only prune

`CLAUDE.md` documents pruning to a CLI-only project as a
supported path, and `/template-sync` is built to default the
removed paths to "skip". But the frontend assumptions in
`.claude/commands/*.md` are woven into prose rather than kept
separable, so the prune leaves instructions describing a project
that no longer exists.

Concretely, in bombyx these all had to be corrected by hand:

- `commit.md` step 6 tells the agent to run `scripts/e2e.sh`.
  With the E2E suite gone the step is unrunnable, and the agent
  needs telling to skip it on every commit until the file is
  edited.
- `implement.md` lists "frontend type-check" among the
  `cargo xtask validate` gates, and steers test-level choices
  toward Vitest and Playwright. Its manual-verification step
  says to run the backend and frontend dev servers.
- `update-deps.md` is dual-ecosystem throughout -- a frontend
  phase, the `ERESOLVE` reset, the `cd frontend` cwd trap, and
  `Bash(npm:*)` in `allowed-tools` -- for a project where
  `xtask` has no npm support at all.

None of these is wrong upstream. They are wrong the moment the
documented prune happens, and the compiler cannot find them: the
prose is what goes stale, and nothing checks prose. In bombyx
they were missed on a first sweep and caught only on a second,
after two reviewers independently flagged stale text elsewhere.

Suggested shapes, in preference order:

1. Put frontend-conditional guidance behind an explicit marker
   the prune can strip, or in a separate included section, so
   removing the frontend removes its instructions too.
2. Failing that, add the sweep to the prune checklist: after
   removing a subsystem, grep `.claude/` and `CLAUDE.md` for its
   name and check every surviving hit is deliberate. bombyx
   adopted this as a general rule after the npm removal left
   `cargo xtask --help` advertising an audit capability the
   binary no longer had.

### tf-2026-08-10-crate-readme-points-outside-the-package -- crate readme points outside the package

`crates/<name>/Cargo.toml` ships with
`readme = "../../README.md"`. That path is outside the package
root, and `cargo package` only includes files under the package
directory, so the published `.crate` carries a manifest pointing
at a file that is not in it.

What makes this worth fixing rather than documenting is that the
obvious workaround does not work. Deleting the key fails the
build: clippy's `cargo` lint group, which the template enables,
requires `package.readme` and reports "package `<name>` is
missing `package.readme` metadata". So a derived project hits a
choice between a warning-free build and a valid manifest, and
only discovers it at publish time -- the worst moment, and one
the local `validate` gate never reaches.

The other manifest metadata the template ships
(`description`, `keywords`, `categories`, `repository`)
indicates publishing is intended, so this is on the expected
path rather than an edge case.

Suggested fix: ship a short `crates/<name>/README.md` and point
the key at it, as bombyx now does. A crate-level readme is
useful anyway -- it is what crates.io renders -- and keeping it
brief (what the crate is, a usage sketch, a link to the
repository for the rest) avoids the drift that duplicating the
workspace README would cause.

### tf-2026-08-10-todo-done-writes-dangling-issue-links -- todo done writes dangling issue links

`move_to_done` in `xtask/src/todo.rs` always renders the Done
entry as `- [**slug**](issues/slug.md)`, whether or not that file
exists.

Items completed through `/implement` have a planning doc, so the
link resolves. Anything closed outside that flow gets a dead
link, and the pressure it creates is visible in practice: in
bombyx one item was hand-written into Done specifically to avoid
the broken link, and another had its issue doc written partly so
that the generated link would resolve. Both are workarounds for
the tooling rather than decisions about the work.

Two possible fixes, with a tradeoff worth stating:

- Omit the link when `docs/issues/<slug>.md` is absent. This
  needs an existence check, which `move_to_done` cannot do
  itself without becoming impure -- it is otherwise a pure
  function over the markdown, and worth keeping that way. Pass
  the answer in from the caller.
- Keep the link unconditional and have `/implement` guarantee the
  doc exists before finalising. Closer to the current intent, but
  it is enforced nowhere today, and it leaves `todo done` unsafe
  to use directly.

The first is more robust because it makes the command correct on
its own rather than correct only when driven by one particular
workflow.

### tf-2026-08-10-todo-bullet-format-mismatch -- todo bullet format mismatch

`cargo xtask todo` writes one bullet format and reads only that
format, so entries typed by hand are invisible to the tooling.

`todo add` writes `- **slug** -- summary`. `parse_slug` accepts
that and the linked Done form, but not the backticked
`` - `slug` -- summary `` that a human naturally types -- and
that the template's own `docs/todo.md` header prose uses for its
usage bullets. The consequences are quiet rather than loud:
`todo list` omits such entries with no warning, `slug_exists`
cannot see them so `add` would allow a duplicate slug, and
`todo done <slug>` fails with "no pending todo with slug" for an
item plainly present in the file. In bombyx this went unnoticed
for two days, and the short list was reported as complete
several times. A listing that silently drops rows is worse than
one that errors.

A second defect has the same root cause. `add` wraps a long
summary across lines using a two-space continuation indent, and
writes the optional `--body` with the same two-space indent.
Nothing distinguishes the second line of a summary from the
first line of a body, so reading a summary back can only take
the first line: `todo done` then writes a Done entry truncated
mid-sentence.

Suggested fix, as applied in bombyx:

- Accept all three spellings when reading, keep writing the bold
  form. Require the ` -- ` separator and validate the captured
  text as kebab-case, so inline code in prose
  (`` - `bombyx.toml` -- lives at the root ``) is not read as an
  entry -- the delimiters alone are too weak a guard, and a
  space-bearing capture would otherwise be spliced into a link
  path by `done`.
- Do not wrap the summary at all; refuse one that would not fit
  on a single line, naming the remaining budget after the slug.
  Rejoining wrapped lines on read cannot work, because the
  summary continuation and the body's first line are
  structurally identical. Removing the wrap removes the
  ambiguity at source, and turns the flag's advisory
  "<= 80 chars recommended" into a checked contract.

Both need unit tests over fixture markdown covering each bullet
spelling.

_None yet._
