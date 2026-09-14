# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code)
when working with code in this repository.

## Writing

Write like a person explaining something to a colleague. Plain
words, short sentences, one idea at a time.

Six habits make prose read on the first pass:

- Name the subject -- a file, a function, a command, a value, a
  person. Not "there", "it", or "nothing".
- Use a concrete verb: reads, clones, refuses, removes. Not "is
  the point", and not a noun doing the verb's work.
- Define a term where it first appears, before leaning on it.
- Explain the mechanism before the conclusion.
- State the relationship with when, if, so, because.
- Keep the core sentence short. Read it aloud; if you run out of
  breath or backtrack to parse it, split it.

In prose about the work the actor is "we"; in a code comment it
is the program. Say plainly what you have not verified. Give the
reason the code is the way it is, not its history -- bombyx is
pre-release, so "this used to" and "an earlier version" are
defects, not context.

Begin every chat reply with a summary under forty words,
wrapped in `[short]` and `[/short]` tags on their own lines,
before the body. A speech synthesizer reads it, so use whole
words and short sentences, and put no backticks, paths, or
symbols inside it.

## Working directory

**IMPORTANT: The working directory is already set to the
project root. NEVER use `cd` to the project root or
`git -C <dir>` -- blanket permission rules cannot be
set for commands starting with `cd` or `git -C`, so
they require manual approval every time.**

## Project Overview

bombyx drives isolated AI-agent VMs on a libvirt host,
usually a second machine reached over SSH. It is the tooling
half of the agent-VM isolation strategy: bombyx generates the
Vagrantfile and a bootstrap script from the operator's own
`config.toml`, writes them onto the VM host and runs `vagrant`
there. When `host` names the machine bombyx is running on, it
runs the same script through `sh -c` instead of `ssh`. Neither the
workstation nor the VM host reads a file from the project's
repository; the guest clones the project itself.

- **Stack**: Rust CLI, no runtime services
- **Target platforms**: Windows (dev workstation), Linux
- **Control plane**: a wrapper around `vagrant` on the VM
  host, over `ssh` or through `sh -c` when the VM host is this
  machine. Deliberately thin -- see `README.md`.

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
cargo xtask check             # type-check all targets, run none
cargo xtask validate          # every gate, in run order
cargo xtask test [filter]     # tests only
cargo xtask test --ignored    # run #[ignore]-tagged tests
cargo xtask clippy            # lint only
cargo xtask doc               # doc build + doc-link check
cargo xtask coverage          # coverage only (>=90%)
cargo xtask fmt               # format code
cargo xtask canon-check       # canon prose claims vs the tree
cargo xtask dupes             # code duplication check
cargo xtask audit             # security-advisory audit (RUSTSEC)
cargo xtask deny              # licence/bans/sources gate (cargo-deny, offline)
cargo xtask licenses [--out P] [--target T] [--max-missing N]
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

**When a `validate` step fails, re-run that step, not the
pipeline.** It prints the command for you
(`-> iterate with: cargo xtask clippy`), and that hint exists
because the failing gate is usually seconds while the whole
pipeline pays for coverage and the network audit every round.
Ignoring it four times in one sitting is what prompted writing
this down. Run `validate` once at the end to confirm.

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
  command that actually talks to it (`ssh`, `vagrant`) may
  fail for reasons unrelated to the change under test.
  Prefer `--dry-run` for argv-level checks, and say so
  explicitly when a claim rests on a dry run rather than a
  real run against the VM host.
- **Scripting**: use PowerShell, Bash, or Rust (`xtask`).
  Keep non-trivial logic in `xtask` -- see "Shell wrappers".
- **Do not grep canon prose for a phrase.** Every markdown
  file here wraps at 80 columns, so a phrase you remember as
  one line is usually two, and `grep` sees neither. A search
  for `keeps its ID` returned nothing while the text said
  "keeps its\nID", and a false claim was reported as fixed on
  the strength of that empty result. The same wrap broke a
  check inside `cargo xtask canon-check`, which now matches
  against the text with every whitespace run collapsed. Search
  for one distinctive word, or flatten first:
  `tr '\n' ' ' < FILE | grep -o 'the phrase'`.
