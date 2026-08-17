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
- [How the push works](#how-the-push-works)
- [bombyx.toml is untrusted input](#bombyxtoml-is-untrusted-input)

## Commands

```bash
bombyx doctor             # check the preconditions, change nothing
bombyx up                 # push vagrant/, boot the VM
bombyx provision          # push vagrant/, re-run provisioning
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
editing `vagrant/provision.sh` and running `up` again ships the
new script to the host and never executes it. The push reports
success, which is what makes the gap easy to miss.

`provision` pushes the directory exactly as `up` does, then
runs `vagrant provision` instead of `vagrant up`. The VM has to
exist already: on one that was never booted, `provision`
creates the remote directory and ships the archive before
vagrant reports it has nothing to provision, so run `up` first.
It applies to the project VM only -- a scratch VM is
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
host after destroying the VM. Nothing in that directory is
unique -- bombyx pushed it there, or `vagrant` generated it,
and your repo holds the original. Teardown is re-runnable: a
directory with no Vagrantfile is removed rather than treated as
an error, so an interrupted push cannot leave one stranded.

`remote_root` must be an anchored path at least one directory
deep, with no `.` or `..` segment. bombyx deletes the directory
it derives from it, so a root of `/`, `~` or `~/.` is refused
when the config loads rather than at teardown.

## Checking a host with doctor

Run `bombyx doctor` first on a new host. `up` creates a
directory and ships a tarball before it runs `vagrant`, so
without it a missing piece is reported half-way through:

```console
$ bombyx doctor
  local   tar               ok    tar 1.35 in C:\Program Files\Git\usr\bin
  local   ssh               ok    OpenSSH_for_Windows_9.5p2 3.8.2 in C:\Win...
  local   scp               ok    C:\Windows\System32\OpenSSH
  local   Vagrantfile       ok
  vmhost  ssh               ok
  vmhost  login shell       ok    posix
  vmhost  tar               ok    /usr/bin/tar
  vmhost  scp               ok    /usr/bin/scp
  vmhost  vagrant           ok    /usr/bin/vagrant
  vmhost  project dir       ok    /home/igor (will create /home/igor/vms/myproject)
  vmhost  libvirt provider  ok    vagrant-libvirt (0.12.2, global)
all checks passed
```

It runs every check rather than stopping at the first failure,
and exits non-zero if any fails. It **creates, deletes and
modifies nothing** — with one honest exception worth naming: the
provider check runs `vagrant plugin list`, and on a host where
vagrant has never run as that user, vagrant itself creates
`~/.vagrant.d`. bombyx disables vagrant's version-checkpoint call
so the probe neither writes more than that nor stalls on a
firewalled endpoint. When SSH itself fails the remaining host checks are
skipped rather than each waiting on a dead host. Locally it does
execute `tar` and `ssh` to read their versions, so it is not a
no-op on your workstation.

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

The local lines name the directory each tool came from. bombyx
resolves `tar`, `ssh` and `scp` against `PATH` explicitly rather
than leaving it to the operating system, which on Windows
searches the working directory first — and bombyx runs inside a
repository whose contents arrive with whatever branch you
checked out.

Every command resolves what it needs the same way, all of it
before running any step. So a missing `tar` stops `up` before it
has created the directory on the host, rather than after.

See [vm-host-setup.md](vm-host-setup.md) for what to do about
each failure; `doctor` reports facts and leaves the remedies to
the guide.

## Seeing what would run: `--dry-run`

Every command accepts `--dry-run`, which prints the exact
`ssh`/`scp` invocation instead of running it:

```console
$ bombyx --dry-run up
ssh vmhost "mkdir -p ~/'vms/myproject'"
cd /tmp/.tmpAL8i && tar -czf .bombyx-push-4821-729551000.tar.gz -C /repo/vagrant --exclude=./.vagrant --exclude=./.git .
cd /tmp/.tmpAL8i && scp .bombyx-push-4821-729551000.tar.gz vmhost:.bombyx-push-4821-729551000.tar.gz
ssh vmhost "{ cd ~/'vms/myproject' && tar -xzf ~/'.bombyx-push-4821-729551000.tar.gz'; }; rc=\$?; rm -f ~/'.bombyx-push-4821-729551000.tar.gz'; exit \$rc"
ssh vmhost "cd ~/'vms/myproject' && BOMBYX_VM_HOST='vmhost' BOMBYX_VM_HOSTNAME=\$(hostname -s) vagrant 'up'"
```

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

## How the push works

The push ships a tar archive rather than using `scp -r`,
which copies *into* an existing destination and would nest
the directory one level deeper on every push. Extracting a
tar overwrites in place, so repeated pushes are idempotent.
`rsync` would also work but is not present on a stock
Windows workstation; `tar`, `scp` and `ssh` are.

Details that look fussy and are not:

- **The tilde sits outside the quotes** (`~/'vms/myproject'`). A
  POSIX shell does not expand `~` inside single quotes, so
  the obvious `'~/vms/myproject'` would create a directory
  literally named `~`. Quoting only the rest keeps the path
  injection-proof *and* expandable.
- **The archive keeps a bare name** and `tar`/`scp` run in
  its directory. `scp` reads everything before the first
  colon as a host name, so handing it a Windows path
  (`C:\Users\...`) would make it dial a host called `C`.
- **`.vagrant/` is excluded from the archive.** It holds the
  VM's identity on the host; shipping a local copy would
  orphan the running VM. `.git/` is excluded because there is
  no reason to ship it.
- **Cleanup is unconditional** (`rc=$?; rm -f ...`), so a
  failed extract does not leave a corrupt archive in the
  directory `vagrant up` runs in.

The tradeoff of extracting in place: a file deleted locally
is not removed from the host. Run `vagrant destroy` and
re-push if the remote tree needs pruning.

## `bombyx.toml` is untrusted input

`bombyx.toml` travels inside a repo, so it is treated as
untrusted input. Every field is checked against an allowlist:
`remote_root` must be an anchored path with no traversal,
`vagrant_dir` must stay inside the project, and a scratch name
must be a single path segment, so `../../etc` is refused rather
than quoted.

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

Validation runs after `bombyx.local.toml` is merged in, so a
per-project override is subject to the same checks.
