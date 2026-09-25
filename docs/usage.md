# Using bombyx

This is the command reference for a working setup: bombyx
installed and a `config.toml` with a table for your project.
[../README.md](../README.md) and [tutorial.md](tutorial.md) cover
getting there, and the README's **Use** section is the short
version. For why bombyx behaves the way it does -- the
file-writing design, the config validation rules, the `--dry-run`
shell internals -- see [trust-boundary.md](trust-boundary.md) and
[architecture.md](architecture.md).

The examples use the README's config: a host alias `vmhost` and a
project `myproject`.

**Every command but `list` takes `--project myproject`**, left
out of the examples so the line under discussion stays readable.
bombyx reads nothing out of the project's own directory, so it
cannot work out which project you mean from where you are
standing. `--config <path>` names a registry file other than the
one in your config directory -- point it only at a file you
trust, because a config can name a key to copy into the VM
([trust-boundary.md](trust-boundary.md) explains).

- [Commands](#commands)
- [up and provision](#up-and-provision)
- [reset and snapshot](#reset-and-snapshot)
- [destroy and discard](#destroy-and-discard)
- [list](#list)
- [doctor](#doctor)
- [--dry-run](#--dry-run)

## Commands

```bash
bombyx doctor             # check the preconditions, change nothing
bombyx up                 # write the generated files, boot the VM
bombyx provision          # re-run provisioning in the guest
bombyx shell              # open a shell inside the VM
bombyx status             # vagrant status on the host
bombyx reset              # restore the fresh-install snapshot
bombyx snapshot           # replace the fresh-install snapshot
bombyx down               # halt the VM
bombyx destroy myproject  # destroy the VM and remove its dir

bombyx scratch pr-1234    # boot a throwaway VM
bombyx discard pr-1234    # destroy it

bombyx list               # every registered project and its VM state
```

There are two lifecycles, on purpose:

- **Persistent** (`up`/`down`) for your own projects -- warm
  caches, fast boots, rolled back by snapshot.
- **Ephemeral** (`scratch`/`discard`) for untrusted code --
  external PRs, unfamiliar dependencies. Nothing survives, which
  is the point.

A scratch VM lives in `<remote_root>/scratch/<project>/<name>`, so
the same name in two projects does not collide.

## up and provision

`up` boots the VM: it writes the generated files, boots with
`vagrant up`, and on the *first* boot takes the `fresh-install`
snapshot that `reset` returns to. The box downloads on the first
`up` and later boots are quick. Provisioning runs only on that
first `up`.

Because `up` provisions only when it first creates a VM, a second
`up` after you change your provisioning script boots the existing
machine and reports success without re-running it. Use `provision`
instead:

```bash
bombyx provision
```

`provision` re-runs the bootstrap in the guest, fetching
`[source]` again at your configured ref. The checkout is forced,
so **push your work first**: it overwrites edits to tracked files,
and a commit made inside the guest does not survive it. Untracked
files -- the agent's work -- are kept, except where the fetched
commit adds a file at the same path. Changing `source.repo` to a
different repository discards the clone and starts over.

`provision` needs a VM that already exists, so run `up` first, and
it targets the project VM only; for a scratch VM the answer is
`discard` then `scratch`.

## reset and snapshot

`reset` rolls the project VM back to the `fresh-install` snapshot.
`up` takes that snapshot on the first boot only, after
provisioning finishes, and never overwrites it -- so it keeps
pointing at the clean install rather than whatever an agent has
done since.

When a snapshot cannot be taken -- a provider without snapshot
support, for one -- `up` warns on stderr and still succeeds. So a
`reset` that finds nothing to restore usually means that warning
went by unread.

To move the return point on purpose:

```bash
bombyx snapshot
```

That replaces `fresh-install` without asking; the old return point
is gone, but the VM, its disk and its caches are untouched. Run it
when the snapshot no longer records a clean install, or when you
have reached a state worth returning to, such as after a long
dependency build.

## destroy and discard

`destroy` throws away the VM and its directory, and asks for the
project name as confirmation:

```bash
bombyx destroy myproject
```

**Read the target it prints, not the name you typed.** Both the
refusal and the confirmation print the resolved
`<host>:<directory>`:

```console
$ bombyx --project myproject destroy
bombyx: destroy needs the project name to confirm: re-run the
same command with "myproject" as its last argument -- target is
vmhost:~/vms/myproject
```

That printed target is the part you can check against reality;
repeating the name only proves you can read your own command line.

`discard` does the same for a scratch VM. Both remove the VM's
directory after destroying the VM, and both are re-runnable, so an
interrupted `up` leaves nothing stranded.

## list

`bombyx list` reports every project in your config and its VM
state:

```console
$ bombyx list
NAME        HOST             BOX                 CPUS    MEM  STATE
faraway     offsite.invalid  generic/ubuntu2204     4   8192  unknown
jutro       vmhost           generic/ubuntu2404     8  16384  not created
neverbuilt  vmhost           generic/ubuntu2204     4   8192  not created
vmtest      vmhost           generic/ubuntu2204     2   4096  running
```

Everything but `STATE` comes from your config; `STATE` comes from
`vagrant status` on the machine that owns the project. A few
things to know:

- A machine that cannot be reached leaves its own projects
  `unknown` and does not hold up the others. The reason goes to
  stderr, one line per machine, rather than into the table.
- `not created` means either no VM exists yet, or the project has
  never been `up`-ed.
- Any project left `unknown` makes `list` exit non-zero, so a
  script can tell a complete table from one with gaps.
- `bombyx list --offline` reads only your config and contacts no
  machine; it answers instantly and drops the `STATE` column.
- Scratch VMs are not listed, because no config table names them.

## doctor

Run `bombyx doctor` first on a new host. It changes nothing, runs
every check rather than stopping at the first failure, and exits
non-zero if any fails:

```console
$ bombyx doctor
  local   ssh               ok    OpenSSH_for_Windows_9.5p2 3.8.2 in C:\Win...
  vmhost  ssh               ok
  vmhost  login shell       ok    posix
  vmhost  vagrant           ok    /usr/bin/vagrant
  vmhost  project dir       ok    /home/igor (will create /home/igor/vms/myproject)
  vmhost  libvirt provider  ok    vagrant-libvirt (0.12.2, global)
all checks passed
```

The `libvirt provider` row appears only for a libvirt project; a
Hyper-V project shows a `provider` row reading `skip` instead. See
[vm-host-setup.md](vm-host-setup.md) for what to do about each
failure.

**Changing `provider` on a project that already has a VM does
nothing until you destroy it** -- vagrant records the provider it
built the machine with. Run `bombyx destroy`, then `bombyx up`.

On a machine that is its own VM host, `doctor` checks `sh` rather
than `ssh`; [local-host.md](local-host.md) covers that route.

## --dry-run

Every command takes `--dry-run`, which prints the exact shell it
would run and touches nothing:

```bash
bombyx --dry-run up
```

It is worth using whenever you are unsure what a command is about
to do, `destroy` above all. The plan is for reading, and for
pasting one line at a time.

**Do not pipe the plan into a shell.** `bombyx --dry-run up | sh`
writes the generated files empty, or not at all depending on
the shell, and can leave `vagrant up` running against an empty
Vagrantfile with a zero exit that reads as success. To run the
commands, run bombyx without `--dry-run`.