- **Read a large file in pieces.** Over roughly 500 lines,
  `grep -n` for the item you want and then `sed -n` the range
  around it. Reading `crates/bombyx/src/config.rs` whole
  produced 66KB that overflowed into a persisted file, and
  every fact the session actually used came from the greps that
  followed it. No line count here on purpose: that file grows
  every commit, and `config-tests-own-file` in `docs/todo.md`
  records why a figure in prose costs the next reader a check.

  **Do not then read the same range again with `Read`.** Four
  ranges of `config.rs` were read twice in one sitting, on a
  belief that `Edit` refuses a file the session has read only
  through Bash. Two probes say it does not. An `Edit` against a
  file never read at all reported a missing match rather than a
  missing read, and an `Edit` against `docs/todo.md`, opened
  that session with `grep` and `sed` and never with `Read`,
  applied cleanly and was reverted with `git checkout`. So
  `sed -n` alone is enough before an edit, whatever the tool
  description gives as the precondition.
- **Edit YAML and doc-comment neighbourhoods with `Edit`, not a
  slurp-mode regex.** `perl -0pi -e 's/.../.../'` over a whole
  file has no idea which block it landed in. One substitution
  aimed at the `deny` job's cache block matched the `test` job's
  instead and spliced steps into it; another put a statement at
  line 1 of `audit.rs`, glued onto the module doc. Both needed
  `git checkout` and a redo. Two shapes are reliably dangerous:
  indentation-carrying formats, where a wrong-block match still
  parses, and anything next to a `///` block, where inserting
  before an item silently reassigns the comment above it to the
  new one. `sed`/`perl` are fine for flat text and one-line
  substitutions.

  **A scripted string replace over Rust is the same hazard**,
  whatever language does the replacing. Six edits needed a
  repair round in one sitting: a `\bplaces\b` rename caught
  the word in a sentence, a replacement left a duplicated
  clause behind, a `map_err` closure came out nested wrongly,
  and a signature change missed one call site. Every one of
  them sat next to a `///` block. Reach for `Edit` with an
  anchor unique in the file, and keep a scripted replace for a
  substitution that fits on one line.

  **A script that batches several replacements and writes once
  at the end reports success for edits it never made.** Three
  did so in one sitting. Each asserted its matches and wrote
  the file after the last one, so a failed assertion raised and
  the write never ran -- while the shell's next `echo ok`
  printed anyway, because the failure was the script's exit
  status and nothing read it. One such edit was reported
  applied, and a reviewer found the unchanged text two stages
  later. Write the file after **each** successful replacement,
  and read the result back: a fix is landed when `grep` or
  `sed -n` shows it, never when the script that made it says
  so.
- **Print the variable before claiming what it holds.** Three
  false statements this week came from writing an environment
  claim from expectation: that the guest's DMI exposes the host
  (it exposes the emulated machine), that the repository was
  public (it was private), and that Windows sets `USERPROFILE`
  "and not HOME" (Git Bash sets both, with `HOME` in POSIX
  form). Each was one command away -- `cat /sys/class/dmi/id/...`,
  `gh repo view --json visibility`, `echo $HOME`. A claim about
  what a variable, a file or a platform actually contains needs
  the command that read it, in the same breath.

  **Which stream carries a message is the same kind of claim.**
  A doc comment said the `vagrant-libvirt` fog warning arrives
  in the reply bombyx parses; it is on stderr, so it never gets
  there. Two commands settled it -- `cmd 2>/dev/null` and
  `cmd 2>&1 >/dev/null` -- and the second is the one people
  forget, because the redirections have to be in that order to
  keep stdout out of the way. A count is the same kind of claim
  too: "`Registry` had two functions" was five, and
  `grep -n "fn "` said so.
- **Test an SSH identity with `-F /dev/null`.**
  `IdentitiesOnly=yes` does not exclude identities named
  in `ssh_config`, so `ssh -i key -o IdentitiesOnly=yes`
  on a host with a `Host github.com / IdentityFile ...`
  entry authenticates with *that* key and reports
  success for a key the far side has never seen. Ignoring
  the config is what makes the answer honest.
