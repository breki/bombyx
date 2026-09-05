# project-config-off-repo

**Status:** In progress -- chunk 1 landed 2026-09-02, steps 1
to 4 of 7 landed 2026-09-04, step 5 landed 2026-09-05. Two
steps remain: #18 switches the tool over, #27 settles
`destroy`'s positional.

The Status line and the **Progress log** are written when a step
is committed, so they lead `docs/todo.md`: an entry there moves
from **Pending** to **Done** when its pull request is merged,
which is later. A step named here and still pending there is on
a branch, not lost.

This document uses two words for two things. **Chunks** are the
original three, and **Plan** below still describes them. **Steps**
are the seven that chunks 2 and 3 were re-split into, and they
are what the work actually follows. The seven are listed under
**2026-09-04 -- chunks 2 and 3 re-split into seven steps** in
the **Progress log**, and that list is the live plan.

**Problem**, **Context** and **Open questions** describe the
state when this was captured, before chunk 1, and are frozen.
**Plan** is frozen too, except that its chunk-2 section is kept
current as steps land. The **Progress log** at the end says what
has changed. Line numbers in this document are from the day it
was written and the work it plans moves them; treat them as a
hint, not an address.
**Captured:** 2026-08-30
**Started:** 2026-09-02

This document plans the whole move out of the repository. It
began as three items and is now eight: chunk 1 landed as
`remote-clone-project-source` (GitHub #10), and chunks 2 and 3
were re-split into seven steps on 2026-09-04. The **Re-split**
entry in the progress log lists them and says why. It is filed
under the middle slug because that is where the design work
sits.

## Problem

`docs/trust-boundary.md` states two rules. The guest is the
only machine holding the project's source code, and neither
the workstation nor the VM host reads any file from the
project's repository.

Neither rule holds today.

The guest no longer receives a mount of the project, because
the generated Vagrantfile disables the `/vagrant` share. But
the workstation still holds the checkout the push is built
from and the VM host still holds the unpacked copy, so two
machines outside the guest hold the project's source, and the
first rule fails.

`docs/trust-boundary.md:45` claims the first rule is reached.
That line contradicts its own document, which says at `:63`
that "two machines end up holding project files". Correcting
it belongs with chunk 1, which is what makes the claim true.

Two files block the second rule. `bombyx.toml` is read from
the working directory, and the `vagrant/` directory is pushed
to the VM host. `bombyx.local.toml` is read from the working
directory as well. `.gitignore:38` lists it, which stops an
accidental commit and not a deliberate one -- the comment above
that line says as much, and a force-added file is tracked like
any other. So it is inside the second rule, not outside it.
Chunk 2 removes it anyway, for the separate reason given under
`## Decisions`.

This document covers both blockers. Removing the push is a
deletion. Moving the config is a design question: the config
has to live somewhere, and bombyx has to work out which
project a command is about without opening anything the
repository ships.

## Context

### What reads the project file today

`crates/bombyx/src/bin/bombyx/main.rs:45` defaults `--config`
to `bombyx.toml`, so a bare `bombyx up` reads the file in the
working directory. (There are two `main.rs` files in this
repo; the other is `xtask`'s.) `Config::load`
(`config.rs:398`) reads it, then reads an overlay beside it
(`bombyx.local.toml`, from `read::local_config_path`), then
ranks four host sources and merges.

Five values come out of the repository: `project`,
`vagrant_dir`, `remote_root`, `[vm]` and `[source]`. `host`
already comes from the operator: `--host`, `BOMBYX_HOST`, the
overlay, or `config.toml` in the user config directory. That
last file is called **the registry** from here on, because
the plan below grows it into one. `config/host.rs` owns the
ranking, and `bombyx.toml` is refused a `host` key outright.

### The push is already dead weight

`push_then` (`plan.rs:181`, renamed `write_then` by chunk 1)
ensures the remote directory
exists, unpacks the project's `vagrant/` into it, and *then*
writes the generated Vagrantfile and bootstrap over the top.
The generated Vagrantfile disables Vagrant's default
`/vagrant` share (`vagrantfile.rs:112`) and its only `path:`
names bombyx's own bootstrap script (`vagrantfile.rs:120`).
`source.script` is resolved inside the guest's own clone, not
in the pushed copy.

The VM host receives an archive that no program there reads.

This matters for ordering. `vagrant_dir` is the only config
value naming a location inside the checkout. It has two
consumers, and chunk 1 removes both: the push, and the
`doctor` check that looks for a Vagrantfile in that directory
(`main.rs:342` hands `doctor_run` a path derived from it).
Removing them removes `vagrant_dir`, and then no repo-relative
path is left for an off-repo config to resolve. Doing the move
first would instead require a per-project absolute checkout
path on the workstation, which is the dependency this work
exists to delete.

## Open questions

Three are live, and none of them blocks chunk 1.

- Whether reading `.git/config` in the working directory to
  find the remote URL is inside the boundary. `.git/config`
  is not part of what a repository ships: `git` writes it
  locally when you clone, and checking out a branch never
  rewrites it. So somebody who controls a branch cannot plant
  a value there for bombyx to act on, which is the threat the
  boundary exists to stop. It is still a file in the
  project's directory, which is what makes it a question.

  No part of the plan waits on this answer, because chunk 3
  makes the operator name the project instead. The question
  returns if that argument becomes tiresome.

- Whether `remote_root` stays per-project, as chunk 2 has it
  and as it is today, or becomes a top-level default in the
  registry with a per-project override.

- What happens to `VmCmd::Destroy`'s confirmation positional
  once `--project` exists. Chunk 3 names the two candidate
  shapes.

## Plan

Three chunks, in this order. Each is its own commit.

### 1. Drop the push

`remote::push_dir` (`remote.rs:282`) returns three commands,
not two: `tar`, `scp`, and the `ssh` that unpacks the archive
and deletes it. All three go, and with them `push_dir`
itself, `PushArchive`, `Action::pushes` and the `vagrant_dir`
field.

`plan()` loses its `local_dir` and `archive` parameters
(`plan.rs:93`). In `main.rs`, both the `local_dir`
computation (`:310`) and the `TempDir` workspace built when
`action.pushes()` is true (`:318`) go, along with the
`PushArchive` they feed (`:325`). `doctor`'s local
`vagrant_dir` check goes too --
`doctor::local::vagrantfile_finding` (`doctor/local.rs:115`),
which reads the filesystem here and is not one of the probes
sent to the host.

`bombyx up` goes from seven steps to four: `mkdir`, the two
generated-file writes, and `vagrant up`. Of the original
seven, `tar` and `scp` run on the workstation, so the count
is of `RemoteCommand` values rather than of processes on the
VM host.

`docs/trust-boundary.md` gets the VM host's half of the first
statement made true. The workstation keeps its checkout until
chunk 2, so the statement as a whole is not reached here.

This is the only chunk that changes what runs on the VM host.

### 2. Move the project settings into the registry

The registry gains a `[projects.<name>]` table per project,
carrying `remote_root`, `[vm]` and `[source]`. The optional
`host` key is step 4's work, not step 3's, so an entry written
after step 3 has no `host` in it.

Delete `ProjectFile`. Step 1 removed `Config::with_overlay`
and the call site that printed the "overrides" notice, and
step 2 removed `Overlay`, `read::local_config_path` and
`HostOrigin::Overlay`, so `ProjectFile` is the last of the
list. A second loader, `Config::load_project(name, sources)`,
takes a project name where `Config::load` takes a path, and
step 6 deletes `Config::load` rather than changing its
signature -- the decision under **The registry loader is
`Config::load_project`** says why a sibling and not a rewrite.
Host ranking becomes flag, environment, the project's `host`,
the top-level `host`.

### 3. Select the project explicitly

`--project <name>` becomes a required global argument for
every `VmCmd` variant (`main.rs:90`). `bombyx self-update`
needs it no more than it needs a config today.

`--config` names the registry file: today it defaults to
`bombyx.toml` in the working directory, afterwards to
`config.toml` in the user config directory.

`VmCmd::Destroy` already takes a positional `project`
(`main.rs:115`), which exists as a confirmation prompt rather
than as selection. Two shapes to choose between: drop the
positional and confirm some other way, or keep it and let
`bombyx --project myproj destroy myproj` read as a typed
confirmation. Undecided.

### Documentation

Each chunk carries its own. `README.md`'s Configure section
inverts: it explains the split as committed-project-file
versus private-host-file, and that framing does not survive.
`docs/trust-boundary.md` gets both statements marked reached,
in the chunk that reaches each. `docs/architecture.md`,
`docs/tutorial.md` and `docs/usage.md` all describe the
overlay and the push.

## Test strategy

Rust unit tests for the config loading and lookup.

The `plan.rs` test module is written throughout against a
plan that pushes, so expect to rework most of it rather than
a named few. `plan_for` and `run` both construct a
`PushArchive`; `up_makes_the_dir_then_pushes_then_boots`
(`:384`), `a_push_writes_both_generated_files_before_booting`
(`:425`), `scratch_pushes_before_booting` (`:470`),
`provision_pushes_then_reprovisions` (`:487`) and
`only_pushing_actions_need_an_archive` (`:728`) all assert
behaviour chunk 1 deletes.

The binary-level tests run the real binary under `--dry-run`.
Three pin the seven-element program vector
(`integration_test.rs:165`, `:273`, `:292`), and
`up_never_hands_scp_a_windows_drive_letter` (`:210`) searches
the output for an `scp` line and unwraps, so it panics rather
than fails once no such line is emitted.

Definition of Done item 3 applies to chunk 1: it changes the
commands bombyx emits, so it needs a real run against frosti,
the libvirt VM host this project is developed against. Chunk
2 changes only where values are read from, and the emitted
commands stay identical, which a dry run can show.

## Decisions

- **2026-09-05 -- `--config` names the registry file, and
  `HostSources` goes with the flag.** The operator chose a file
  path over a directory path and over dropping the switch. So
  `--config` takes the path to `config.toml` and defaults to the
  one in the config directory, which is what this document and
  `docs/todo.md` already promised.

  Three things follow inside the library. `Registry::read` takes
  the file rather than the directory holding it, and
  `config::registry_file` does the join once for the binary's
  default. `registry_place` takes that same optional path, so a
  machine whose environment names no config directory still gets
  the message describing the file instead of a path.

  And `HostSources` is deleted. Step 6 removes `--host`,
  `BOMBYX_HOST` and `Config::load`, which leaves the struct
  carrying one field, `user_config_dir`, that every caller fills
  in the same way -- and `from_registry` already overwrites
  `project` rather than reading it. A struct wrapping one value
  its owner ignores is worse than the value: `load_project` now
  takes `Option<&Path>` directly.

- **2026-09-05 -- Every `host` in the registry is checked as the
  file is read.** The operator's rule, given during the review
  of step 5: all hosts in the file are checked when the file is
  read and reported as errors, whatever the command line says,
  and `--host` is checked separately. So
  `config::registry::parse` applies `host_problem` to the
  file-wide `host` and to every `[projects.*].host` before it
  builds a `Registry` at all.

  This reverses two reasons written down in earlier steps.
  `Registry::host` said checking there "would report a value
  this file supplies even on a run that never uses it", and
  `Registry::project_host` said the same. Those reasons landed
  in `b534bb1` and were hardened in `59dd110` after a review
  round on #25. That is exactly the behaviour the operator
  wants: this file is where host names are written, so a value
  bombyx would refuse is a mistake to report while they are
  looking at it, and checking only the winner leaves a typo in
  an unused line until the day that line wins.

  Two things follow. Holding a `Registry` is now the proof that
  every host in it passed, so `Registry::host` and
  `Registry::project_host` run no rule and `Project::validate`
  no longer checks `host` -- one place owns it. And the entry
  host's error now names its table, through
  `HostOrigin::ProjectEntry`, where the field-named message used
  to win the race; that was the review finding that started the
  discussion.

  **What this changes in the CLI**, corrected after a review
  round caught the first version of this paragraph claiming
  nothing did. A bad `host` anywhere in the registry now fails
  every command that reads the file, and that is every command
  run without `--host`. Verified by running the binary: a
  registry whose `[projects.other].host` starts with `-` makes
  `bombyx --dry-run status` for an unrelated project exit 1,
  where the same file at `b491701` ran normally. The first
  version of this paragraph was written after checking only the
  file-wide `host`, which is the one case that did not change.

  Two things are unchanged, both verified the same way. With
  `--host`, `resolve_host` never opens the registry, so a bad
  host in it is still unreported -- an old deliberate property,
  so that the flag works on a machine whose registry is broken.
  And a bad file-wide `host` with no flag is refused with the
  same error naming the same file as before.

  Nothing that shipped is affected. `v0.4.1` parsed the registry
  with `deny_unknown_fields` on a struct carrying `host` alone,
  so a released bombyx refused any `config.toml` containing a
  `[projects.*]` table outright. The behaviour this changes was
  introduced in steps 3 and 4 and has never been released, which
  is why the CHANGELOG entry carries no `**BREAKING:**` marker.

  A `HostName` newtype would be the stronger form of this and is
  not built here: `host` is one of the five fields
  `newtype-remaining-config-fields` (#17) covers.

- **2026-09-04 -- The registry loader is
  `Config::load_project(name, sources)`.** Step 6 deletes
  `Config::load`, so the name chosen here is the only loader
  bombyx has afterwards, and renaming it then would be churn in
  the step that already carries every document. "load" states
  that the function opens a file, which the common errors are
  all about: no `config.toml`, no entry in it, a broken entry.
  `for_project` hides that.

  It splits in two. `load_project` reads the registry and hands
  off to a private `from_registry(registry, name, sources)`,
  which looks the entry up, ranks the host, checks the winner
  and validates. The pure half is what the test-only
  constructor calls, so a test needs no temporary directory.

  `config/host.rs` splits the same way and for a harder reason.
  `resolve_host` reads the registry itself, and the loader needs
  the same file for the entry. Its own comment says why reading
  it twice is wrong: a file edited between the two reads can
  supply a project host and a file-wide host that never
  coexisted. So the four-source precedence moves into a pure
  `rank(sources, Option<&Registry>)`, and `resolve_host` becomes
  read-then-`rank` for `Config::load` alone until step 6 deletes
  it.

  `from_registry` overwrites `sources.project` with `name`
  rather than reading what the caller put there. The project
  whose host is ranked is then always the project the rest of
  the config came from, and a caller cannot make the two differ.

- **2026-09-04 -- A missing registry file gets its own error,
  `RegistryNotFound`.** It carries a `place` rather than a
  `PathBuf`, so it reuses the function that words the file for
  the two host messages, which already covers the case where the
  environment names no config directory at all. That function is
  now `registry_place`: a third message uses it, and it was
  never about the host.
  The message names the file and says to create it with a
  `[projects.<name>]` table.

  `ConfigError::NotFound` was the alternative and says only that
  a file is absent, leaving the operator to find out what goes
  in it. `ProjectNotFound` already names the tables to write,
  but its message claims bombyx looked inside a file that does
  not exist.

- **2026-09-04 -- `Config::parse` becomes registry-based, and
  the project-file tests get their own helper.** `parse` and
  `for_tests` are both test-only constructors built from a
  `bombyx.toml` string, and every test module in the crate uses
  `for_tests`. Moving both onto the registry now means step 6
  deletes `ProjectFile` and its tests rather than rewriting the
  helper every module depends on.

  The forty-odd tests in `config.rs` that are *about* parsing
  `bombyx.toml` keep testing it. They reach it through one local
  helper in that file's test module, and the helper now does the
  `ProjectFile` parse itself. It is deleted in step 6 along with
  the tests that call it and the struct it parses.

- **2026-09-04 -- The flag and the environment variable go in
  step 6 (`project-selection-flag`), not step 4
  (`registry-project-host`).** The operator wants the VM host tied to
  the project, which means `--host` and `BOMBYX_HOST` are
  deleted: a one-off flag can point `destroy`'s `rm -rf` at a
  machine the project never named. They cannot go in step 4.
  The binary still loads `bombyx.toml`, and that file is
  refused a `host` key outright, so the flag, the variable and
  the top-level `host` are the only three sources bombyx has --
  deleting them here leaves the tool unable to find a host at
  all until steps 5 and 6 land. Step 6 is where `bombyx.toml`
  dies and every setting comes from the registry, so the
  deletion goes there, and #18 carries it. The seven steps are
  listed under **2026-09-04 -- chunks 2 and 3 re-split into
  seven steps** below.

- **2026-09-04 -- The top-level `host` survives as a default.**
  A project entry's `host` wins when it has one; otherwise the
  file-wide `host` applies. Requiring the key in every entry
  writes one machine name once per project, so renaming the
  machine means editing every entry rather than one line. The
  coupling the operator asked for is what the per-project key
  buys, and a default it overrides does not weaken it.

- **2026-09-04 -- `HostOrigin::ProjectEntry` carries the
  project name.** The startup notice exists so the operator can
  see which machine `destroy` will run `rm -rf` on. A project's
  host and the file-wide host come out of the same
  `config.toml`, so "from config.toml" no longer says which one
  won, and a placeholder such as `[projects.<name>]` leaves the
  operator to work it out. Carrying the name costs `HostOrigin`
  its `Copy`, and that cost nothing at the call sites: `!=`
  autoborrows both operands, so `main.rs` compiles unchanged
  against a non-`Copy` enum. Once step 6 deletes the flag and
  the variable, this variant is the only one the notice ever
  prints.

- **2026-09-04 -- `config/registry.rs` owns the registry file.**
  Step 3 adds `[projects.<name>]` to the per-developer
  `config.toml`, and that file had exactly one parse type:
  `UserFile` in `config/host.rs`, carrying `host` alone. Two
  structs deserializing the same file would drift, and
  `deny_unknown_fields` on either would refuse the other's keys.
  So the type moves to a module named after the file, `host.rs`
  keeps the ranking between `--host`, `BOMBYX_HOST` and the
  file, and steps 4 and 5 add to `registry.rs` rather than
  growing a module whose header is about the VM host.

- **2026-09-04 -- Delete the "overrides" notice in step 1.**
  Once the overlay carries only `host`, the line
  `bombyx: bombyx.local.toml overrides bombyx.toml` describes
  something the file cannot do. The host provenance line under
  it already names `bombyx.local.toml` whenever that file
  supplies the host, so nothing true is lost. An overlay file
  present but empty then prints nothing, which is the honest
  report: it changed nothing. The integration test
  `an_overlay_without_a_host_does_not_claim_the_host` exists to
  police the gap between the two notices, and it goes with the
  notice.

  This is the one place step 1 changes behaviour outside the
  overlay, and #22 claims it changes none. The alternative was
  rewording the line to say bombyx *read* the file, which keeps
  a signal that a mistyped filename fell back silently, at the
  cost of a line saying what the line beneath it already says.

- **2026-09-04 -- `resolve_host` keeps `&mut Overlay` and
  `take()` in step 1.** Its doc comment justifies the `take` by
  saying it makes `Config::with_overlay` ignoring `host` safe
  rather than merely intended, and step 1 deletes
  `with_overlay`, so that sentence goes. The mechanism stays:
  step 2 (#23) deletes the whole overlay branch of
  `resolve_host`, so narrowing it to `&Overlay` now is work
  that step 2 throws away.

- **2026-09-02 -- Drop the push before moving the config.**
  The push is dead weight already, and `vagrant_dir` is the
  only config value naming a location inside the checkout.
  Removing the push first means the moved config never needs
  a path to a checkout.

- **2026-09-02 -- One registry file, not a file per
  project.** Every project is a `[projects.<name>]` table in
  the per-developer `config.toml`. Everything the operator
  configures is then in one file they own.

- **2026-09-02 -- No overlay file.** `bombyx.local.toml`
  exists because the base file is committed and shared, so
  one developer needs a private way to differ from it. Once
  the base file is the operator's own private file, an
  overlay is a file overriding a file only its owner can
  edit. It buys nothing, so `Overlay` and `local_config_path`
  go. The one thing it did that the registry could not -- a
  different VM host for one project -- becomes an optional
  `host` key in that project's table.

- **2026-09-02 -- Name the project explicitly.**
  `--project <name>`, rather than matching the working
  directory's git remote against the registry. bombyx then
  reads nothing at all from the project's directory, which is
  the boundary stated without an exception to explain.
  Matching the remote is not captured as a follow-up; if the
  argument becomes tiresome, that is when to design it.

- **2026-09-02 -- A missing entry is an error.** It names the
  registry file and the keys the entry needs. bombyx writes
  nothing on the operator's behalf, which is how a missing
  `bombyx.toml` behaves today.

## Progress log

**Oldest first**, unlike `docs/developer/DIARY.md`. Every step
entry below carries the same date, so the order of the headings
is the only thing that says which landed first -- and two of
them are out of it: step 4's entry sits above step 3's because
it was written first. An entry is written when a step is merged,
so the Status line at the top of this document is what says how
far that has got.

### 2026-09-02 -- chunk 1 landed

The push is gone. `remote::push_dir`, `PushArchive`,
`Action::pushes` and the `vagrant_dir` config field are deleted,
`plan()` lost its `local_dir` and `archive` parameters, and
`main.rs` no longer builds a `TempDir` workspace. `bombyx up` is
four `ssh` commands.

Three things went further than the plan said, each because the
push was their only reason to exist. `doctor` lost its local
`scp` check and its `tar` and `scp` host probes -- bombyx runs
`scp` nowhere now, and `tar` only in `self-update`. It also lost
`vagrantfile_finding` entirely: all three of that check's cases
were about a directory bombyx no longer reads. And
`guards::check_project_relative` went, because `vagrant_dir` was
its only caller.

`host`'s error message named "ssh and scp" and now names `ssh`
alone, which is a correctness fix rather than tidying: the
message tells the operator which program the value reaches.

`cargo xtask validate` passes, coverage 97.9%. Deleting the
integration test that ran a real `doctor` took
`ProbeResult::from_output` below the threshold, so it gained
direct unit tests.

**Not verified against a real VM host.** This chunk changes
what executes there, so Definition of Done item 3 applies and is
not met. frosti was unreachable from this session.

### 2026-09-04 -- step 1 landed

`Overlay` now carries `host` and nothing else. Three more
things went with the four project fields:
`Config::with_overlay`, the `replace` helper, and
`into_config`'s overlay parameter. `HostSources`,
`resolve_host` and the four-source ranking are unchanged, so
`bombyx.local.toml` still supplies a host for one project and
step 2 (#23) is what deletes the file.

Four unit tests went with the merge they described. One
replaces them: a project key in the overlay is now an unknown
field, and `deny_unknown_fields` refuses it naming the file and
the key. It was written first and seen to fail, applying
`project = "other"` before the fields came out.

The `overrides` notice is gone, as decided above. Its
integration test is replaced by one asserting the opposite: an
empty `bombyx.local.toml` makes bombyx say nothing about that
file at all. That test was seen to fail with the notice put
back.

Documents carrying the merge: `README.md` (the section is now
"A different machine for one project"), `llms.txt`,
`bombyx.toml.sample`, `docs/usage.md`, `docs/tutorial.md` and
the class diagram in `docs/architecture.md`. Two doc comments
in `config/host.rs` justified themselves by naming
`with_overlay` and had to be rewritten rather than deleted.

### 2026-09-04 -- step 2 landed

`bombyx.local.toml` is gone. `Overlay`, `local_config_path`,
`HostOrigin::Overlay` and the overlay branch of `resolve_host`
are all deleted. `resolve_host` lost the two parameters that
carried the file, and `host_places` lost the one it carried and
is now `host_place`. Host ranking is now `--host`,
`BOMBYX_HOST`, `config.toml`.

Every source that can name a VM host now sits outside the
checkout. That is what closes the exposure
`rt-2026-09-04-a-committed-overlay-redirects-every-ssh`
described, and three other red-team findings go with it:
`rt-2026-09-04-provenance-line-names-the-default-filename`,
`rt-2026-09-04-a-malformed-overlay-defeats-the-host-flag` and
`rt-2026-09-04-overlay-and-local-config-path-are-pub`. All four
carry a closing line in `docs/developer/redteam-log.md`.

The operator chose the pure removal over stopping with a
message when a stray file is found. A leftover
`bombyx.local.toml` is now an ordinary unread file: bombyx does
not open it, so its contents cannot win and cannot error. The
cost is that anyone relying on one gets their `config.toml`
host with no warning. bombyx is pre-release, so the migration
is one delete.

Two tests were written first and seen to fail.
`a_stray_local_config_is_not_read` failed with `my-vmhost`
winning. The provenance assertion added to
`the_host_env_var_outranks_the_user_config` failed with the
`if` in `main.rs` disabled -- it is there because the overlay
test that used to police that line is one of the five deleted,
and `--host` alone would have left the environment case
uncovered.

`cargo xtask validate` passes, coverage 98.1%.

**Not verified against a real VM host.** The change alters
which host is selected, not the shape of any command bombyx
emits, and the selection is fully visible in `--dry-run`. The
VM host was unreachable from this session in any case: its host
key is not known here.

### 2026-09-04 -- step 4 landed

A `[projects.<name>]` table may carry an optional `host`, and
`config/host.rs` ranks it above the file-wide one. Host ranking
is now `--host`, `BOMBYX_HOST`, the named project's `host`, the
file-wide `host`. `HostOrigin` gained `ProjectEntry`, which
carries the project name because both file sources come out of
one `config.toml` and the notice has to say which won.

`HostSources.project` is `None` on every path in the binary, so
the key never wins yet. Step 6 supplies the first project name.

Three reviewers ran in sequence on this one, after the commit
rather than before it, so two of their findings are corrections
to what the commit says. `Registry::project` handed out an entry
whose `host` no rule had run on, which `Project::validate` now
fixes. And the claim that dropping `Copy` cost two call-site
changes is false -- `!=` autoborrows, so nothing had to change
-- which the commit message still says and cannot be corrected
there.

### 2026-09-04 -- step 3 landed

The registry parses `[projects.<name>]` tables.
`crates/bombyx/src/config/registry.rs` is new and owns the file:
`RegistryFile` parses it, `Project` is one entry, `Registry` is
the parsed file plus the path it came from, and
`Registry::project` looks an entry up and validates it. A
missing entry is `ConfigError::ProjectNotFound`, whose message
names the file and the tables the entry needs. bombyx does not
load a project table yet -- the tests are the only caller.

`UserFile` moved out of `config/host.rs` and became the new
module's business, because `deny_unknown_fields` on it meant the
first `[projects.x]` table made every bombyx command fail. That
was the first failing test. `host.rs` now ranks `--host`,
`BOMBYX_HOST` and the file, and asks the registry to read it.

Two guards came out of the reviews rather than the plan. The
table key is a `ProjectName`, a new newtype beside `ScratchName`,
because the key becomes a directory name on the VM host and
`Config.project` already had that rule. And `MAX_NAME_LEN` moved
into `check_segment`, so a project name over 64 characters is now
refused in `bombyx.toml` too -- previously the same name was
legal there and illegal as a registry key. That is the one
change in this step that narrows what an existing config may
say, and it carries a `**BREAKING:**` CHANGELOG entry.

`cargo xtask validate` passes, coverage 98.3%.

**Not verified against a real VM host.** The commands bombyx
emits are untouched, so Definition of Done item 3 does not apply.

### 2026-09-04 -- chunks 2 and 3 re-split into seven steps

Two findings. Chunks 2 and 3 could not land separately: chunk 2
changes `Config::load` to take a project name, and chunk 3 is
what adds `--project` to supply one, so between them nothing
selects a project. Landing chunk 2 alone would need an interim
selector that chunk 3 then deletes.

And the split was on the wrong axis. Of the 52 references in
`config.rs` to the types this work removes, 41 are the overlay
and 11 are `ProjectFile`. The overlay carries the bulk, and the
plan bundled it into chunk 2 because the *reason* to delete it
depends on the move -- once the base file is the operator's own
private file, an overlay over a file only its owner can edit
buys nothing. The *work* does not depend on the move at all.

Seven steps, each a commit that compiles and passes
`cargo xtask validate`:

1. `overlay-drop-project-overrides` (#22) -- the overlay stops
   overriding project fields
2. `overlay-drop-host-source` (#23) -- delete
   `bombyx.local.toml` entirely
3. `registry-projects-table` (#24) -- `config.toml` gains a
   `[projects.<name>]` table
4. `registry-project-host` (#25) -- an optional `host` key per
   project
5. `registry-config-load` (#26) -- load a `Config` from the
   registry by project name
6. `project-selection-flag` (#18) -- `--project`, and the
   deletion of the project file
7. `destroy-confirmation-shape` (#27) -- what `destroy`'s
   positional becomes

Step 2 is a pure removal needing no design. Step 1 was a
removal plus one deliberate behaviour change, the deleted
"overrides" notice, recorded under **Decisions**. Steps 3 to 5
are pure additions whose only caller is their own tests, so
nothing can regress. Step 6 switches the tool over and carries
every document. Step 7 settles a design question that would
otherwise sit inside step 6.

Two of the three **Open questions** above are now answered.
`remote_root` stays per-project rather than becoming a top-level
default with an override, because one place to look for a value
beats two. `destroy`'s positional is step 7 and still open. The
`.git/config` question stays parked, as the plan always had it.

#16 is closed as split rather than done.

### 2026-09-05 -- step 5 landed

`Config::load_project(name, sources)` reads the operator's
`config.toml`, takes the `[projects.<name>]` table for every
setting but the host, ranks the host across the four sources and
returns the same `(Config, HostOrigin)` pair `Config::load`
returns. A sibling, not a rewrite, as the decision above has it.
Nothing in the binary calls it; step 6 supplies the first caller
and deletes `Config::load`.

`config/host.rs` split. The four-source precedence is now `rank`,
which takes an already-read registry, and `resolve_host` is
read-then-`rank` for the old loader alone. That split exists
because the new loader needs the same copy of the file for the
project's settings, and its own comment said why reading twice is
wrong.

**The review changed more than the step did.** The operator's
rule -- every `host` in the registry checked as the file is read
-- came out of a finding, reverses reasoning written down in
`b534bb1` and `59dd110`, and is the one behaviour change here: a
bad host in any entry now fails every command that reads the
file. The decision above records it. Nothing released is
affected, because `v0.4.1` refused a `config.toml` with a
`[projects.*]` table at all.

Three review rounds each found one copy of one rule: a project
name printed inside a TOML heading must be quoted. The
consolidation landed afterwards as its own commit, `c9c9ac6`, and
`docs/architecture.md` under **The heading spelling has one
owner** holds the reasoning.

Coverage 98.5%, ten gates, seven CI jobs. Definition of Done item
3 does not apply: the loader has no caller and the emitted
commands are unchanged, which a dry run shows.
