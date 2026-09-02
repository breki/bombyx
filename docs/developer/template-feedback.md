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

### tf-2026-08-18-skills-json-registers-a-missing-skill -- skills.json registers a missing skill

`.claude/skills.json` still registers a `web-dev` skill that does not
exist. Its `path` is `web-dev/SKILL.md`, and `.claude/skills/` contains
only `architect/`. Its description names "Axum backend, Svelte 5
frontend, Vite configuration, and Playwright E2E testing" -- every one
of which was removed when this project was pruned to a CLI.

Two separate problems sit in that one entry. The path is dangling, so
anything that resolves registered skills against the filesystem finds
nothing. And the description is a false claim about what the project
does, which is the failure `CLAUDE.md` warns about under "After
removing a capability, re-grep for it": the compiler finds the code
that referenced a deleted subsystem, and nothing finds the prose.

It survived because the prune removed the crates, the xtask
subcommands and the E2E suite, and `skills.json` is data rather than
code, so no build step objected. It has been sitting here since the
prune and was only noticed while adding an unrelated command.

Still open in bombyx: the entry has not been removed, because it was
found during work on a different subject and deleting canon in that
commit would have mixed concerns.

Suggested for the template: either do not ship `skills.json` entries
for optional subsystems, or make something validate that every
registered `path` resolves -- a few lines in `xtask` would turn a
silent dangling reference into a failed gate. The prune instructions
should also name `skills.json` explicitly, since it is the one place a
removed subsystem leaves a description behind.

_None yet._

## Resolved

_None yet._

## Suggestions to flow back to the template

### tf-2026-08-30-xtask-invocations-in-command-files-are-not-quiet -- xtask invocations in command files are not quiet

Every xtask invocation in the template's `.claude/commands/` files is
written as bare `cargo xtask ...`, so cargo's build progress goes to
the terminal ahead of the command's own output.

Measured here on 2026-08-30. The first `cargo xtask todo list` of the
session printed 30 lines -- an index update and 28 `Compiling` lines
for the xtask dependency tree, then `Finished` and `Running` -- ahead
of the 7 lines of actual result. The answer arrives last and looks
like a footnote to a build log.

Two things make this worse for an agent than for a person reading a
terminal. The output is what the agent reasons over, so the noise is
paid for in context on every first call after any change to `xtask/`
or its dependencies, which is exactly the situation where a template
maintainer is iterating. And the obvious workaround is to pipe through
`grep -v`, which is what happened in this session before the cause was
looked at: a filter written against cargo's current wording, in a
command whose exit status the pipeline then reports as grep's rather
than the tool's. `CLAUDE.md` already warns against piping a command
whose status is the thing being checked, so the noise is quietly
pushing callers toward a pattern the same canon forbids.

`cargo -q` suppresses all of it and changes nothing else: the
subcommand's own stdout, its stderr and its exit status are untouched,
and a compile error still prints. Applied here in commit `ce87176` to
the two invocation examples in `.claude/commands/todo.md`, leaving the
surrounding prose that names the command alone.

Suggested for the template: write `cargo -q xtask ...` in the command
files that show an invocation. The same argument covers the wrapper
scripts, though those are usually read by a person watching a build
rather than by an agent parsing a result, so the case there is weaker
and the noise is arguably wanted.

### tf-2026-08-30-todo-tooling-cannot-revise-a-captured-entry -- todo tooling cannot revise a captured entry

`cargo xtask todo` offers `list`, `add` and `done`. Nothing revises a
captured entry. Verified from `cargo xtask todo --help` on 2026-08-30:
those three subcommands and `help`, and no fourth.

`.claude/commands/todo.md` closes with "Never hand-edit `docs/todo.md`;
go through `cargo xtask todo`". So the two template-provided halves
contradict each other the moment a captured item needs a correction,
and they contradict each other silently -- the rule reads as absolute
and the missing capability is only discovered by looking for it.

The case that produced this is ordinary rather than exotic. Eight items
were captured from a design discussion in `7d4b733`. One of them said
to write down that the VM host is trusted, but not the argument that
led there, which is the part that goes stale first. Adding two
paragraphs to that item's body is a revision, not a new entry, and
there was no command for it: commit `283ad16` hand-edited the file,
which is what the rules forbid. A capture tool with no revise path
pushes every second thought into either a hand edit or a duplicate
entry, and the duplicate is worse because `slug_exists` will not stop
it -- the second entry is a different slug describing the same work.