- **Single-quote any `$` you pass through PowerShell.**
  A double-quoted string is expanded before the argument
  reaches the program, and the two cases fail differently.
  A variable PowerShell defines, such as the automatic
  `$HOME`, becomes a path -- so a real home directory
  lands in the file. Anything else becomes the empty
  string *silently*, which is the worse half: it leaves
  plausible-looking text behind. Note that an environment
  variable is *not* a bare `$NAME` in PowerShell -- it is
  `$env:NAME` -- so `$XDG_CONFIG_HOME` is simply an
  undefined variable and expands to nothing at all. A
  `cargo xtask changelog add` call describing
  `$XDG_CONFIG_HOME/bombyx` wrote a bare `/bombyx` into
  `CHANGELOG.md`, next to an expanded home path from the
  same line. Backslash does not escape `$` in PowerShell;
  backtick does. Use a single-quoted string, or a Bash
  heredoc, whenever the text contains `$`.
- **The same mistake has three other shapes. Check all
  four when a value crosses a shell boundary.** The rule
  above protects a *primitive* -- who expands the text --
  not the `$` character, and each sibling produced a false
  statement before it was noticed:
  - **`$(...)` inside a nested remote command runs on the
    near side.** `ssh host "vagrant ssh -c \"uname -srm\""`
    is fine, but `$(uname -srm)` written inside it is
    expanded by the *host* shell, so a guest check happily
    reports the host's kernel and hostname. Escape it
    (`\$(...)`), and assert one value that must differ
    between the two, so a wrong-side expansion is visible
    rather than plausible.
  - **An empty argument to a native `.exe` is not empty.**
    `ssh-keygen -N '""'` in PowerShell passes two literal
    quote characters as the passphrase, producing an
    encrypted key that then prompts and hangs anything
    unattended. Generate keys from Bash, and verify with
    `ssh-keygen -y -P '' -f <key>` before relying on one.
  - **`pgrep -f <pattern>` matches its own invocation.**
    The wrapper command contains the pattern, so a count
    is inflated and a dead process looks alive. Count with
    `ps -eo comm | grep -c '^name'` instead.
- **A Windows command needing elevation blocks on a dialog
  you cannot see.** `wsl --update` produced no output for
  ten minutes and read as a hang; a UAC prompt was waiting
  off-screen the whole time. Run anything that may elevate
  (`msiexec`, `wsl --update`, `Start-Process -Verb RunAs`)
  with `run_in_background`, and when a command stalls with
  an empty log, check `Get-Process consent` before
  diagnosing anything else.

## Collaboration

**Writing** is at the top of this file and applies to
everything here as well.

- **Write plainly.** One idea per sentence; lead with the
  concrete example, then the rule; prefer plain words
  ("reminder" over "forcing function"). **Writing** covers the
  rest.
- **Narrate the work as it happens.** Before each meaningful
  tool call or step, say in one short sentence what is about
  to happen and why. Do not batch silently and only speak at
  the end -- a run of silent tool calls reads as "lost".
  This holds regardless of the active output style.
- **Do not poll for a subagent. Wait for the notification.**
  The harness reports an `Agent` call's completion on its own.
  One `/review` run spent about fifteen `sleep 115` calls
  waiting on three reviewers that took five to eleven minutes
  each, and one of those sleeps hit its own timeout and was
  backgrounded, which produced a second notification to read.
  Say in one sentence that the reviewer is running, then stop.
  Start other work only when it touches no file the reviewer
  is reading.
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
- **Prefer strong types. Avoid primitive obsession.** A
  value with a rule attached gets a type that enforces the
  rule, not a `String` with a checking function somewhere
  else. The pattern is a newtype: a struct wrapping one
  private field, buildable only through a constructor that
  checks first, so holding one *is* the proof it passed and
  the compiler carries that proof to every use site. See
  `config::source::RepoUrl` for the shape.

  A checking function is weaker in a way that is easy to
  miss. It proves the value was checked on the paths that
  call it, and nothing about the paths that do not.
  `Config` has public fields, so any code can build one by
  hand and skip every check; a type cannot be skipped that
  way.

  **"The rules are generic" is not a reason to leave a
  value primitive.** What a type promises is not that its
  rules are interesting, it is that they *ran*. A rule as
  dull as non-blank and no-leading-dash still earns a type,
  because the alternative is remembering to call the
  checker.

  Wire it up with `#[serde(try_from = "String")]` so a bad
  value is refused while the config is being read, before
  the struct exists, and the error names the offending
  line. Without that attribute serde assigns the private
  field directly and skips the constructor.

  Three cases justify a primitive: the value has no rule at
  all; the type would be built and unwrapped in the same
  breath with nothing in between; a standard type already
  carries the meaning. Say which one applies, in a comment.
  **The representation has to be argued for.**
  `ScriptPath` is a checked `String` rather than a
  `PathBuf` for a written-down reason -- the path is
  resolved on the guest, and `PathBuf` answers for the
  machine bombyx was compiled for.
