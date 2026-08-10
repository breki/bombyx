# provision-command

**Status:** Done
**Captured:** 2026-08-10
**Started:** 2026-08-10
**Completed:** 2026-08-10

## Problem

`bombyx up` pushes the project's `vagrant/` directory to the
host, but vagrant provisions a machine only when it first
creates it: every later `vagrant up` skips the provisioners,
whether the VM was halted or running. So an edited
`provision.sh` lands on the host and nothing ever executes it. The push reports success, which is
what makes the gap hard to see: the operator has every reason
to believe the change was applied.

Applying a provisioning edit today means `ssh <host>` and
`vagrant provision` by hand. That contradicts the premise
bombyx exists to serve -- the operator stays on the workstation
and never logs into the VM host to drive vagrant.

Found while fixing a dash login shell in the jutro agent VM:
the fix went into `provision.sh`, and there was no bombyx
command that would apply it.

## Context

Relevant files:

- `crates/bombyx/src/plan.rs` -- `Action` enum, `pushes()`, and
  `plan()`, which maps an action to its ordered commands.
  `boot()` (`plan.rs:112`) is the shared push-then-`vagrant up`
  helper used by `up` and `scratch`.
- `crates/bombyx/src/remote.rs` -- `vagrant_in()` builds a
  `cd <dir> && vagrant <args>` command; `push_dir()` and
  `ensure_dir()` build the push sequence.
- `crates/bombyx/src/bin/bombyx/main.rs` -- the clap `Cmd` enum
  (`main.rs:44`) and `action_of()` (`main.rs:144`), which maps a
  subcommand to an `Action`.
- `crates/bombyx/tests/integration_test.rs` -- drives the real
  binary under `--dry-run` and asserts on the emitted argv.

Current behaviour: `Action::pushes()` returns true only for
`Up` and `Scratch`, which is what allocates the temp workspace
for the archive (`main.rs:104`). A new pushing action has to be
added there or the archive directory will not exist.

Constraints:

- "Wrap, don't reimplement" -- `vagrant provision` already
  exists; bombyx only has to route to it.
- Every action renders its dry run through `plan()`, so no
  subcommand can describe a run it would not perform. The new
  action must go through `plan()` like the rest.

## Open questions

None outstanding; the scratch-scope question is answered under
Decisions.

## Plan

1. `plan.rs`: add `Action::Provision`, include it in
   `pushes()`, and add a `plan()` arm.
2. `plan.rs`: generalise `boot()` into a helper that takes the
   final vagrant subcommand, so `up`/`scratch` (`up`) and
   `provision` (`provision`) share one push sequence. Keeping
   one helper is what stops `provision` drifting into pushing
   differently from `up`.
3. `main.rs`: add `Cmd::Provision` with help text, and the
   `action_of()` arm.
4. `README.md`: add `bombyx provision` to the command list and
   say why it exists -- that `up` does not re-provision a
   running VM.
5. `CHANGELOG.md`: an `### Added` bullet under `[Unreleased]`
   (handled by `/commit`).

## Test strategy

Cheapest level that proves the behaviour, per the project rule:

- **Unit tests in `plan.rs`** for the plan shape: `provision`
  emits mkdir, tar, scp, extract, then `vagrant 'provision'`;
  and `Action::Provision.pushes()` is true. The existing
  `pushes()` test enumerates every variant, so it gains the new
  one.
- **Integration test** (`--dry-run` against the real binary)
  for the CLI wiring: the subcommand exists, reaches a push,
  and ends in `vagrant 'provision'` in the project directory.

This is a behaviour change in existing code (a new arm in a
match whose siblings are all tested), so the failing test comes
first.

**A dry run is not a real run.** The change alters the commands
bombyx emits, so Definition of Done item 3 applies: it must be
exercised against frosti before this is called done. That is
practical here -- the jutro VM is running and has a
provisioning edit (the `chsh` to bash) that has not been
applied, so a real `bombyx provision` has an observable effect
to verify.

## Progress log

- **2026-08-10** -- Red step confirmed: the four new tests
  failed to compile against the missing `Action::Provision`.
- **2026-08-10** -- Implemented. `boot()` became
  `push_then()`, taking the final vagrant subcommand, so `up`,
  `scratch` and `provision` share one push sequence
  (`plan.rs:139`). CLI wiring at `main.rs:47` and
  `main.rs:152`. Command list updated in `README.md`,
  `crates/bombyx/README.md` and `llms.txt`.
