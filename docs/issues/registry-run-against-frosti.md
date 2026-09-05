# registry-run-against-frosti

**Status:** Done
**Captured:** 2026-08-30
**Started:** 2026-09-05
**Completed:** 2026-09-05

The record of driving the current bombyx against a real
libvirt host. Every command below was run for real, on
frosti, against a VM that actually booted. Where a claim
rests on a dry run instead, this document says so.

This is a record and not a plan. Nothing in bombyx changed
while it was written: the three defects the run found were
captured in `docs/todo.md` rather than fixed here, so that
the run's result stays readable.

## Why the run was owed

`first-real-run` was the last time bombyx touched a real
host, and it exercised a tool that no longer exists: `up`
from inside a project directory, a committed `bombyx.toml`,
and the tar-push. Seven steps of `project-config-off-repo`
landed after it, and `docs/issues/project-config-off-repo.md`
says "not verified against a real VM host" six times. Every
one of those steps was argued from `--dry-run`, which proves
the argv and nothing about whether the far side accepts it.

## What the machine was

frosti is a laptop, and it is both the workstation and the VM
host. Ubuntu, Vagrant 2.4.9, `vagrant-libvirt` 0.12.2, the
account in the `libvirt` and `kvm` groups.

That single machine is why this item sat parked. Verifying it
looked like it needed SSH from the machine to itself, so the
question went to `local-host-execution` (#38) instead. That
work landed first, and this run is the first use of it: with
`host = "frosti"` bombyx runs `vagrant` here and never calls
`ssh`. `doctor` says so and skips both ssh rows.

## Building the registry was itself the first test

There was no `~/.config/bombyx/` on this machine at all. The
registry every command now requires had never existed outside
a temp directory, so writing one by hand from
`config.toml.sample` was part of what needed testing: the
`/review2` on #18 proved that the sample *loads*, not that
somebody following its comments arrives anywhere useful.

The file that came out of it loaded on the first attempt, and
no comment in the sample had to be reread to work out what a
key wanted. Two of the sample's own values are worth
recording, because the run turned on both:

- `box = "generic/ubuntu2204"` is what the sample writes, and
  it is the value the successful run ended up using. It still
  publishes a libvirt provider, and it carries `git`.
- `remote_root = "~/vms"` put the project at `~/vms/vmtest`.

The project is named `vmtest` rather than after the
repository it clones. `~/vms/jutro` holds a hand-written
Vagrantfile that predates bombyx, and `destroy` runs `rm -rf`
on the directory it derives from the project name, so a test
run must not be able to name that directory even by mistake.

## The first attempt failed, and the failure was the point

The first registry named `cloud-image/debian-13`, a box
already on the machine. `up` created the domain, booted it,
and then stopped:

```
default: bombyx: git is not installed in this box.
default: bombyx: install it in the box, or choose one with git,
         so the guest can clone the project.
```

Exit 1. That is `bootstrap.sh` doing exactly what it is
written to do, and the message names the fix. What the run
adds is when the message arrives: after the box download, the
domain creation and the boot. Captured as
`box-must-carry-git`.

Switching the box to the sample's own `generic/ubuntu2204`
fixed it, and everything below ran on that box.

## What ran

The project cloned is `breki/kozmotic`, which is public and
already carries `vagrant/provision.sh`.

| Command | Result |
|---------|--------|
| `doctor` | passed, 2 skipped (both ssh rows) |
| `up` (debian-13 box) | exit 1, refused for the missing `git` |
| `up` (ubuntu2204 box) | exit 0, provisioned to completion |
| `status` | reported `running (libvirt)` |
| `shell` | opened a shell in the guest |
| `down` | halted the domain |
| `up` again | booted, skipped provisioning, exit 0 |
| `provision` | re-ran, left the existing clone alone |
| `reset` | exit 1, snapshot not found -- expected, see #6 |
| `scratch probe` | booted `~/vms/scratch/vmtest/probe` |
| `discard probe` | removed the domain and the directory |
| `destroy` (no name) | refused, named the target |
| `destroy jutro` | refused, name did not match the project |
| `destroy vmtest` | removed the domain and `~/vms/vmtest` |

`destroy` left `~/vms/jutro` alone, which is the thing that
most needed checking.

`provision` printed `kozmotic checkout already present,
leaving it untouched`, so the bootstrap's re-run path works
against a clone it made earlier rather than only against an
empty directory.

## What the host did that no dry run showed

**The environment handover reaches the guest.** kozmotic's
provisioning script printed `== vm host: frosti (frosti)`.
The two halves come from `BOMBYX_VM_HOST`, which bombyx sets
on the `vagrant` process, and `BOMBYX_VM_HOSTNAME`, which the
Vagrantfile fills from `hostname -s`. They agree, which is
what proves the value was not expanded on the wrong side.

Those variables reach the provisioner and nothing else. An
interactive `bombyx shell` sees `BOMBYX_VM_HOST` empty,
because Vagrant sets it for the provisioning run rather than
for the guest's login shell.

**A VM survives a failed provision.** The debian-13 attempt
left a booted, running domain behind after `up` returned 1.
`status` reported it, `shell` entered it, and `down` and
`destroy` both worked on it. So a failed `up` is not a
half-made machine that needs unpicking by hand.

**The guest gets the box's own disk, whatever that is.**
debian-13 gave a 9.7 GB root. ubuntu2204 gave a 128 GiB
virtual disk of which the root logical volume is 63 GB, with
another 63 GB unallocated in the volume group. The disk is
sparse and shares the box image as a backing store, so the
running VM cost about 5.4 GiB on the laptop. bombyx writes
`cpus` and `memory` into the Vagrantfile and nothing else.
Captured as `vm-disk-size-unset`.

**libvirt names a domain after the directory, not the
project.** `~/vms/jutro` gave `jutro_default`, `~/vms/vmtest`
gave `vmtest_default`, and `~/vms/scratch/vmtest/probe` gave
`probe_default`. The project name is nowhere in that last
one, which is what `scratch-domain-name-collides` is about.

**`discard` leaves the per-project scratch directory.** After
discarding `probe`, `~/vms/scratch/vmtest` remained as an
empty directory. It is harmless and it is not what
`discard-leaves-dir` was about, so it is recorded here rather
than captured.

## The paths the recent work added

**`--config <path>`** read a registry outside the config
directory and used it.

**A project entry's own `host` wins, and says so.** With a
second registry whose top-level `host` was
`some-other-vmhost` and whose project entry said
`host = "FROSTI"`, bombyx printed:

```
bombyx: host FROSTI from [projects."vmtest"].host in
        /.../alt-registry.toml
```

The notice names the table and the file it came from. The
spelling also confirms the match ignores case: `FROSTI` was
taken for this machine.

That pair was exercised with `--dry-run`, so what is proven
is which host wins and what bombyx says about it, not a
second real boot.

**Three error paths**, each exiting 1 with a message that
names what to do:

```
bombyx: no `[projects."nosuch"]` in
        /home/igor/.config/bombyx/config.toml -- add that
        table with ...
bombyx: no registry file -- create /.../nope.toml with a
        `[projects."vmtest"]` table
bombyx: --project is required: name the `[projects.<name>]`
        table this command is about
```

## What was not verified

- **Hyper-V.** frosti has libvirt only. The Hyper-V branch of
  the Vagrantfile has still never started a machine.
- **A real remote host.** Everything here went through the
  local route, so `ssh` carried nothing. The ssh route was
  last exercised by `first-real-run`, against the old tool.
- **A project entry's `host` naming a different machine.**
  Only the local-route spelling was run for real; the
  precedence and the notice were checked with `--dry-run`.
- **The scratch domain-name collision.** Three domains show
  that the name comes from the directory basename. A second
  project's `probe` was not booted, so the collision is
  reasoned rather than observed.
- **A private repository.** kozmotic is public, so the guest
  needed no credential and the trust-boundary question that
  `docs/trust-boundary.md` raises was not touched.
- **`reset` succeeding.** No command creates the
  `fresh-install` snapshot, which is #6.

## Defects captured, not fixed

- `vm-disk-size-unset`
- `box-must-carry-git`
- `scratch-domain-name-collides`