- All public items must have doc comments
- Wrap markdown at 80 characters per line
- **Prose written to GitHub is not wrapped.** An issue body, an
  issue comment, a PR title and a PR description all get one
  long line per paragraph. The rule above covers a markdown
  file in the repository, where the file is what a reader
  opens; GitHub renders the markdown instead, so a hard wrap
  there buys the reader nothing and makes the text harder to
  edit afterwards. Wrap in `docs/`, `README.md`, this file,
  `llms.txt`, `.claude/` and code comments. Do not wrap in
  anything handed to `gh`.
- **Fixing an over-long line means reflowing its whole
  paragraph.** Patching the one line pushes the overflow onto
  the next and leaves half-empty lines mid-paragraph, which a
  reader takes for a paragraph break. Three edits in a row
  went that way in one sitting, and the reviewer then filed
  the ragged result as a finding.
- Maximum code line width: 80 characters (`rustfmt.toml`)
- **Validate a field's invariants where the field
  lives.** Put the rule in the module that owns the
  value -- `RemoteRoot`'s constructor for `remote_root` --
  not at each use site. A check bolted onto one
  call site leaves every other path disagreeing with
  it: a depth floor placed on the removal path once
  left the same `remote_root` illegal to delete but
  legal to write, so `up` would happily `mkdir -p
  /etc`. Validating once also keeps the error next to
  the field name and avoids threading a `Result`
  through callers that have nothing to decide.
- **Guarding one field? Check its siblings.** A rule
  protects a *primitive*, not a field name, so every
  value that reaches the same primitive needs it.
  `remote_root` reaches `rm -rf` and got a careful depth
  and traversal guard; `vagrant_dir` reached `tar -C`
  and got none, so an absolute value made `bombyx up`
  archive `~/.ssh` and ship it to the host named in the
  same file. The dangerous-*looking* field had the
  attention, and the one beside it did not. (The push
  is gone and `vagrant_dir` with it, so do not go
  looking for the field. The rule is what survives.)
- **After fixing a bug, grep the file for the same
  shape.** A bug class rarely appears once. A guard
  calling `swapon` without `sudo` -- invisible on the
  non-interactive `PATH` -- was fixed, explained in a
  comment, and then repeated twenty lines later with
  `ldconfig`, costing a whole verification cycle. The
  fix is mechanical: before re-running anything, search
  for the other instances of the pattern you just
  corrected.
- **After removing a capability, re-grep for it.** The
  compiler finds the code that referenced it; nothing finds
  the *prose* that described it -- clap `///` help, module
  docs, `CLAUDE.md`, `.claude/commands/`. Before handing a
  removal to review, run `grep -rni "<term>" .` and check
  every surviving hit is deliberate. Stale help text is a
  false claim about what the tool does, and stale wording
  around a deleted branch is what makes the next reader
  believe a bug is intentional.

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

**Ask before testing something that is not the
program.** The rules above say how to write a test
once we have decided it should exist. They do not
decide that. When the *subject under test* is a
repository document, a rendered transcript, a file
layout or a build artifact -- rather than a function
bombyx runs -- call `AskUserQuestion` before writing
it. Put the choice in the lead prose and the file
names in the option descriptions, which is what
**Collaboration** asks of every question.

**This does not touch ordinary tests.** A function in
`crates/bombyx/src`, a helper in `xtask`, a new
`Action` variant: those follow the red/green rules
above and Definition of Done item 1, with no question
asked. Developer tooling is not the trigger; the
subject is.

We wrote three such tests and deleted them four review
rounds later. They ran bombyx, split its output on
string literals, split a markdown file the same way,
and compared. One checked that the config samples in
the documents load. One checked the `(N lines elided)`
counts in the dry-run transcripts. One checked that a
`doctor` transcript showing skip rows also shows the
skip count. Nobody asked for any of them. Each round
the reviewers found real defects in them, each fix was
right, and it never converged, because rendered
terminal output and hand-written prose offer no
contract to assert against. The three came to 251
lines, a quarter of the integration suite, and caught
two defects.