- **2026-08-10** -- `cargo xtask validate` passes all eight
  gates; coverage 99.4%.
- **2026-08-10** -- Exercised against frosti twice, against
  the real jutro VM. See Outcome.

## Decisions

- **2026-08-10 -- A separate `provision` subcommand rather than
  `up --provision`.** It works on a running VM without implying
  a boot, and it keeps `up` from growing a flag that changes
  what the same command means.
- **2026-08-10 -- `provision` pushes before running.** The
  whole point is applying a locally edited script, so a
  non-pushing variant would run whatever stale copy is already
  on the host.
- **2026-08-10 -- The project VM only; no scratch variant.**
  Asked and answered: a scratch VM is disposable, so the answer
  to "its provisioning changed" is `discard` then `scratch`,
  which costs a boot and matches what the ephemeral lifecycle
  is for. An optional name argument would quietly invite
  treating scratch VMs as long-lived, which is the opposite of
  their purpose. Revisit only if iterating on provisioning
  inside a scratch VM turns out to be common.

- **2026-08-10 -- `provision` requires an existing VM; it does
  not boot one.** Raised in review: on a VM that was never
  booted, `provision` creates the remote directory and ships
  the archive before vagrant reports it has nothing to
  provision, which is the change-state-then-fail shape
  `doctor` exists to avoid. Using `vagrant up --provision`
  instead would be self-correcting, but it makes a command
  named `provision` start a machine, and that is a worse
  surprise than a clear failure. Documented as a precondition
  in the clap help and `README.md` rather than papered over.
  The blast radius is small -- the tree pushed is identical to
  what `up` would push.

## Outcome

`bombyx provision` pushes the project's Vagrant directory and
then runs `vagrant provision` in the project directory on the
host. Project VM only; there is no scratch variant.

Shipped:

- `Action::Provision` (`plan.rs:30`), included in `pushes()`
  (`plan.rs:54`) so the archive workspace is allocated.
- `boot()` generalised into `push_then()` (`plan.rs:139`),
  which takes the closing vagrant subcommand. `up`, `scratch`
  and `provision` now share one push sequence, which is what
  stops `provision` from drifting into pushing differently
  from `up`.
- `Cmd::Provision` and its `action_of` arm (`main.rs:47`,
  `main.rs:152`).
- Four unit tests in `plan.rs` and one `--dry-run` integration
  test. `provision_never_boots` asserts the plan contains no
  `vagrant 'up'` at all: a `provision` that quietly started a
  halted VM would be doing more than its name claims.
- Command list updated in `README.md`,
  `crates/bombyx/README.md` and `llms.txt`, with a paragraph
  in the main README explaining why `up` is not enough.

`cargo xtask validate`: all eight gates, coverage 99.4%.

### Verified against a real host

Run against frosti, targeting the jutro agent VM. The first
run **failed on the host**, which turned out to be the more
useful result: a bug in jutro's own `provision.sh` made
`vagrant provision` exit 1, and bombyx surfaced the vagrant
output and propagated the non-zero exit rather than reporting
success. Error propagation was exercised for free.

The second run, after jutro's script was fixed, completed with
exit 0. The observable effect was checked independently rather
than inferred from the exit code: the pending provisioning
edit set the VM's login shell to bash, and
`vagrant ssh -c 'getent passwd vagrant | cut -d: -f7'`
afterwards returned `/bin/bash`.

Both runs also confirmed the push half: the second run's
output shows the *edited* script running, not the copy already
on the host.

### Follow-ups

- The failure that the first run exposed was in jutro, not
  bombyx: a guard ran `swapon --show` without `sudo`, and
  `swapon` is in `/usr/sbin`, off the non-interactive `PATH`
  for an unprivileged user. It is the same trap
  `docs/vm-host-setup.md` documents for vagrant, one level
  down in the guest. Worth remembering that the lesson
  generalises to any `/usr/sbin` tool a provisioning script
  probes.
- Measurement note, not a code issue: the first run was piped
  through `tee`, so the reported exit status was `tee`'s, not
  bombyx's. A pipeline masks the exit code of every command
  but the last -- do not pipe a command whose exit status is
  the thing being verified.
