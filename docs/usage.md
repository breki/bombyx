# Using bombyx

This is the command reference. It assumes bombyx is installed
and a `bombyx.toml` exists in the project you are working in --
[../README.md](../README.md) covers both, and its **Use**
section is the short version of this page. If none of that is
set up yet, start with [tutorial.md](tutorial.md), which builds
a working project from nothing.

The examples all use the config from the README: a host alias
of `vmhost` and a project named `myproject`.

- [Commands](#commands)
- [Checking a host with doctor](#checking-a-host-with-doctor)
- [Seeing what would run: --dry-run](#seeing-what-would-run---dry-run)
- [How the generated files are written](#how-the-generated-files-are-written)
- [bombyx.toml is untrusted input](#bombyxtoml-is-untrusted-input)

## Commands

```bash
bombyx doctor             # check the preconditions, change nothing
bombyx up                 # write the generated files, boot the VM
bombyx provision          # re-run provisioning in the guest
bombyx shell              # open a shell inside the VM
bombyx status             # vagrant status on the host
bombyx reset              # restore the fresh-install snapshot
bombyx down               # halt the VM
bombyx destroy myproject  # destroy the VM and remove its dir

bombyx scratch pr-1234    # boot a throwaway VM
bombyx discard pr-1234    # destroy it
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
either.** A
forced checkout of `FETCH_HEAD` detaches HEAD, so a commit the
agent makes afterwards sits on no branch, and the next
`provision` moves HEAD away from it. `git log` stops showing it
and only `git reflog` can find it. Push the work out to survive
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

### Why `destroy` asks for the project name

`destroy` takes the project name as confirmation and refuses if
it does not match `project` in `bombyx.toml`. `down` halts a VM
and `reset` rolls it back; `destroy` throws away the warm caches
and installed tooling that make a persistent VM worth keeping,
so it asks for a deliberate act rather than a flag.

**Read the target it prints, not the name you typed.** Both the
refusal and the confirmation print the resolved
`<host>:<directory>`:

```console
$ bombyx destroy
bombyx: destroy needs the project name to confirm: run
`bombyx destroy myproject` -- target is vmhost:~/vms/myproject
```

The name on its own proves less than it appears to, because
`project` comes from the same `bombyx.toml` that decides which
directory is deleted -- a repo you cloned can name itself after
a VM you care about. The printed target is the part you can
check against reality.

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

Setting `provider = "hyperv"` does not get you a Hyper-V VM
today. bombyx writes the settings block for it but never tells
vagrant which provider to use, so vagrant picks whatever the
host offers -- on a libvirt host, a libvirt VM at vagrant's
default size. That is `provider-configured-not-selected` in
`docs/todo.md`.

It runs every check rather than stopping at the first failure,
and exits non-zero if any fails. It **creates, deletes and
modifies nothing** — with one honest exception worth naming: the
provider check runs `vagrant plugin list`, and on a host where
vagrant has never run as that user, vagrant itself creates
`~/.vagrant.d`. bombyx disables vagrant's version-checkpoint call
so the probe neither writes more than that nor stalls on a
firewalled endpoint. When SSH itself fails the remaining host checks are
skipped rather than each waiting on a dead host. Locally it does
execute `ssh` to read its version, so it is not a no-op on your
workstation.

`ssh` is the only local program `doctor` checks, because it is
the only one a VM command runs. `bombyx self-update` also needs
`git`, `curl` and `tar`, and `doctor` deliberately says nothing
about those: a row that fails for a tool no VM command runs
teaches operators to ignore the exit code.

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

The local line names the directory `ssh` came from. bombyx
resolves it against `PATH` explicitly rather than leaving it to
the operating system, which on Windows
searches the working directory first — and bombyx runs inside a
repository whose contents arrive with whatever branch you
checked out.

Every command resolves what it needs the same way, all of it
before running any step. So a missing `ssh` stops `up` before it
has created the directory on the host, rather than after.

See [vm-host-setup.md](vm-host-setup.md) for what to do about
each failure; `doctor` reports facts and leaves the remedies to
the guide.

## Seeing what would run: `--dry-run`

Every command accepts `--dry-run`, which prints the exact `ssh`
invocation instead of running it:

```console
$ bombyx --dry-run up
ssh vmhost "mkdir -p ~/'vms/myproject'"
ssh vmhost "cat > ~/'vms/myproject/Vagrantfile' <<'BOMBYX_EOF' (33 lines elided)
ssh vmhost "cat > ~/'vms/myproject/bootstrap.sh' <<'BOMBYX_EOF' (265 lines elided)
ssh vmhost "cd ~/'vms/myproject' && BOMBYX_VM_HOST='vmhost' BOMBYX_VM_HOSTNAME=\$(hostname -s) vagrant 'up'"
```

Each generated file prints as one line naming its heredoc and
how many lines were dropped. Printing both in full would bury
the four-step plan they belong to; the host receives the whole
content regardless.

The output is real shell: each argument is printed bare only
when it is unambiguous, and quoted otherwise, so what you
read is what runs.

The `\$` in the last line is the escaping doing its job rather
than a stray backslash. `BOMBYX_VM_HOSTNAME` has to be filled in
by the *host's* shell -- it is the host's name the guest wants --
so the substitution is printed escaped, and the line you paste
asks the same machine bombyx would have asked. Unescaped it would
answer with your workstation's name, which is exactly the kind of
wrong answer nobody questions. See
[the README section](../README.md#telling-the-vm-which-host-it-runs-on)
for what the two variables are for.

## How the generated files are written

bombyx sends the Vagrantfile and the bootstrap script over the
same SSH connection it uses for everything else, with a shell
*heredoc*: `cat > <file> <<'BOMBYX_EOF'`, then the file's lines,
then a line holding the delimiter alone. The remote shell reads
everything up to that delimiter as data.

Details that look fussy and are not:

- **The tilde sits outside the quotes** (`~/'vms/myproject'`). A
  POSIX shell does not expand `~` inside single quotes, so
  the obvious `'~/vms/myproject'` would create a directory
  literally named `~`. Quoting only the rest keeps the path
  injection-proof *and* expandable.
- **The delimiter is quoted** (`<<'BOMBYX_EOF'`, not
  `<<BOMBYX_EOF`). An unquoted delimiter makes the shell expand
  `$` and backticks inside the body, so a `$` in the generated
  Vagrantfile would be replaced by the host shell before the
  file was ever written.
- **The delimiter grows if the payload contains it.** A file
  holding a line equal to `BOMBYX_EOF` would end the heredoc
  early and hand the rest to the shell as commands. bombyx
  lengthens the delimiter until no payload line matches it,
  which cannot fail and needs nothing from the caller.

Both files are written on every `up`, `provision` and
`scratch`, so the host's copy cannot drift from what the
configuration currently says.

## `bombyx.toml` is untrusted input

`bombyx.toml` travels inside a repo, so it is treated as
untrusted input. Every field is checked against an allowlist:
`remote_root` must be an anchored path with no traversal, and a
scratch name must be a single path segment, so `../../etc` is
refused rather than quoted.

The field that used to matter most is no longer in the file at
all. `host` is handed to `ssh` as its first argument, and `ssh`
reads a leading `-` as an option, so a repo shipping
`host = "-oProxyCommand=..."` could run code on your
workstation from a bare `bombyx status`. `host` in
`bombyx.toml` is now an error, and the value comes from your
own machine instead -- see **Where bombyx looks for the host**
in `../README.md`.

The charset check on `host` stays, because the remaining
sources can still be wrong: a gitignored `bombyx.local.toml`,
your `config.toml`, `BOMBYX_HOST`, or a mistyped `--host`. It
guards the argv, not one particular file.

`bombyx.local.toml` carries a host, so the charset check
applies to it exactly as it applies to `--host` and
`BOMBYX_HOST`. Every other value comes from `bombyx.toml` and
is validated there.

That file is worth one more paragraph, because it is the
residual exposure in this path. It sits inside the project
directory, only convention keeps it out of git, and it outranks
your own `config.toml`. So a repository that commits one
redirects every `ssh` bombyx runs, `destroy` included, to a
machine of its choosing -- which is the attack `host` was
removed from `bombyx.toml` to prevent. bombyx does not refuse
such a file. What it does is print a line on stderr saying the
host came from a `bombyx.local.toml`, so the redirection is
visible rather than silent. That line names the default
filename rather than the derived one, so under
`--config staging.toml` it says `bombyx.local.toml` while
`staging.local.toml` is the file that supplied the host.
`docs/developer/redteam-log.md` tracks it as
`rt-2026-09-04-provenance-line-names-the-default-filename`.