The tell is on the assertion side. **A test whose
assertions need their own parser is testing the
parser.** bombyx contains the parser for a config
file, so "does this sample load" had a contract behind
it. No parser exists for a rendered `doctor` report,
so "does this transcript look right" could not have
one. That is why the sample-config check survives, as
one `include_str!` of `config.toml.sample` with no
document scanning in it, and the other two do not.

Auxiliary code is where this costs the most, because
nobody is waiting for the test and nobody notices what
it costs to keep. Ask.

**Input guards: enumerate the family first.** When
adding a check that rejects bad input, write the test
table before the check, listing the whole family the
guard claims to cover -- for a path that means `.`,
`..`, empty, unrooted, too shallow, doubled and
trailing slash. Fixing only the case that prompted
the work and then describing the guard in general
terms is how a guard comes to claim more than it
does: a `remote_root` check once rejected `..` but
not `.`, and the doc comment asserted it stopped a
hostile root reaching a top-level directory. `/.`
defeated it in five characters.

## Commits and releases

**All commits must go through the `/commit` skill.**
Never use `git commit` directly. No "Co-Authored-By",
no emoji. (The sole exception is `/release`, which makes
one direct bookkeeping commit for the version bump.)

Committing and releasing are separate:

- **`/commit`** is a save-point. It updates the diary and the
  `CHANGELOG.md` `[Unreleased]` block and commits. It does
  **no reviewing** -- see **Reviewing is its own process**
  below. It does **not** bump the version, touch `Cargo.lock`,
  or run `cargo xtask validate` -- multiple commits land
  between releases, and forcing each one to make a SemVer
  decision turns the version field into accounting rather than
  a description of what users run. Run `cargo xtask validate`
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
  (`git tag -a vX.Y.Z`; annotated rather than lightweight,
  so the tag carries a date and author and is what
  `git describe` finds).

There is no deploy step. bombyx is a CLI installed with
`cargo install`, so a release is the tag -- nothing is
pushed to a server afterwards. The template this project
came from had `cargo xtask deploy` gating exactly that,
and the prose describing it outlived the subsystem by
several weeks.

### Reviewing is its own process

`/review` reviews, `/commit` commits, and neither calls the
other. Nothing requires a review: reach for `/review` when you
want work hardened before it becomes a commit, and skip it when
you do not.

That split is deliberate. We tried running the reviews inside
`/commit`, and it cost us this: every `/commit` became a
multi-round session -- fixes needing their own commits, the
reviewers firing again on each, and no way to commit a
save-point without inviting all of it. A save-point should be
cheap. Commit `6055f93` is where that arrangement landed.

The earlier arrangement got one thing right, and `/review`
keeps it: **the reviewers get an immutable target.** Reviewing
a live working tree means reviewing something that changes
while they read it, and this repo has already had a reviewer
report against a tree that no longer compiled, because fixes
for its own earlier findings had landed underneath it. That is
the reason; `/review` under **Snapshot** holds how it does it.

**Stop when we would not fix anything the round found** -- every
finding deferred or declined. Do not keep going for a clean
sheet: reviewers always find something, and the stopping rule is
agreement on what matters. Do not read a falling finding count
as progress either: a round's fixes make the next round's
findings, so the count flattens out well above zero. `/review`
under **What earns another round** lists the conditions
themselves, the three-round cap among them, and holds the run
that showed it.

## Definition of Done

A task is done only when all of the following hold -- not
just when the code compiles:

1. **Targeted tests** for the change are written and pass.
2. **Type-check** passes (`cargo xtask check`).
3. **Verify against a real VM host** for anything that
   changes the commands bombyx emits. `--dry-run` proves
   the argv; it does not prove the remote side accepts it.
   "tests pass" is not "the command works".
4. **Self-review the diff** before committing. This is your
   own read. `/review` is available and is not required; it
   does not replace reading your own diff either way.
5. **`cargo xtask validate`** passes (the umbrella gate).

`cargo xtask validate` runs ten gates, **listed here in the
order they execute** so the numbers match what the run prints:

1. **Dependency cooldown** (`cargo xtask dep-age-check`) --
   fails when a dependency added or bumped since `HEAD` was
   published within the 14-day window; an unchanged
   lockfile makes it a no-op
2. **Formatting**: auto-fixed in place by default; pass
   `cargo xtask validate --check` for the read-only
   `cargo fmt --all -- --check` (use in CI or before
   partial staging, so an in-place rewrite does not sweep
   unrelated drift into the working tree)
