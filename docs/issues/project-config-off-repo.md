# project-config-off-repo

**Status:** Planning
**Captured:** 2026-08-30
**Started:** 2026-09-02

## Problem

`docs/trust-boundary.md` states two rules. The guest is the
only machine holding the project's source, and neither the
workstation nor the VM host reads any file from the project's
repository. The first is reached. The second is not, because
bombyx reads `bombyx.toml` out of the working directory.

Moving that file is what this item covers. It is a design
question rather than a deletion: the config has to live
somewhere, and bombyx has to work out which project a command
is about without opening anything the repository ships.

## Context

### What reads the project file today

`main.rs:39` defaults `--config` to `bombyx.toml`, so a bare
`bombyx up` reads the file in the working directory.
`Config::load` (`config.rs:398`) reads it, then reads an
overlay beside it (`bombyx.local.toml`, from
`read::local_config_path`), then ranks four host sources and
merges.

Six values come out of the repository: `project`,
`vagrant_dir`, `remote_root`, `[vm]` and `[source]`. `host`
already comes from the operator: `--host`, `BOMBYX_HOST`, the
overlay, or `config.toml` in the user config directory.
`config/host.rs` owns that ranking and `bombyx.toml` is
refused a `host` key outright.

### The push is already dead weight

`push_then` (`plan.rs:180`) ensures the remote directory
exists, unpacks the project's `vagrant/` into it, and *then*
writes the generated Vagrantfile and bootstrap over the top.
The generated Vagrantfile disables Vagrant's default
`/vagrant` share (`vagrantfile.rs:112`) and its only `path:`
names bombyx's own bootstrap script (`vagrantfile.rs:120`).
`source.script` is resolved inside the guest's own clone, not
in the pushed copy.

The VM host receives an archive that no program there reads.

This matters for ordering. `vagrant_dir` is the only config
value naming a location inside the checkout, and the push is
its only consumer. Removing the push removes `vagrant_dir`,
and then no repo-relative path is left for an off-repo config
to resolve. Doing the move first would instead require a
per-project absolute checkout path on the workstation, which
is the dependency this work exists to delete.

`docs/todo.md` states the opposite ordering, under
`remote-clone-project-source`. That note was written before
the generated Vagrantfile disabled the share, and it is wrong
now.

## Open questions

- Ordering: drop the push first, or move the config first?
- Where the per-project configuration lives, and its shape.
- How bombyx is told which project a command is about.
- What happens when there is no entry for a project.
- What becomes of the `bombyx.local.toml` overlay, which is
  defined as sitting beside the project file.
- Whether reading `.git/config` in the working directory to
  find the remote URL is inside the boundary. It is not
  committed content and no branch can change it, but it is a
  file in the project's directory.

## Plan

Three chunks, in this order. Each is its own commit.

**1. Drop the push.** Delete `remote::push_dir`,
`PushArchive`, `Action::pushes` and the `vagrant_dir` field,
and remove the `tar`/`scp` pair from `push_then` in
`plan.rs`. `bombyx up` goes from seven remote commands to
four. `doctor`'s `vagrant_dir` probe goes too. This is the
only chunk that changes what runs on the VM host.

**2. Move the project settings into the registry.** Extend
the per-developer `config.toml` with a `[projects.<name>]`
table carrying `remote_root`, `[vm]`, `[source]` and an
optional `host`. Delete `ProjectFile`, `Overlay`,
`read::local_config_path`, `Config::with_overlay` and
`HostOrigin::Overlay`. `Config::load` stops taking a path to
a project file and takes a project name instead. Host
ranking becomes flag, environment, the project's `host`, the
top-level `host`.

**3. Select the project explicitly.** `--project <name>`
becomes a required global argument for every `Cmd::Vm`
subcommand, and `--config` keeps pointing at the registry
file rather than at a project file. `bombyx self-update`
needs neither, as today.

Documentation lands with the chunk that causes it.
`README.md`'s Configure section inverts: it currently
explains the split as committed-project-file versus
private-host-file, and that framing does not survive.
`docs/trust-boundary.md` gets its second statement marked
reached. `docs/architecture.md`, `docs/tutorial.md` and
`docs/usage.md` all describe the overlay and the push.

## Test strategy

Rust unit tests for the config loading and lookup, and
integration tests with `--dry-run` against the real binary
for the plan changes, which is where the removed `tar`/`scp`
commands are asserted today (`plan.rs:384` and `:487`).

Definition of Done item 3 applies to chunk 1: it changes the
commands bombyx emits, so it needs a real run against frosti.
Chunk 2 changes only where values are read from, and the
emitted commands stay identical, which a dry run can show.

## Decisions

- **2026-09-02 -- Drop the push before moving the config.**
  The push is dead weight already, and `vagrant_dir` is the
  only config value naming a location inside the checkout.
  Removing the push first means the moved config never needs
  a path to a checkout. `docs/todo.md` asserts the opposite
  ordering under `remote-clone-project-source`; that note
  predates the generated Vagrantfile disabling the `/vagrant`
  share and is wrong. Fix it there.

- **2026-09-02 -- One registry file, not a file per
  project.** Every project is a `[projects.<name>]` table in
  the per-developer `config.toml`. Everything the operator
  configures is then in one file they own.

- **2026-09-02 -- No overlay file.** `bombyx.local.toml`
  exists because the base file is committed and shared, so
  one developer needs a private way to differ from it. Once
  the base file is the operator's own private file, an
  overlay is a file overriding a file only its owner can
  edit. It buys nothing, so `Overlay` and
  `local_config_path` go. The one thing it did that the
  registry could not -- a different VM host for one project
  -- becomes an optional `host` key in that project's table.

- **2026-09-02 -- Name the project explicitly.**
  `--project <name>`, rather than matching the working
  directory's git remote against the registry. bombyx then
  reads nothing at all from the project's directory, which
  is the boundary stated without an exception to explain.
  Matching the remote is not captured as a follow-up; if the
  argument becomes tiresome, that is when to design it.

- **2026-09-02 -- A missing entry is an error.** It names
  the registry file and the keys the entry needs. Nothing is
  written on the operator's behalf, which is how a missing
  `bombyx.toml` behaves today.