One measurement worth passing on, since it constrains the fix. The
hand-edited body is a second paragraph separated by a blank line,
which is a shape `todo add` never writes. `cargo xtask todo list`
still parsed all fifteen entries afterwards, so the reader tolerates
it. A writer would have to preserve that tolerance.

Suggested for the template: add `todo edit --slug <slug>` that
rewrites the summary, the body, or both, keeping the existing
wrap-and-render path so a revised entry is indistinguishable from a
freshly added one. Failing that, soften the rule in
`.claude/commands/todo.md` to name the one edit the tooling cannot
make, so an agent following it is not choosing between two
instructions that cannot both be obeyed.

### tf-2026-08-18-idempotent-release-can-redefine-a-version -- idempotent release can redefine a version

A follow-on to `tf-2026-08-18-template-ships-no-ci-or-release-workflow`,
which argues the template should ship a release workflow. If it does,
this is the trap to ship it without.

`gh release create` fails with "a release with the same tag name already
exists", and a re-pushed tag is a legitimate event -- rewriting history
moves every tag, and both of this project's tags were force-updated once
for exactly that reason, after which the release job failed on a build
that was otherwise green. The obvious repair is to branch on
`gh release view` and, when the release exists, `gh release edit` plus
`gh release upload --clobber`.

That makes the workflow idempotent and also makes a published version's
bytes mutable, which nothing downstream can detect. A self-updater
compares version *numbers*: this project's compares `MAJOR.MINOR.PATCH`
and has no notion of "this version's bytes changed". So whoever
installed the old assets is reported up to date forever and never
receives the replacement, and whoever is mid-download gets a checksum
mismatch whose message says the bytes are not the ones that were
released -- pointing at tampering when the cause was a re-publish.

The distinction the branch is missing is between *repairing* an
incomplete release and *redefining* a complete one, and a completion
marker separates them cheaply. Here the checksum file is uploaded with
the assets, so its presence means a previous run reached the end: a
release without `SHA256SUMS` can still be re-run, one with it fails and
asks for a new patch tag, and a repository variable
(`ALLOW_RELEASE_REPLACE=true`) overrides deliberately. Any asset the
workflow uploads last would serve as the marker.

Suggested for the template: if the release workflow is idempotent, gate
the asset replacement on a completion marker rather than clobbering
unconditionally, and say in the README that a replaced version is not
picked up by an installed copy. The guard has not itself been exercised
-- it runs only on a tag push, and no tag has been pushed since it
landed *(unverified)*. See commit `337e60a` and
`.github/workflows/release.yml`.

### tf-2026-08-18-backlog-convention-has-no-closing-half -- backlog convention has no closing half

`.claude/commands/commit.md` defines the deferred-findings backlogs
carefully on the way in: two files, newest-first, entries go after the
`---`, a `<rt|aq>-<YYYY-MM-DD>-<kebab-slug>` ID so there is no central
counter, and "a later commit that acts on or reverses a deferred item
cites its ID inline". It says nothing about what happens to the entry
itself when the item is closed. So the convention is silent at exactly
the moment someone acts on it.

Two habits both look correct under that silence and they contradict
each other. One is to annotate: this project's `doctor.rs` split was
closed by adding a `**Status:** Resolved 2026-08-10` block to the entry,
together with the measured line counts and the reason one submodule was
deliberately left whole. The other is to delete, on the reasoning that a
"deferred backlog" should hold only what is still deferred.