3. **Canon claims** (`cargo xtask canon-check`) -- reads four
   inputs, and only these four: `CLAUDE.md`, `llms.txt`,
   `.claude/commands/` and `.claude/agents/`. So
   `.claude/skills/` and everything under `docs/` are
   unchecked, and a green gate says nothing about them --
   `backlog-ids-dangle-in-docs` in `docs/todo.md` holds that.
   It fails on five kinds of claim the tree does not support: a
   bold cross-reference introduced by the word "under" that
   names no heading anywhere in canon, a backticked repo path
   that does not exist, a command file telling the agent to
   run a `git` subcommand its own `allowed-tools` does not
   grant, prose past 80 columns, and a cited backlog ID that
   is in no backlog. It reads markdown only, so it needs no
   compilation and runs before every gate that does
4. **Code duplication <= 6%** (production code, tests
   excluded)
5. **Licences, bans and sources** (`cargo xtask deny`) --
   runs offline against `deny.toml`; a licence outside the
   allow-list, a banned crate or a non-crates.io source fails,
   and a missing `cargo-deny` is an error rather than a warning
   because there is no network here to be down
6. **No warnings**:
   `cargo clippy --all-targets -- -D warnings`
7. **Documentation builds and every doc link resolves**
   (`cargo xtask doc`) -- see "Doc gate" below
8. **`xtask`'s own tests pass** -- this step runs `-p xtask`
   only, which is why the run prints `Test (xtask only)`
9. **Coverage >= 90% overall and >= 85% per module** -- one
   file below the per-module floor fails the run even when the
   workspace figure passes. `xtask/src/coverage.rs` owns both
   as `OVERALL_THRESHOLD` and `MODULE_THRESHOLD`. This is
   also where the *workspace* tests run, under
   `llvm-cov --workspace --exclude xtask`; splitting them that
   way stops the same tests being compiled and run twice
10. **Security audit** (RUSTSEC; `cargo xtask audit`) --
   a positive vulnerability fails; an unreachable advisory
   DB degrades to a warning

**Dep-age, Deny and Audit are the supply-chain three**, and
`docs/developer/supply-chain.md` explains each one: why `deny`
runs offline and in CI while `audit` deliberately does not, and
why `Validate OK` does not mean the dependencies were audited.
Refer to them by name rather than by number -- a list and a run
order that disagree is how the two documents drifted before.

Why that order: the cooldown gate is first because it is a
no-op on an unchanged lockfile and fails fast on a
within-cooldown dependency **before anything compiles it or
runs its build script**. After it the cheap static gates run,
then the expensive dynamic ones, and the network audit last. A
failed step prints the single command to re-run just that gate.

### Doc gate: two rustdoc passes, not one

`cargo xtask doc` runs rustdoc **twice** under
`RUSTDOCFLAGS=-D warnings`: once normally, and once with
`--document-private-items`. That is not a redundant second
pass: a broken doc link fails in one of two ways, and neither
pass catches both.

- A link **inside a private module** naming something not in
  scope. The public pass never renders a private module's docs,
  so it reports nothing at all.
- A **public page linking to a private item**. This is an error
  in the public pass and perfectly legal in the private one --
  rustdoc even suggests `--document-private-items` to make it
  resolve.

Both cases were live in this repo when the gate was added, and
each was invisible to the other pass. If you are tempted to drop
one pass to save a second, note that the remaining one will keep
reporting success on the class it cannot see.

The `-D warnings` is what makes it a gate: rustdoc's link lints
are warnings by default, so a broken link otherwise builds
cleanly and the docs quietly stop navigating.

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
| `/check` | Type-check all targets, incl. tests; runs none |
| `/test` | Run tests with agent-friendly output |
| `/validate` | Full quality pipeline with stepwise progress |
| `/review` | The three reviewers in sequence, red-team looping until behaviour settles; commits nothing, independent of `/commit` |
| `/commit` | Save-point commit with diary and CHANGELOG (no reviewing, no version bump) |
| `/release` | Cut a SemVer release: bump the version, promote `[Unreleased]`, validate, commit, and tag |
| `/retrospect` | Workflow retrospective (Efficiency / Quality / Speed / Cleanup). Invoked automatically by `/commit`; also callable manually mid-session |
| `/rundown` | Grouped one-line rundown of the session's work, ending with the decisions and actions left for the operator. Reports only -- changes nothing |
| `/todo` | Capture a work item into `docs/todo.md` (no implementation) |
| `/implement` | Plan + implement a captured item; writes `docs/issues/<slug>.md` |
| `/issue` | Work a GitHub issue end to end: verify, implement, review, PR |
| `/update-deps` | Upgrade third-party deps to the newest versions outside the 14-day cooldown |
| `/simplify` | Review changed code for quality |
| `/architect` | Project overview and architecture guide |
| `/short` | Restate the reply above, or answer an instruction, in under 40 words |
| `/html-report` | Produce a self-contained local HTML report from the in-repo template (never a cloud Artifact) |
| `/template-improve` | Log feedback for the rustbase template |
| `/template-sync` | Sync upstream template changes |

