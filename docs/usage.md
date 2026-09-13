# Using bombyx

This is the command reference. It assumes bombyx is installed
and your `config.toml` has a table for the project you are
working on -- [../README.md](../README.md) covers both, and its
**Use** section is the short version of this page. If none of
that is set up yet, start with [tutorial.md](tutorial.md), which
builds a working project from nothing.

The examples all use the config from the README: a host alias
of `vmhost` and a project named `myproject`.

**Every command below but `list` takes `--project myproject`**,
left out of the examples so the line under discussion stays
readable. bombyx reads nothing out of the project's own
directory, so it cannot work out which project you mean from
where you are standing. `--config <path>` names a registry file
other than the one in your config directory. `self-update` needs
neither, and `list` is about every project at once rather than
one, so it ignores `--project` and reads only the registry.

- [Commands](#commands)
- [Seeing every project at
  once](#seeing-every-project-at-once)
- [Checking a host with doctor](#checking-a-host-with-doctor)
- [Where the snapshot reset restores comes
  from](#where-the-snapshot-reset-restores-comes-from)
- [Seeing what would run: --dry-run](#seeing-what-would-run---dry-run)
- [How the generated files are written](#how-the-generated-files-are-written)
- [What is checked, and what is
  not](#what-is-checked-and-what-is-not)

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
  caches, fast boots, reset by snapshot.
- **Ephemeral** (`scratch`/`discard`) for untrusted code --
  external PRs, unfamiliar dependencies. Nothing survives,
  which is the point: malware that persists to survive
  credential rotation has nothing to persist to.

A scratch VM lives in `<remote_root>/scratch/<project>/<name>`,
so the same name in two projects does not collide.

### Why `provision` is a separate command

`provision` exists because `up` provisions a VM only when it
first creates one. Every later `vagrant up` skips the
provisioners -- whether the VM was halted or running -- so
committing a change to your provisioning script and running
`up` again leaves the guest running the version it cloned when
it was created. `up` reports success, which is what makes the
gap easy to miss.

`provision` writes the generated files exactly as `up` does,
then runs `vagrant provision` instead of `vagrant up`. That
re-runs the bootstrap in the guest, which fetches `[source]`
again at the ref you configured and runs the script from the
clone the guest already has.

The checkout is forced, so it overwrites your edits to tracked
files, and it overwrites an untracked file as well when the
fetched commit adds one at the same path. An untracked file
survives only where the commit has nothing at that path. There
is deliberately no `git clean`: in an agent VM the untracked
files are the agent's work.

**Committing inside the guest does not save the agent's work
either.** A forced checkout of `FETCH_HEAD` detaches HEAD, so a
commit the agent makes afterwards sits on no branch, and the
next `provision` moves HEAD away from it. `git log` stops
showing it and only `git reflog` can find it. Push the work out to survive
a provision.

**Changing `source.repo` loses everything.** The guest removes
the clone and starts over when the URL names a different
repository. Rewriting the same URL with or without a trailing
`/` or `.git` is not a different repository and keeps the
clone.

The VM has to exist already: on one that was never
booted, `provision` creates the remote directory and writes the
files before vagrant reports it has nothing to provision, so run
`up` first.
`provision` targets the project VM only -- a scratch VM is
disposable, so the answer there is `discard` followed by
`scratch`.

### Where the snapshot `reset` restores comes from

`reset` rolls the project VM back to a snapshot named
`fresh-install`, and `up` is what takes it. The save runs after
the boot, so the snapshot records a machine that has finished
provisioning rather than one part-way through it.

`up` saves it only when the machine does not already hold that
name. That is the whole rule, and it exists because "after `up`
completes" is a known-good moment on the first `up` and on no
later one: every `up` after the first follows whatever an agent
has been doing in the VM. Saving unconditionally would quietly
move the point `reset` returns to, which is the one thing the
snapshot is for.

`up` does not fail when it cannot take the snapshot. The step
warns on stderr and lets the boot stand, because a VM that came
up correctly has not failed. Two machines meet that on every
run: a provider with no snapshot support, and one whose listing
decorates the name so vagrant refuses the unforced save. So a
`reset` that finds nothing to restore may mean that warning went
by unread.

### `bombyx snapshot`: moving the point `reset` returns to

The guard makes the snapshot immovable by accident. To move it
on purpose, ask:

```bash
bombyx --project myproject snapshot
```

That replaces the existing `fresh-install` without asking, and
the state `reset` would have returned to is gone. The VM, its
disk and its caches are untouched -- a snapshot is a return
point, not the machine, which is why it takes no confirmation
argument the way `destroy` does. Both commands still need
`--project`, as every command does.

It is worth running in two situations. The first is a VM you
created before this behaviour existed, and which branch you are
in depends on whether you have run `up` since. If you have, its
`fresh-install` exists and records the moment of that `up`,
which was not a fresh install. If you have not, there is no
snapshot at all. The second is a machine you have brought
somewhere worth returning to -- a long dependency build
finished, a toolchain installed -- which makes a better starting
point than the original one.

### Why `destroy` asks for the project name

`destroy` takes the project name as confirmation and refuses if
it does not match the `--project` being destroyed. `down` halts
a VM and `reset` rolls it back; `destroy` throws away the warm
caches and installed tooling that make a persistent VM worth
keeping, so it asks for a deliberate act rather than a flag.

**Read the target it prints, not the name you typed.** Both the
refusal and the confirmation print the resolved
`<host>:<directory>`:

```console
$ bombyx --project myproject destroy
bombyx: destroy needs the project name to confirm: re-run the
same command with "myproject" as its last argument -- target is
vmhost:~/vms/myproject
```

The name on its own proves less than it appears to: you typed it
into `--project` a moment earlier, so repeating it confirms only
that you can read your own command line. The printed target is
the part you can check against reality.

Whether the positional stays in this shape is still open --
`destroy-confirmation-shape` in [todo.md](todo.md) carries it.

### What teardown removes

Both `destroy` and `discard` remove the VM's directory on the
host after destroying the VM. Every file in that directory is
reproducible: bombyx generated the Vagrantfile and the bootstrap
script and writes them again on the next `up`, and `vagrant`
generated the rest. Teardown is re-runnable -- a directory with
no Vagrantfile is removed rather than treated as an error -- so
an interrupted `up` cannot leave one stranded.

`remote_root` must start with `/` or `~/`, and must name at
least 1 directory below that anchor, with no `.` or `..`
segment. So `/`, `~`, `~/` and `~/.` are all refused. bombyx
deletes the directory it derives from this value, which is why
the check runs when the config loads rather than at teardown.

## Seeing every project at once

Every other bombyx command is about one project, named with
`--project`. `bombyx list` is about all of them. It reads your
config file, asks each machine named in it what its projects
are doing, and prints one row per project:

```console
$ bombyx list
NAME        HOST             BOX                 CPUS    MEM  STATE
faraway     offsite.invalid  generic/ubuntu2204     4   8192  unknown
jutro       frosti           generic/ubuntu2404     8  16384  not created
neverbuilt  frosti           generic/ubuntu2204     4   8192  not created
vmtest      frosti           generic/ubuntu2204     2   4096  running
```

That is the whole of stdout. Rows are sorted by project name,
so one config file always lists in the same order however you
arrange the tables in it.

Everything but `STATE` comes from your config file. `STATE`
comes from `vagrant status` on the machine that owns the
project.

`faraway` reads `unknown` above, and the reason for it goes to
stderr rather than into the table, one line per machine:

```
bombyx: offsite.invalid: ssh: Could not resolve hostname
offsite.invalid: Name or service not known
```

bombyx prints that as a single line; it is wrapped here to fit
this page, and its length is the argument for keeping it out of
the `STATE` column.

### One call per machine, not per project

Several projects usually share a machine, and each `ssh`
invocation pays for its own connection and authentication. So
bombyx groups the projects by the machine that owns them and
sends one command per machine, which asks about every project
on it in turn.

Remember that a project's own `host` key beats the file-wide
one, so which machine owns a project is a question the config
file answers per entry. `faraway` above is on its own machine
for exactly that reason.

### A machine that does not answer

The VM host is often off, asleep or off the network, and a
listing that failed because one machine of four was asleep
would be useless. So a machine that cannot be reached leaves
its own projects `unknown` and costs the other machines
nothing. That last part is why the `ssh` call carries
`ConnectTimeout`, `BatchMode` and the two `ServerAlive`
options, from the same builder `bombyx doctor` uses: without
them a machine that swallows packets would block the machines
after it in the queue for minutes.

The reason goes to stderr rather than into the `STATE` column,
once per machine. It is a sentence from `ssh`, and a column
wide enough to hold one would push every other column off the
screen. Keeping it off stdout also leaves the table clean for
anything you pipe it into.

Any project left `unknown` makes `bombyx list` exit non-zero, so
a script can tell a complete table from one with gaps in it.
That is deliberately wider than "a machine was unreachable": a
machine that answers but cannot run vagrant leaves the same gap,
and a script wants to see both. `--offline` establishes no
states at all, so nothing there can leave a gap -- but it still
reads your config file, and a missing or unparseable one fails
the command as it would any other.

### `not created` means two things, and both are true

A project reads `not created` when vagrant says so about a
machine it has no domain for, and also when the project's
directory on the host holds no `Vagrantfile` at all -- which is
a project bombyx has never run `up` for. In both cases there is
no VM, so the listing says the same thing about both.

### Listing without contacting anything

`bombyx list --offline` reads your config file and asks no
machine anything. It is for a workstation away from the hosts,
and it answers instantly. The `STATE` column is left out rather
than filled with dashes, because a column of dashes would stand
in for a question nobody put.

### What it does not list

Scratch VMs. They live in `<remote_root>/scratch/<project>/<name>`
and no config table names them, so bombyx would have to search
directories on the host and trust the names it found there. The
listing describes what your config file holds.

## Checking a host with doctor

Run `bombyx doctor` first on a new host. `up` creates a
directory and writes two files before it runs `vagrant`, so
without it a missing piece is reported half-way through:

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

The `libvirt provider` row appears only when `[vm] provider` is
`libvirt`. A Hyper-V project gets a `provider` row reading
`skip` instead: Hyper-V has no plugin to grep for, and bombyx
has never driven a Hyper-V host, so there is no honest probe to
send. The row stays in the report rather than vanishing, because
an absent row reads as a check that passed, and the summary line
counts it.

Setting `provider = "hyperv"` still does not get you a Hyper-V
VM on a Linux host, but it now fails rather than substituting.
bombyx passes the provider to vagrant on every project call
but the teardown, so
`bombyx up` stops with `The Hyper-V provider only works on
Windows` instead of quietly building a libvirt VM at vagrant's
default size. `bombyx status` and `bombyx halt` stop the same
way while the machine does not exist yet. `bombyx destroy`
still clears the directory that failed boot left behind,
because the teardown is the one call that names no provider --
were it to name one, vagrant would refuse it too and the
removal behind it would never run.

**Changing `provider` on a project that already has a VM does
nothing until you destroy it.** Vagrant records the provider it
built the machine with and reads that back on every later
command, so a second `bombyx up` boots the machine you already
have and neither vagrant nor bombyx reports the difference. Run
`bombyx destroy` first, then `bombyx up`. That gap is
`provider-change-on-existing-vm` in `docs/todo.md`.

It runs every check rather than stopping at the first failure,
and exits non-zero if any fails. It **creates, deletes and
modifies nothing** — with one honest exception worth naming: the
provider check runs `vagrant plugin list`, and on a host where
vagrant has never run as that user, vagrant itself creates
`~/.vagrant.d`. bombyx disables vagrant's version-checkpoint call
so the probe neither writes more than that nor stalls on a
firewalled endpoint. On the SSH route, when SSH itself fails the remaining host
checks are skipped rather than each waiting on a dead host, and
`ssh` is executed locally to read its version -- so it is not a
no-op on your workstation.

The local route differs in three ways. No check gates the ones
behind it, because there is no host to be unreachable and every
remaining check asks about this machine. `sh` is looked up on the `PATH`
and not run, since `sh` may be `dash`, which has no version
flag to ask. And two rows come back as skips rather than
passes: `ssh`, which is not used, and `login shell`, because
the shell is the `sh` bombyx started rather than whatever your
login shell happens to be. A row that passes whatever the state
of your machine is worse than no row at all.

`doctor` checks one local program, and which one depends on
the route. Over SSH that is `ssh`. When `host` names this very
machine bombyx starts `sh` instead, so `sh` is the row you get
and the `ssh` host row becomes a skip. Either way it is the
program a VM command actually runs. `bombyx self-update` also
needs `git`, `curl` and `tar`, and `doctor` deliberately says
nothing about those: a row that fails for a tool no VM command
runs teaches operators to ignore the exit code.

The `vagrant` line is the one that earns the command: it asks
the **non-interactive** shell, which is the one bombyx gets.
Vagrant installed outside that `PATH` works when you log in and
type it, and is invisible to bombyx — and vagrant cannot report
that itself, because it is not running.

Each check is built to carry a verdict rather than a value,
because a probe that merely reports something passes on the
state it exists to catch. `login shell` makes the host *run* a
POSIX construct instead of printing `$SHELL`; `libvirt
provider` checks vagrant's own exit status and matches an
anchored plugin name, because `vagrant plugin list` exits zero
even with nothing installed.

The local line names the directory that program came from.
bombyx resolves it against `PATH` explicitly rather than
leaving it to the operating system, which on Windows searches
the working directory first — and you run bombyx from wherever
you happen to be standing, which is usually a repository whose
contents arrive with whatever branch you checked out.

Every command resolves what it needs the same way, all of it
before running any step. So a missing `ssh` stops `up` before it
has created the directory on the host, rather than after — and
on the local route the same holds for a missing `sh`.

See [vm-host-setup.md](vm-host-setup.md) for what to do about
each failure; `doctor` reports facts and leaves the remedies to
the guide.

## Seeing what would run: `--dry-run`

Every command accepts `--dry-run`, which prints the exact
invocation instead of running it:

```console
$ bombyx --dry-run up
ssh vmhost "unset VAGRANT_CWD VAGRANT_VAGRANTFILE VAGRANT_DOTFILE_PATH VAGRANT_DEFAULT_PROVIDER VAGRANT_PREFERRED_PROVIDERS; mkdir -p ~/'vms/myproject'"
ssh vmhost "unset VAGRANT_CWD VAGRANT_VAGRANTFILE VAGRANT_DOTFILE_PATH VAGRANT_DEFAULT_PROVIDER VAGRANT_PREFERRED_PROVIDERS; umask 077; cat > ~/'vms/myproject/Vagrantfile' && chmod 600 ~/'vms/myproject/Vagrantfile'"  # N bytes on stdin, not shown
ssh vmhost "unset VAGRANT_CWD VAGRANT_VAGRANTFILE VAGRANT_DOTFILE_PATH VAGRANT_DEFAULT_PROVIDER VAGRANT_PREFERRED_PROVIDERS; umask 077; cat > ~/'vms/myproject/bootstrap.sh' && chmod 600 ~/'vms/myproject/bootstrap.sh'"  # N bytes on stdin, not shown
ssh vmhost "unset VAGRANT_CWD VAGRANT_VAGRANTFILE VAGRANT_DOTFILE_PATH VAGRANT_DEFAULT_PROVIDER VAGRANT_PREFERRED_PROVIDERS; cd ~/'vms/myproject' && BOMBYX_VM_HOST='vmhost' BOMBYX_VM_HOSTNAME=\$(hostname -s) VAGRANT_DEFAULT_PROVIDER='libvirt' vagrant 'up'"
ssh vmhost "unset VAGRANT_CWD VAGRANT_VAGRANTFILE VAGRANT_DOTFILE_PATH VAGRANT_DEFAULT_PROVIDER VAGRANT_PREFERRED_PROVIDERS; cd ~/'vms/myproject' && { names=\$(BOMBYX_VM_HOST='vmhost' BOMBYX_VM_HOSTNAME=\$(hostname -s) VAGRANT_DEFAULT_PROVIDER='libvirt' vagrant 'snapshot' 'list') && if ! printf '%s\\n' \"\$names\" | grep -qx 'fresh-install'; then BOMBYX_VM_HOST='vmhost' BOMBYX_VM_HOSTNAME=\$(hostname -s) VAGRANT_DEFAULT_PROVIDER='libvirt' vagrant 'snapshot' 'save' 'fresh-install'; fi || printf 'bombyx: could not save the fresh-install snapshot for %s; re-run this command with snapshot in place of up\\n' 'myproject' >&2; }"
```

Neither generated file appears in the plan, and there is
nothing to elide: the file is not part of the command. It
travels on the command's standard input, which is a pipe
between two processes rather than text. The trailing comment is
how many bytes bombyx will send down that pipe.

The transcript above writes each count as `N` rather than
quoting one. Both generated files change size with almost every
release, so a figure copied into this document is stale by the
next one. Run the command to see the numbers for your version.

Every line begins with the same `unset`. Five vagrant variables
redirect a command to a different directory, a different
Vagrantfile or a different provider, and a value for any of
them on the VM host would otherwise decide where bombyx's own
commands land. Clearing them first is what makes the `cd` on
each line mean what it says. bombyx writes the project's own
`VAGRANT_DEFAULT_PROVIDER` back in front of each `vagrant` call
but `bombyx destroy`, which is the assignment you can see
further along the line. The teardown is the exception because
naming a provider the host cannot supply would have vagrant
refuse it, and the directory removal runs only afterwards.

The fifth line is the snapshot guard, and it is one command
rather than two: the host's shell runs the listing, tests it and
saves only when `fresh-install` is missing. The `|| printf` at
the end is what keeps a snapshot bombyx cannot take from
failing an `up` whose VM booted correctly -- it warns on stderr
instead.

On a machine that is its own VM host every line reads
`sh -c "..."` instead, carrying the identical script. Which
route is in force is decided by comparing `host` against this
machine's name — **Running bombyx against your own machine** in
[tutorial.md](tutorial.md) has the rule.

The output is real shell: each argument is printed bare only
when it is unambiguous, and quoted otherwise, so what you read
is what runs. The two write lines are the exception, and the
trailing comment on each is what marks it: pasting one runs
`cat` against your own terminal, because the file bombyx would
have sent is not in the line to be pasted.

**Do not feed the plan to a shell.** `bombyx --dry-run up | sh`
and `sh < plan.sh` both go wrong, and neither says so.

The mechanism is the one the write lines depend on. A shell
reading a script from its own standard input passes that same
input to the children it starts, so a child that reads standard
input reads the script. Two children here do: the `cat` on the
local route, and `ssh` itself on the SSH route, which forwards
its standard input to the far side on **every** line of the plan
rather than only on the two writes.

What that costs you depends on the shell, and the difference
matters more than it sounds:

- Under `bash`, the first child swallows the rest of the script
  and the run stops there. On the SSH route nothing is written
  at all; on the local route the Vagrantfile receives the
  remaining plan lines as its contents.
- Under `dash` -- which is `/bin/sh` on Debian and Ubuntu, so it
  is what a plain `| sh` gets there -- the shell has already read
  the whole script, so **every line still runs** while each
  child sees an immediate end of file. Both generated files are
  written empty, and `vagrant up` then runs against an empty
  Vagrantfile. On the SSH route that truncates the files already
  on the VM host.

The `dash` case is the one to know about: the run looks like it
worked, the exit status is zero, and the VM host is left holding
two empty files where its Vagrantfile and bootstrap script were.

*(Checked on Linux with OpenSSH 9.6, `bash` 5.2 and `dash` 0.5.12,
on both routes. The per-shell details are what varies; that
feeding the plan to a shell is wrong does not.)*

The plan is for reading, and for pasting one line at a time. To
run the commands, run bombyx without `--dry-run`.

The `\$` in the last line is the escaping doing its job rather
than a stray backslash. `BOMBYX_VM_HOSTNAME` has to be filled in
by the *host's* shell -- it is the host's name the guest wants
-- so the substitution is printed escaped, and the line you
paste asks the same machine bombyx would have asked. Unescaped
it would answer with your workstation's name, which is exactly
the kind of wrong answer nobody questions. See
[the README section](../README.md#telling-the-vm-which-host-it-runs-on)
for what the two variables are for.

## How the generated files are written

bombyx sends the Vagrantfile and the bootstrap script over the
same SSH connection it uses for everything else. The command is
as short as it looks in the plan above:

```sh
cat > ~/'vms/myproject/Vagrantfile'
```

The file itself is not in that command. bombyx opens a pipe to
the `ssh` process and writes the file into it; `ssh` passes
whatever it reads on its own input through to the remote `cat`,
which redirects it into the file. On a machine that is its own
VM host, `sh -c` receives the same command and the same pipe.

**Why not simply pass the file as an argument.** On any Unix
machine, every logged-in account can list the commands other
accounts are running, arguments included -- that is what `ps`
prints. A file passed as an argument is therefore readable by
anyone with a login on the VM host while the write runs, and by
anyone with a login on your workstation too, since the same
text sits in the `ssh` command line there. Bytes on a pipe
between two processes appear in no such listing.

That matters for the Vagrantfile in particular, because it
carries every value from the project's `[env]` table.

The file on disk is the other half, which is why the command
carries `umask 077` and a `chmod 600`. The umask decides the
mode of a file being created, so the contents never exist at a
readable mode even for an instant; the `chmod` corrects a file
an earlier run left at `0664`, because writing over a file
truncates it without touching its mode. Both generated files
therefore end up readable and writable by you and by nobody
else on the VM host. Its administrator is a different question:
a mode stops other accounts, not root.

The second benefit is that nothing has to be escaped. Bytes on
a pipe are bytes: no shell looks inside them for a `$` to
substitute or an end-word to stop at, so any file at all can be
sent without the caller having checked it first.

One detail in the command still repays a look. **The tilde sits
outside the quotes** (`~/'vms/myproject'`). A POSIX shell does
not expand `~` inside single quotes, so the obvious
`'~/vms/myproject'` would create a directory literally named
`~`. Quoting only the rest keeps the path injection-proof *and*
expandable.

Both files are written on every `up`, `provision` and
`scratch`, so the host's copy cannot drift from what the
configuration currently says.

## What is checked, and what is not

Your `config.toml` is normally your own file, and every field
in it is still checked against an allowlist: `remote_root` must
be an anchored path with no traversal, and a scratch name must
be a single path segment, so `../../etc` is refused rather than
quoted.

The checks are not only there for your typos. `--config <path>`
reads whatever file you name, a repository can commit one, and
`BOMBYX_CONFIG_HOME` needs only to be anchored -- so a
per-directory environment tool can redirect bombyx from inside a
clone. A registry that arrived that way chooses two values
worth naming. `remote_root` is what `destroy` builds its
`rm -rf` from. `deploy_key` names a file on the VM host that
`vagrant` copies into the guest, and nothing restricts which
file -- so a config you did not write can ask for the VM host's
own SSH key and have it delivered into a VM about to run that
project's code. Do not pass `--config` a path inside a
repository you did not write.

Because `deploy_key` is checked here and expanded on a machine
you may not be sitting at, its rules are stricter than they
look. It must be anchored (`/` or `~/`) and name a file below
that anchor, with no `.` or `..` segment, no `//` and no
trailing slash, and `~` only as its first character. Every
character has to be a letter, a digit, `.`, `_`, `-`, `/` or
`~`, so a path with a space in it is refused too.

Before `up`, `provision` or `scratch` creates anything, bombyx
checks on the VM host that the file is there and readable, and
stops with a message naming the expanded path and the host when
it is not. That check runs as the user the VM host logs you in
as, which is the user `vagrant` runs as, so a key it cannot
open is refused here rather than inside Vagrant. The teardown
verbs check nothing, so `destroy` still clears a directory
whose key has since gone.

`env_file` is checked in the same spirit and by different
rules, because bombyx is what opens it. The value names a file
on the machine you are typing on, so bombyx reads it before it
builds the plan and stops with a message naming the path it
looked for when the file is not there. Nothing is created on the
VM host first.

The rules are shorter than `deploy_key`'s for a reason worth
knowing: the path never reaches a shell on either machine.
bombyx opens the file itself and the contents travel on the
command's standard input, so there is nothing to quote. The
value has to start with `~/` or be an absolute path, and a bare
`~` is refused because the home directory is a directory rather
than a file. A relative path is refused because it would
resolve against whatever directory you happened to run bombyx
from. A space or a quote in the file name is accepted.

A config you did not write can point `env_file` at any file
your account can read, and bombyx will deliver it into a VM
about to run that project's code. That is the same hazard
`deploy_key` carries, aimed at your own machine rather than at
the VM host, and the same advice covers both.

`host` gets the sharpest rule, because it is handed to `ssh` as
its first argument and `ssh` reads a leading `-` as an option:
`host = "-oProxyCommand=..."` would run code on your
workstation from a bare `bombyx status`. Every `host` in the
file is checked as the file is read -- the file-wide one and
every project's, not only the one this command wants -- so a bad
value is reported wherever it sits and the error names the line.

`docs/trust-boundary.md` under **What this costs** says what
having that key inside the guest costs, and it is not a small
thing: code in the VM can reach it.

bombyx opens no file inside the project's directory at all, and
that is the property the design turns on. It is a rule about
files rather than about everything a repository can reach: a
per-directory environment tool can still set
`BOMBYX_CONFIG_HOME` from inside a clone. "Which host a command
is about to use" in `../README.md` explains what silence does
and does not promise. Read it before you rely on the
distinction.