Working through twelve accumulated items on 2026-08-18 I deleted, and
two things broke. `docs/issues/doctor-preflight.md` still cited
`aq-2026-08-10-doctor-module-size` by ID, so the pointer resolved to
nothing -- caught by a reviewer, not by me. And the only written record
of *why* the largest submodule stays at 193 lines went with it, which is
the record that stops the next size review reopening the question. Two
of the twelve turned out not to be defects at all but standing decisions
("keep this duplication, and here is the condition that would change
that"), and a deferred-findings backlog is the wrong home for a decision
nobody intends to revisit.

The resolution used here: a fixed item leaves no entry (its resolution
is in the commit message, as the file already says); a standing decision
moves into a comment beside the code it governs, with its revisit
condition; and before any entry is deleted, `grep -rn "<id>" .` runs,
because an ID is greppable *by design* and that cuts both ways.

Suggested for the template: state the closing half of the convention in
`commit.md` next to the opening half -- what happens on fix, on
reversal, on a decision-not-to-act -- and require the ID grep before
removal. The IDs exist so other documents can point at them; a
convention that explains how to mint them and not how to retire them
guarantees a dangling pointer eventually. See commits `e1b00bd` (the
mistake) and `08413bc` (the resolution).

### tf-2026-08-18-toml-error-interpolation-echoes-the-config -- toml error interpolation echoes the config

A `toml` parse error renders the offending **source line** into its
`Display`, and the ordinary `thiserror` spelling passes that straight
through. In this project the field was `#[error("invalid config in {}:
{source}", .path.display())]` over a `source: toml::de::Error`, and the
binary printed:

```
bombyx: loading bombyx.toml: invalid config in bombyx.toml:
TOML parse error at line 1, column 12
  |
1 | -----BEGIN OPENSSH PRIVATE KEY-----
```

Reproduced against the built binary before and after the fix. The
config file here ships *inside a repo*, so its path is influenced by
whoever wrote the repo; it can be a symlink, and nobody inspects a
config after a clone. Aimed at `~/.ssh/id_ed25519`, a malformed parse
echoed a line of the key to stderr.

**Whether the template itself carries this is unverified** -- this
project's `config` module is its own, and the claim that generalises is
about the *convention* rather than about a specific file: any generated
project that reads a TOML file it does not fully control, and reports
the error by interpolating `{source}`, gets the file's contents in its
own stderr. That is the default spelling, and it is wrong by default.

The fix does not cost the diagnostic, which is what made this look like
a trade worth deferring. `toml::de::Error` exposes the two halves
separately: `message()` is the reason alone ("key with no value,
expected `=`") with no snippet and no position, and `span()` is a byte
range. Keeping both and dropping only the quoted line gives
`line 1, column 12: key with no value, expected `=``, which is
everything needed to correct a malformed config. The source string is
needed to turn the byte range into a line and column, so it is a
parameter to the summariser and nothing it returns comes from it. Count
the column in characters rather than bytes, or a non-ASCII line reports
a position past where the operator sees the problem.

Suggested for the template: if it ships a TOML config reader, summarise
the error rather than interpolating `toml::de::Error`, and say in the
comment why -- the next person will otherwise "improve" the message by
putting the snippet back. See commit `337e60a` and
`crates/bombyx/src/config.rs` (`toml_summary`, `line_column`).

### tf-2026-08-18-coverage-gate-cannot-see-src-bin -- coverage gate cannot see src/bin

`cargo xtask coverage` prints `Coverage 98.0% >= 90%` and passes, and
that number is not about all of the code. `IGNORE_REGEX` in
`xtask/src/coverage.rs` is `src[/\](main\.rs$|bin[/\])`, so the whole
of `src/bin/` is invisible to the gate. The reasoning in that constant's
doc comment is sound -- a spawned binary's coverage cannot be fully
credited to the source, and the advice "keep the testable logic in the
library crate" is right. What is missing is that nothing says so at the
point the number is reported, and nothing measures how much was skipped.

The cost, measured in this project on 2026-08-18 in one session, twice.
`bombyx`'s `src/bin/bombyx/main.rs` had accumulated the operator-facing
text for all three no-op outcomes of the self-update decision -- among
them a sentence naming two version numbers, where swapping them tells a
developer their freshly built binary is out of date. No test asserted
which version each sentence named, because no test could reach them
under the gate. They were moved into the library as
`Decision::outcome`, with tests, in commit `1e05b8d`.

In the *same commit* a new security check was written into that same
file: a digest comparison re-verifying a downloaded archive after
extraction. A reviewer pointed out that inverting its `==` would fail
nothing, and that the commit's own reasoning for moving the decision
sentences out applied to the code it had just added. It moved to the
library as well. So the blind spot is not merely theoretical: it
attracted new untested logic in the act of being described.

Suggested for the template: have `coverage` report what the ignore
regex excluded -- a file count, or a line count, beside the percentage
-- so `Coverage 98.0%` reads as "98.0% of 2,102 lines; 640 lines in 1
file excluded" rather than as a statement about the project. A number
that names its own denominator cannot be misread. Stating the caveat in
`CLAUDE.md`'s Definition of Done would help too, but the report is
where the misreading happens.

The per-file `MODULE_THRESHOLD` of 85% has the same shape and the same
answer: an excluded file has no module figure either, so a project can
carry one wholly untested file and see every gate green.

### tf-2026-08-18-agent-editing-and-measurement-rules-worth-shippi -- agent editing and measurement rules worth shipping

Two rules were added to this project's `CLAUDE.md` after each cost real
time. Both are about how an agent edits and how it states facts, so
neither is specific to this project and both would serve any project
generated from the template.

**Use `Edit` rather than a slurp-mode regex for YAML, and for anything
next to a doc comment.** `perl -0pi -e 's/.../.../'` over a whole file
has no idea which block it landed in. One substitution aimed at a
workflow's `deny` job cache block matched the `test` job's instead and
spliced steps into the wrong place; another put a statement at line 1 of
`xtask/src/audit.rs`, glued onto the module doc comment. Both needed a
`git checkout` and a redo, and the YAML one only surfaced when a parse
check failed several steps later.

Two shapes are reliably dangerous. Indentation-carrying formats, where a
wrong-block match still parses and so fails silently. And anything
adjacent to a `///` block, where inserting before an item reassigns the
comment above it to the new item -- which is how a function ended up
documented as doing the opposite of what it does, with both rustdoc
passes clean because the comment was syntactically valid on the item it
landed on. `sed` and `perl` remain fine for flat text and one-line
substitutions.

**Print the variable before claiming what it holds.** Three false
statements in one week came from writing an environment claim from
expectation rather than measurement: that a libvirt guest's DMI exposes
the host (it exposes the emulated machine -- `sys_vendor` reads `QEMU`),
that a repository was public (it was private, so every asset URL 404'd
and a whole design was built on the wrong premise), and that Windows sets
`USERPROFILE` "and not HOME" (Git Bash sets both, with `HOME` in POSIX
form). Each was one command away: `cat /sys/class/dmi/id/sys_vendor`,
`gh repo view --json visibility`, `echo $HOME`. The rule is that a claim
about what a variable, a file or a platform actually contains has to
arrive with the command that read it.

The second rule matters more than it looks, because the failure mode is
a *plausible* wrong answer rather than an error. A claim that a platform
does not expose something is the easiest kind to invent, and nothing
fails when it is wrong -- it just gets written into three files and
believed.

Suggested for the template: carry both in the `CLAUDE.md` scaffolding.
They are cheap to state and each one here was learned by losing an hour.

### tf-2026-08-18-nothing-checks-a-second-platform-before-release -- nothing checks a second platform before release

Nothing in the template checks a second platform before a release. Both
`validate` and `/release` run on the machine the developer is sitting at,
and the template claims Windows, Linux and macOS as first-class targets.
So a release can be tagged, pushed and immediately fail CI on the two
platforms nobody checked.

That is not a worry, it is the record. `v0.3.0` was cut here after a
green nine-gate `validate`, tagged, pushed -- and the release run failed
on ubuntu and macos while windows passed. Recovering it meant fixing the
defect, moving an already-pushed tag and paying a second eight-minute
release run. Twice before that, template-provided `xtask` code neither
compiled nor linted off Windows for the same reason, which is recorded
separately as `tf-2026-08-18-xtask-clean-cache-breaks-off-windows`.

That entry suggests noting the cross-target commands wherever the
platform claim is made. This one is the stronger form of the same
lesson: make one of them a gate, because a note is something a release
can skip and a step is not.

The fix here was a step in `/release`, before the tag:

    cargo clippy --workspace --all-targets \
      --target x86_64-unknown-linux-gnu -- -D warnings

Two details that took a failure each to learn. It has to be **clippy**,
not `cargo check`: `check --target` compiles and runs no lints, so it
proves the build and says nothing about the lints -- which is exactly how
the second `xtask` failure slipped through a cross-target check that had
just been run. And no linker for the other platform is needed, because
only the cfg analysis and the lints run, so this costs seconds on a
warm target directory and needs no toolchain beyond a `rustup target
add`.

Suggested for the template: add the cross-target clippy to `/release`
before the tag, and consider it in `validate` for projects where the
extra seconds are acceptable. A template that claims three platforms and
gates one is asserting something nothing verifies.

### tf-2026-08-18-template-improve-only-logs-what-it-is-told -- template-improve only logs what it is told

`/template-improve` only records what the operator happens to remember.
It asks "what did you notice?", logs that one thing, and stops. Nothing
looks at the commits, so an observation made during work and not
mentioned at the moment the command runs is simply lost.

The gap showed itself the first time the command was used seriously
here. Run at the end of a long session it produced four entries, all
drawn from the last hour of context. A sweep of the twenty-one commits
since the file was last touched then found four more, every one of them
a template-provided file and every one already written up in a commit
body: `xtask` not compiling off Windows, a `/todo` prompt stating a
budget its tool does not use, the absent CI that let the first two ship,
and this entry. The observations were not missing. They were unasked
for.

The fix here was a sweep mode. With no argument the command now
establishes a boundary from `git log -1 -- docs/developer/template-
feedback.md`, walks the commits since, and reads four places rather than
one: commit bodies, the diary, the two review backlogs, and the diffs
where a message is thin. That last set matters -- a deferred finding
against template-provided code *is* template feedback and is already
written up, so the sweep is mostly harvesting rather than analysing.

Two details that took a correction to get right. Judgement has to be
explicit or a sweep fills the file with project-specific noise, so the
command now asks three questions of each candidate: does it live in a
file the template provides, would another derived project hit it, and
was it a surprise. And `feedback-add` dedups by ID, which catches only
an identical title, so the sweep greps the file *and* both backlogs for
the subject before offering anything.

One more, found by the operator rather than by the sweep: a boundary
taken from `git log` cannot see uncommitted work, so a change made in
the same session -- including the one adding the sweep -- is invisible to
it. The command now reads the working tree as well.

Suggested for the template: give `/template-improve` a sweep mode with
the boundary, the four sources, the three judgement questions and the
working-tree check. A command that depends on the operator volunteering
what they noticed captures the loudest observation, not the most useful
one.

### tf-2026-08-18-template-ships-no-ci-or-release-workflow -- template ships no CI or release workflow

The template ships no CI and no release workflow. This project had no
`.github` directory at all until one was written here, so a derived
project's quality gates run only where a developer happens to run them.

The cost is not hypothetical, and it landed immediately. The very first
CI run failed on ubuntu and macos while passing on windows, on two
separate defects in template-provided `xtask` code -- see
`tf-2026-08-18-xtask-clean-cache-breaks-off-windows`. Both had been
present through every green local `validate`, because `validate` checks
one platform: the one the developer is sitting at. A template that
claims Windows, Linux and macOS as first-class targets and provides no
multi-platform check is asserting something nothing verifies.

Two design points from building it here are worth carrying upstream
rather than re-deriving.

Which gates go where. `coverage` and `dupes` each need a tool the runner
does not ship, so paying for them on every push buys little; they belong
on the release path, where shipping a regression matters. The advisory
`audit` is the opposite case in a subtler way: it reaches the network for
state that changes on its own, so on every push it fails pull requests
that changed nothing, while at release time it is exactly what must not
be skipped. A licence check, being offline, can sit on every push.

Release-notes extraction. The obvious implementation accepts
`[Unreleased]` as a fallback when it cannot find the version's CHANGELOG
section, and that is a trap rather than a convenience: `[Unreleased]` is
the first heading in the file, so it always matches before the version
section is reached, and `/release` has just emptied it. Matching the
version heading by exact prefix and *failing* when it is absent is what
keeps a silent mistake loud.

Suggested for the template: ship both workflows, with the gate placement
above and the exact-match notes extraction, so a derived project starts
with a check that covers the platforms it claims.

### tf-2026-08-18-todo-command-states-wrong-summary-budget -- todo command states wrong summary budget

`.claude/commands/todo.md` told the agent a summary must be "<= 80
chars". `cargo xtask todo add` measures something else: the whole
rendered line, `- **<slug>** -- <summary>`, against 80 columns. The
prefix costs 10 characters plus the slug length, so the real budget is
70 minus the slug.

The two are template-provided and disagree, which makes the failure
confusing rather than merely inconvenient. A 60-character summary behind
a 23-character slug is well inside the documented limit and was rejected
for being 96 columns wide, costing a repeated call and a guess at what
the tool actually meant.

The general shape is worth naming: a prompt file stating a numeric limit
that the tool it drives computes differently. The number in the prose
has no way to notice when the tool's formatting changes, so it is a
claim that rots silently.

Fixed here in `2d1f6fe` by replacing the flat number with the formula,
noting that a 50-character slug leaves only 20 characters, and saying to
recover by shortening the summary rather than the slug -- the slug is the
entry's identity and appears in the Done link.

Suggested for the template: state the budget as the formula rather than a
number, or have `todo add` report the arithmetic when it rejects a line
so the message is self-explanatory.

### tf-2026-08-18-xtask-clean-cache-breaks-off-windows -- xtask clean_cache breaks off Windows

`xtask/src/clean_cache.rs` neither compiled nor linted on Linux or
macOS. Two separate failures, both from the same shape, both found by
the first CI run this project ever had and neither by any local gate.

First, the build. `is_reparse_or_symlink_meta` is imported at the top of
the file but called only inside the `#[cfg(windows)]` branch of
`is_reparse_or_symlink_path`. Everywhere else the import is unused, and
`[workspace.lints.rust] warnings = "deny"` turns an unused import into a
compile error. So `cargo build` fails outright on two of the three
platforms the template claims.

Second, clippy. With the build fixed, `unnecessary_wraps` objected that
off Windows the function never returns `Err`, so its `Result` is
gratuitous. The lint is right about the non-Windows build and wrong
about the function: the `Result` is load-bearing on Windows, where
`symlink_metadata` can fail, and one signature has to serve both
platforms because the caller propagates the error. A `cfg_attr`-scoped
allow says exactly that and leaves the lint active on Windows.

The reason both shipped is worth more than either fix. Every gate in
`validate` passes on a Windows workstation, and nothing in the template
cross-checks another platform. The two commands that reproduce CI
locally, without a runner, are:

    cargo check --workspace --all-targets --target x86_64-unknown-linux-gnu
    cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu -- -D warnings

And the distinction between them is the trap: `cargo check --target`
compiles and runs *no lints*, so it proved the build and said nothing
about clippy -- which is precisely how the second failure slipped
through a cross-target check that had just been run.

Fixed here in `e309e9c` and `c0596fd`. The template presumably still
carries both, since the file arrived with the cross-cfg import in place.

Suggested for the template: gate the import to match its use site, add
the `cfg_attr` allow, and note the two cross-target commands wherever
the platform claim is made -- a project developed on Windows has no
other way to find this before CI does.

### tf-2026-08-18-validate-step-numbers-are-literals -- validate step numbers are literals

`xtask/src/validate.rs` numbers its steps with literals. A
`TOTAL_STEPS` constant holds the count, each `run_step` call passes its
own index, and nothing checks that the two agree:

    const TOTAL_STEPS: usize = 8;
    ...
    run_step(3, "Duplication", "dupes", run_duplication)?;
    run_step(4, "Clippy", "clippy", run_clippy)?;

Adding one gate therefore means editing the constant and renumbering
every call after the insertion point. bombyx inserted a licence gate as
step 4 and had to touch six call sites for one new check. Miss one and
`validate` prints `[5/9]` twice, or `[9/10]` last, and no test fails --
the numbering is derived data being maintained by hand, and the only
thing that notices a mismatch is a human reading the output.

Suggested for the template: build a table of steps and enumerate it, so
the index and the total both come from the same list.

    let steps = vec![
        ("Dep-age", "dep-age-check", boxed(run_dep_age)),
        ("Fmt", "fmt", boxed(move || run_fmt(check))),
        // ...
    ];
    let total = steps.len();
    for (i, (name, cmd, f)) in steps.into_iter().enumerate() {
        run_step(i + 1, total, name, cmd, f)?;
    }

`run_step` takes `total` as a parameter, `TOTAL_STEPS` disappears, and
inserting a gate becomes one line in one place. The per-step comments
that currently sit above each call read just as well inside the table.

Fixed in bombyx on 2026-08-18, close to the shape above: a
`struct Step { name, cmd, run: Box<dyn FnOnce() -> ... > }` and a
`steps(check) -> Vec<Step>` builder, with `validate` enumerating it.
Splitting the table out of `validate` also made it testable, which
turned out to matter for a second reason: the `cmd` strings are the
`iterate with: cargo xtask <cmd>` hints, and nothing tied them to the
clap subcommands, so a rename would have left the gate advising a
command that does not exist. A test now resolves each one through
`Cli::command().find_subcommand`. The template presumably still carries
the literals.

### tf-2026-08-18-no-licence-gate-or-attribution-tooling -- no licence gate or attribution tooling

The template guards the dependency tree against vulnerabilities and
against freshness -- `audit`, `dep-age`, `dep-age-check`,
`dep-preflight` -- and against licences not at all. There is no
`deny.toml`, nothing checks licence compatibility, and nothing
generates third-party attribution.

Both halves turn out to be needed by any project that publishes
binaries, and the second is an obligation rather than a nicety. MIT and
Apache-2.0 both require the licence and copyright notice to travel with
a distributed binary, and a crate carrying an SPDX `AND` (for example
`unicode-ident`, which is `(MIT OR Apache-2.0) AND Unicode-3.0`) makes
those extra terms required rather than optional. A release archive
holding only the project's own `LICENSE` does not meet that, and the
gap opens with the first published binary rather than at some later
point of scale.

bombyx added two `cargo xtask` commands. `deny` runs cargo-deny over
licences, bans and sources against a `deny.toml` whose allow-list is
exactly the licences in the tree; it is offline, so it runs on every
push in CI as well as in `validate`, where the advisory audit cannot go.
`licenses` generates a `THIRD-PARTY-LICENSES` file from `cargo
metadata` plus the licence texts already unpacked in the cargo
registry, and the release workflow writes it into every archive.

Three details are worth carrying upstream rather than
rediscovering.
`private = { ignore = true }` in `deny.toml` reads as "skip our own
unpublished crates" and does not: cargo-deny skips *any* package with
`publish = false`, wherever it lives, so a GPL-only path dependency
passes the gate -- verified. `cargo deny` needs an explicit `--offline`
or it resolves and fetches the whole tree, which defeats the reason for
running it on every push. And `COPYRIGHT` and `AUTHORS` have to count as
notice files: `rustix` and `linux-raw-sys` explain their triple licence
and the LLVM exception there rather than in `LICENSE-MIT`, and omitting
them is invisible because those crates ship a `LICENSE-*` as well.

Suggested for the template: ship `deny.toml` plus the two commands, wire
`deny` into `validate` and every-push CI, and write the attribution file
into whatever the project distributes.

### tf-2026-08-18-validate-audit-can-pass-without-auditing -- validate audit can pass without auditing

`cargo xtask validate` runs the security audit as one of its steps, and
inside that step a missing `cargo-audit` or an unreachable RUSTSEC
database degrades to a printed warning rather than failing. The
leniency is deliberate and defensible on its own -- an offline machine
should still be able to finish `validate` -- but the consequence is not
stated anywhere the template documents the gate.

The consequence is that **`Validate OK` does not mean the dependencies
were audited.** On a machine where `cargo-audit` was never installed,
`validate` prints a warning nobody reads and then reports success, and
every derived project inherits a gate that is silently not one. The
standalone `cargo xtask audit` errors on both conditions, so the two
spellings of the same check disagree about whether it is a gate.

It matters most at release time, which is exactly when the answer needs
to be trustworthy: an advisory is almost always filed against code that
has already shipped, so no check tied to *changed* dependencies can
catch it, and `dep-age-check` cannot by design.

bombyx closed it two ways. `/release` now runs the standalone
`cargo xtask audit` as its own step after `validate`, so the tag cannot
be created without the advisory database having been consulted, and the
release CI job runs the same command where nobody can skip it. The
caveat is also written into `CLAUDE.md` in as many words, because the
surprising part is not the leniency but that "Validate OK" overstates.

Suggested for the template: state the degrade-to-warning behaviour
wherever `validate`'s audit step is documented, and give `/release` a
standalone audit step rather than relying on `validate`'s copy.

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