## Template tooling: determinism vs judgment

The template-maintenance workflows (`/template-sync`,
`/template-backfeed`, `/template-improve`) split their work
into two kinds, and the split matters:

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

## Build and toolchain recipes

Two recipes live in `docs/developer/build-recipes.md`, because
each is needed rarely and neither is a rule you follow on every
commit: **scoped `unsafe` in `xtask`** (the workspace forbids
`unsafe_code`, so build tooling that needs an OS API redefines
the lint block for `xtask` alone) and **coverage exceptions for
hardware-bound code** (extract the unmockable I/O into a leaf
submodule and name it in `[workspace.metadata.coverage]`, so the
90% gate stays honest). An appendix at the end of that file
holds the **edition-2024 migration** fixes, which bombyx is
already past.

Read that file before weakening a lint or a gate. The rule those
recipes exist to protect: production crates keep
`[lints] workspace = true` and stay `unsafe`-forbidden, and a
coverage exclusion covers the I/O leaf, never the orchestrator
around it.

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
- **Caller side** -- **never pipe a command whose exit status
  is the thing being verified.** A shell pipeline reports only
  its *last* command's status, so `cmd | tee log` returns
  `tee`'s success even when `cmd` failed. A real
  `bombyx provision` run against the VM host failed on the
  remote side and was read as passing for exactly this reason;
  the failure was visible only in the log text. Redirect
  (`cmd > log 2>&1`) or run the command bare and read the
  captured output, and when the status matters, print it
  (`echo "EXIT=$?"`).
- **Caller side** -- **a bombyx command that boots or
  provisions a VM belongs in `run_in_background`.** `up`,
  `provision` and `scratch` wait on a download, a domain and a
  guest boot, so they run for minutes rather than seconds; a
  first `up` against a box the host does not have took most of
  ten minutes here. In the foreground that time buys nothing,
  because the session sits idle until the VM answers. Start it
  in the background and read the log when the notification
  arrives. `status`, `doctor` and the teardown commands are
  quick enough to run in front.

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

The detail lives in `docs/developer/supply-chain.md`: what each
of the six guard commands does, why `deny` runs offline in CI
while `audit` deliberately does not, why the licence file is
over-inclusive on purpose, and what none of it covers.

Hold these rules without opening that file:

- **Do not adopt a dependency version published fewer than 14
  days ago without a stated justification.** That window is when
  a compromised release is most likely still live. Security
  fixes are exempt. Check a candidate with
  `cargo xtask dep-age cargo <crate> <version>`; it exits
  non-zero inside the cooldown. When you do adopt a fresh
  version deliberately, name it in `RUSTBASE_DEP_AGE_ALLOW`
  (`name@version`, comma-separated) so the gate passes and
  leaves a record of what was waved through.
- **`Validate OK` does not mean the dependencies were audited.**
  Inside `validate`, a missing `cargo-audit` or an unreachable
  advisory DB is a printed warning, so an offline machine is not
  blocked. The standalone `cargo xtask audit` errors on both,
  and that is the spelling a release uses.
- **A lockfile-churning `cargo update` will fail the cooldown
  gate on transitive crates**, often several at once. That is
  intended: a bulk update is exactly when a freshly published
  release slips in. Either wait the window out, bulk-approve
  the versions you reviewed by listing them all in
  `RUSTBASE_DEP_AGE_ALLOW`, or prefer
  `cargo update -p <crate>` so the flagged set stays small
  enough to read. This is the one that fires when you were not
  expecting it, which is why it is here and not only in the
  reference file.
