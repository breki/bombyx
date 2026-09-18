# Tutorial: from nothing to a working agent VM

This walks through a complete first setup: the workstation you
work on, the VM host that runs the VMs, and a sample project
that describes one VM. At the end you will have an agent VM you
can open a shell into, halt, boot again, and throw away.

Read it in order. Each part checks itself before the next
depends on it, so a failure shows up where it happened, not
three steps later.

> **What this was checked against.**
>
> The workstation steps (Parts 1 and 2) were run on Windows 11 in
> August 2026, including the two failure cases in **When something
> goes wrong**.
>
> **The transcripts in Parts 3 and 4 are written from the code,
> not captured from a run *(unverified)*.** bombyx generating the
> Vagrantfile, `up` as five `ssh` commands, and `doctor` without
> the `tar` and `scp` rows are all current behaviour; the exact
> output is transcribed from the source rather than run against a
> remote VM host. Part 1 installs bombyx from the clone, so your
> binary is built from that same source -- treat your own first
> `bombyx up` as the real test.
>
> **One route has since been exercised end to end.** On
> 2026-09-05, the whole sequence was run on a Linux workstation
> with `host` naming that machine itself -- `doctor`, `up`,
> `status`, `shell`, `down`, `provision`, `scratch`, `discard`
> and `destroy` -- against a guest that booted and provisioned
> to completion. So the local route is verified, guest included.
> **Running bombyx against your own machine** carries the
> detail. The `ssh`-route transcripts below are still written
> from the code.
>
> The VM host steps are a summary of
> [vm-host-setup.md](vm-host-setup.md), which records what it
> was verified against; follow that page for the detail.
>
> The sample `provision.sh` in Part 3 is *(unverified)* as
> written here -- it is assembled from a working setup rather
> than copied from one. bombyx writes the Vagrantfile itself, so
> Part 3 has none to sample. The comments explain what each
> setting is for, so a failure should be diagnosable rather than
> mysterious.

## The three pieces, and why they are separate

```
workstation                     VM host
  bombyx  ──── ssh ────►    vagrant ──► agent VM
     │                                     │
     └── writes Vagrantfile ───────────────┘
         and bootstrap.sh          clones the repo itself

  the project repo:
    vagrant/           the provisioning script the guest runs

  your own machine, outside any repo:
    config.toml        which VM host is yours, and one table
                       per project
```

- **The workstation** is your daily machine. It holds bombyx,
  your SSH config and your `config.toml`. It never runs a VM,
  and it does not need a checkout of the project either.
- **The VM host** is usually a different machine, and that is
  what puts your credentials out of reach: an agent that escapes
  its VM lands somewhere holding none of them. It runs libvirt,
  Vagrant and the VMs.
- **The project** is a repository somewhere the guest can reach.
  The only thing in it that bombyx cares about is a provisioning
  script, and even that bombyx never sends: the guest clones the
  repository itself and runs the script out of its own clone.
- **Every setting** lives in your own `config.toml`, outside any
  repository -- the VM host, and one `[projects.<name>]` table
  per project. Part 1 sets up the file and Part 3 adds a table
  to it.

bombyx ships neither that file nor the `vagrant/` directory for
you. Parts 1 and 3 write both by hand, once.

## Before you start

You need two machines and about an hour, most of it waiting for
packages and the first box download.

| Where | What |
|-------|------|
| Workstation | A Rust toolchain, `git`, `ssh`, `curl`, `tar` |
| VM host | A Linux machine with hardware virtualisation, reachable over SSH |
| Both | Key-based SSH from the workstation to the host, no password |

On Windows, `ssh` comes with the OpenSSH client that ships with
Windows 11, and `tar` comes with either Windows or Git for
Windows. You do not need WSL. `curl` and `tar` are only for
`bombyx self-update`, which fetches and unpacks the release
archive with them; no VM command runs either.

A spare desktop or a home server makes the best VM host,
because a separate machine is what puts your credentials out of
reach: an agent that breaks out of the VM lands somewhere that
holds nothing of yours, and the host firewall rules can keep it
off your LAN as well.

**Your workstation can be the VM host too, and that is a
supported way to run this.** You keep a separate kernel, no host filesystem
mounted into the guest, and no credentials inside the guest. So
an agent that misbehaves, runs a hostile `postinstall`, or acts
on a prompt injection is still contained. The part you lose is
the one that needed two machines: an escaped guest is already
on your workstation, and there is no separate network to
isolate it from. This is a real trade: you give up network
isolation, and in exchange you avoid running a second machine at
all. It needs no special mode -- `host` is an SSH alias, so it
can point at your own machine (see **Running bombyx against your
own machine** below).

## Part 1: the workstation

### Install bombyx

```bash
git clone https://github.com/breki/bombyx
cd bombyx
cargo install --path crates/bombyx
```

Check it landed:

```console
$ bombyx --version
bombyx <version>    # whatever you installed
```

### Give the VM host an SSH alias

bombyx never handles addresses, usernames or keys. It runs `ssh
<alias>`, and everything about how that connects is your
`~/.ssh/config`. Add an entry for the host:

```sshconfig
Host vmhost
    HostName 192.168.1.50
    User igor
    IdentityFile ~/.ssh/id_ed25519
```

The alias is what goes in your own `config.toml`, which the
next section writes. Name it whatever you like; `vmhost` is
used throughout this tutorial.

**If your VM host is this very machine, skip ahead.** On a
Linux workstation with libvirt on it, bombyx runs `vagrant`
here and needs no alias, no key and no SSH server --
**Running bombyx against your own machine** later in Part 2
replaces this step and the `host` line that goes with it.

Otherwise, prove the alias works without a password prompt,
because that is the form bombyx needs:

```console
$ ssh vmhost true
$ echo $?
0
```

If that asks for a password, copy your key over
(`ssh-copy-id vmhost`, or append the public key to
`~/.ssh/authorized_keys` on the host) and try again. Do not
continue until it is silent.

> **Testing one specific key honestly.** `ssh -i key -o
> IdentitiesOnly=yes` does *not* ignore identities named in
> `ssh_config`, so it can silently authenticate with a different
> key on a machine that has other `IdentityFile` entries
> configured. The reported success then belongs to that other
> key, not the one you are testing. Add `-F /dev/null` to
> ignore the config when that is the thing you are testing.

### Name your VM host, once

Because a VM host is never shared the way a project's
repository is, bombyx reads no file out of that repository at
all. A project is shared and a
VM host is not: everyone has their own hardware on their own
network, so a committed value would be wrong for everyone but
its author -- and `bombyx destroy` runs `vagrant destroy` and
`rm -rf` on whatever host is in force.

Write yours once, outside any repo. This is the same file Part 3
adds a project table to, so keep the path:

```bash
# Linux / macOS
mkdir -p ~/.config/bombyx
printf 'host = "vmhost"\n' > ~/.config/bombyx/config.toml
```

```powershell
# Windows
New-Item -ItemType Directory -Force "$env:APPDATA\bombyx" | Out-Null
Set-Content "$env:APPDATA\bombyx\config.toml" 'host = "vmhost"'
```

That covers every project on this machine. A project that runs
somewhere else gets a `host` of its own inside its table, which
Part 3 comes back to, and that key wins for that project alone.
If neither names a host, bombyx stops and says which line to add
rather than guessing.

## Part 2: the VM host

Do this on the host, over SSH or at its console.
[vm-host-setup.md](vm-host-setup.md) is the real reference --
it has the exact commands, the package names that changed in
Ubuntu 24.04, and what to do when a step fails. This is the
shape of it:

1. **QEMU and libvirt**, and your user added to the `libvirt`
   group. Log out and back in for the group to take effect.
2. **Vagrant**, from HashiCorp's repository. Ubuntu 24.04
   removed its own `vagrant` package.
3. **The libvirt provider plugin**:
   `vagrant plugin install vagrant-libvirt`.
4. **Make the default storage pool autostart.** It creates
   itself but does not come back after a reboot, which turns
   into a confusing `vagrant up` failure weeks later.

Then check the one thing that is easy to get wrong and hard to
diagnose. Run this **from the workstation**, not on the host:

```console
$ ssh vmhost vagrant --version
Vagrant 2.4.9
```

That is not the same test as logging in and typing `vagrant
--version`. `ssh host "cmd"` starts a non-interactive shell,
which skips the startup files that usually extend `PATH`. A
Vagrant installed outside the non-interactive `PATH` works
perfectly when you log in and is invisible to bombyx.
[vm-host-setup.md](vm-host-setup.md) explains the mechanism and
the fix under **Why the non-interactive PATH causes trouble**.

### Running bombyx against your own machine

*Verified on a Linux workstation (Ubuntu, vagrant 2.4.9,
vagrant-libvirt 0.12.2) on 2026-09-05: every VM command ran
through `sh -c`, the two generated files arrived intact, and a
guest booted on this route and provisioned to completion.*

**Read this before you do Part 2, or come back and redo it.**
This section replaces the SSH alias you wrote in Part 1 and the
`host` line that names it. The checks that go with them --
`ssh vmhost true` in Part 1, and `ssh vmhost vagrant --version`
above -- do not apply to you either, because bombyx will not be
using `ssh` at all.

**It needs a Linux workstation**, and that is the one thing
that decides whether you can use it. bombyx's VM host has to
run libvirt, and libvirt does not run on Windows or macOS. If
your workstation is Windows, your options are a Linux VM or a
WSL2 distribution with nested virtualization acting as the host
-- see [vm-host-wsl2.md](vm-host-wsl2.md), which is verified
end to end -- and bombyx will use `ssh` to reach either. It
refuses the local route on Windows outright, so there is
nothing to configure wrongly.

With that settled: write your own machine's name as `host` and
bombyx does the rest. As it reads `config.toml` it compares
`host` against this machine's name, and when the two match it
runs each command here through `sh -c` instead of handing it to
`ssh`. There is
no SSH server to install, no key to authorize to your own
account and no loopback alias to write.

```toml
host = "nimbus"     # this machine, so no ssh hop
```

**Write the name exactly.** The comparison ignores case and
nothing else, so `host` has to be what your machine calls
itself, character for character. Run `hostname` and copy what
it prints. A domain counts: on a machine answering
`nimbus.lan`, `host = "nimbus"` gets you the SSH route.

That strictness is on purpose. A bare label is easy to share --
plenty of machines are called `ubuntu`, `vagrant` or `build01`
-- and the domain is the part that says which one you mean.
Matching on the label alone risks two things: bombyx starts a
guest on your workstation while you believe it is on the
isolated host, and teardown later deletes the workstation's
directory.
Getting it wrong the other way just gives you the SSH route,
which you will notice immediately.

**bombyx never reads your `~/.ssh/config`.** It checks the name
you wrote against the machine's own name, no more. Usually that
is what you want:
write `host = "selfhost"` with `selfhost` aliased to
`127.0.0.1` and you get the SSH route, because you asked for it
by name. The exception: if an SSH alias has exactly your
machine's name but points elsewhere, bombyx matches on the name
and takes the local route anyway. Write
that one as `you@name` and the SSH route is forced, because the
`you@` makes the two names differ.

On Windows the local route is never taken, whatever the names
say. A Windows machine cannot run libvirt, so the local route
there could only ever be a mistake -- and a quiet one, since
Git for Windows supplies an `sh` for it to run.

You can tell which route is in force. bombyx prints a line on
stderr whenever it is running here, and `bombyx doctor` reads
differently in two ways. In the first row, bombyx prints `sh` rather
than `ssh`, because that is the program it will actually start.
And two host rows come back as skips rather than passes:
`ssh`, which is not used, and `login shell`, because bombyx
starts `sh` itself rather than asking your login shell to
interpret anything. The `doctor` transcript further down this
document is an `ssh`-route run, so it shows neither. That notice is
worth reading rather than tuning out: **Before you start**
above says what you give up by putting the guest on the same
machine you work on, and the local route is what makes that
arrangement easy to reach by accident.

Everything else about bombyx stays the same. bombyx still
writes the generated files and still runs `vagrant`, and the
script it builds is identical on both routes -- `sh -c` is the
same POSIX shell `ssh` would have started on a remote host.

On the `ssh` route the host's login shell has to be POSIX,
because bombyx sends `mkdir -p` and `cat > file` for the far
side to interpret. On Linux that is already true. On Windows,
OpenSSH Server starts `cmd.exe` and those commands fail, and
the fix is the `DefaultShell` registry value. That is what
`doctor`'s `login shell` row checks. The local route asks
nothing of your login shell, because bombyx starts `sh`
itself.

One more thing about Windows, since the paragraph above sent
you elsewhere. Hyper-V is the other way to run VMs there, and
bombyx accepts it as a `provider` value -- `libvirt` and
`hyperv` are the two it takes, and VirtualBox is not one of
them. It does not give you the local route, though, and it
comes with a caveat of its own: its provider needs an elevated
shell, which an SSH session does not have. Because bombyx
passes the provider through to vagrant, setting `hyperv` where
it is not available fails the boot rather than quietly falling
back to libvirt. This protection applies only before the VM
exists. Once vagrant creates a machine, it records the provider
and reads that record back later, so switching providers
afterward requires a `bombyx destroy` first. `bombyx destroy` does not pass a
provider at all, so it can remove the directory even after a
boot failed on a provider mismatch. That refusal was tested on a Linux
host and works there. But whether a Windows VM host then boots
the machine is *(unverified)*: nobody has run bombyx against
one.

### Optional: keep the VM from reaching your home network

By default a libvirt guest can reach everything the host can --
your LAN, your router, the host's own SSH port. If the VM is
going to run code you do not trust, that is worth closing.
`scripts/agent-vm-firewall.sh` in this repo loads an nftables
ruleset that allows outbound internet and refuses private
destinations, and **Keeping agent VMs off your home network** in
`vm-host-firewall.md` explains what it does and does not buy.
That page is marked unverified, so read it before applying it.

You can skip this and come back to it. The rest of the tutorial
does not depend on it.

## Part 3: the sample project

This part is done on the workstation, inside whatever repo you
want a VM for.

**It has to be a real repository, pushed somewhere the guest can
reach.** bombyx sends no project file anywhere: the VM clones
`source.repo` at `source.ref` itself and runs `source.script`
out of that clone. A directory that was never pushed leaves the
guest failing at clone time, which is late and confusing.

So an empty repository will not do either. By the end of this
part the repository has to hold `vagrant/provision.sh`, on the
branch you name in `ref`, pushed. Part 3 writes that file and
ends with the step that pushes it.

This tutorial uses a public repository, so the guest clones
with no credential of its own. A private one needs a
credential inside the VM: name a deploy key on the VM host
with `deploy_key` in `[source]`, and `vagrant` uploads it into
the guest before provisioning. Code in the VM can read that
key -- see [trust-boundary.md](trust-boundary.md) for what
that costs.

A project's own secrets go the other way. `env_file` in
`[source]` names a file on the machine you are typing on,
usually the project's untracked `.env`, and bombyx carries it
into the guest. Your provisioning script copies it into place
from `$BOMBYX_ENV_FILE`. The sample config explains it in full.

`repo_token` and `repo_user` beside it clone a private
repository over `https` instead, authenticating with a token
that lives in that same secrets file. On Bitbucket that is the
only arrangement an agent can push with, because an ssh access
key there is read-only. The sample config explains both.

The layout, in two places:

```
myproject/                  your project repo
  .gitignore
  vagrant/              the guest runs this from its own clone
    provision.sh

~/.config/bombyx/
  config.toml           the host from Part 1, plus the project
                        table this part adds
```

### The project's table in `config.toml`

Open `config.toml.sample`. It is at the root of the bombyx
clone you made in Part 1, and also at
<https://github.com/breki/bombyx/blob/main/config.toml.sample>.
Its comments explain every key. A test loads that file as
shipped, so the sample cannot silently stop parsing. The sample
has failed to load twice in the past, which is why the test was
added.

Copy the `[projects.myproject]` block out of it and append it to
the `config.toml` you wrote in Part 1, below the `host` line.
Then change these:

| Key | This tutorial uses |
|-----|--------------------|
| the table key | `myproject` -- names the VM and its directory on the host |
| `vm.box` | `generic/ubuntu2204` -- it carries `git`; see below |
| `source.repo` | the URL you push this repository to |
| `source.ref` | the branch you push, `main` here |

**Pick a box that carries `git`.** `generic/ubuntu2204` does,
and it is the value in `config.toml.sample`. A box without it --
`debian/bookworm64`, say -- cannot finish the first `up`: the
guest boots, then the provisioner refuses at clone time and
exits 1. Your own `provision.sh` cannot rescue it, because `git`
is what would fetch that file in the first place.

**A GitHub or Bitbucket URL over ssh needs `curl` in the box,
and `jq` as well for GitHub.** Before it clones, bombyx has the
guest fetch that host's published ssh keys, so it can tell the
real server from an impostor rather than trusting whatever
answers on port 22. `docs/trust-boundary.md` explains why. The
fetch runs `curl` for either host. Reading GitHub's answer needs
`jq` on top of that, because GitHub publishes its keys as JSON
while Bitbucket publishes finished `known_hosts` lines, so a
Bitbucket clone asks for no `jq` at all. A box missing a program
it needs is refused by name, the same way a missing `git` is:

```
bombyx: jq is not installed in this box, and bombyx needs it to read github.com's published ssh host keys. Install jq in the box, or choose one that has it.
```

A second `bombyx:` line follows it, the same one the `git`
passage above shows.

*(unverified)* We have not booted a guest to find out which of
these boxes carries `jq`. Ubuntu and Debian cloud images
generally do not, so expect to install it -- and note that your
own `provision.sh` cannot do it, for the reason the `git`
passage above gives. An `https://` URL needs neither program,
because it opens no ssh connection at all.

Two later passages were written for the Debian box and will
not match what you have, which is why they still mention it.
The `provision.sh` below runs `chsh` because the Debian box
gives its user `/bin/sh`; on `generic/ubuntu2204` that user
already has `/bin/bash`, so the line does nothing and you can
leave it in. And the arrow-key entry in
**When something goes wrong** describes the same Debian
behaviour, so it will not happen to you.

Keeping the Debian box means installing `git` into it and
repackaging it, which this tutorial does not cover.

The table key is the project name, so nothing inside the table
repeats it. `--project myproject` on every command is what picks
this table: bombyx opens no file in the project's directory, so
it cannot work out which project you mean from where you are
standing.

Leave `provider = "libvirt"`. Deleting the line gets you the
same thing, since libvirt is what bombyx assumes when the key
is absent.

Leave `remote_root` where the sample puts it, above
`[projects.myproject.vm]`. A bare key belongs to the table
header above it, so written below that header this one would
parse as `projects.myproject.vm.remote_root` and the whole file
would be refused.

`[vm]` and `[source]` are required, and every key in them
except `provider`, `deploy_key`, `env_file`, `repo_token` and
`repo_user` is required too.
bombyx builds the VM from `[vm]` and the guest clones the
repository named in `[source]`, so there is nothing sensible for
bombyx to guess: a base image is a choice, and a repository
bombyx invented would be cloned into the guest and have its
script run there.

`remote_root` is optional, shown with its default.

If this one project runs on a different machine from your usual
one, add a `host` line inside its table, above the two tables.
It wins for this project, and bombyx prints a line on stderr
saying so on every command -- because `destroy` runs `rm -rf`
on whichever host wins.

### `.gitignore`

```gitignore
vagrant/.vagrant/
```

`vagrant/.vagrant/` holds a VM's identity, written by `vagrant`
if you ever run it in this directory yourself. bombyx never
reads or sends it; ignoring it stops a stale copy from entering
the repo and confusing the next reader.

### The Vagrantfile: bombyx writes it

You do not write one. bombyx renders the Vagrantfile from
`[vm]` and writes it onto the VM host on every `up`, `provision`
and `scratch`, together with a small bootstrap script.

This is not a convenience. Vagrant reads the Vagrantfile before
the VM exists, so a project-supplied one has to sit on a
machine outside the guest -- and keeping project code off those
machines is the whole point.
[trust-boundary.md](trust-boundary.md) records the reasoning.

Two things the generated file does that are worth knowing:

- **It disables the default `/vagrant` share.** Vagrant would
  otherwise mount the VM host's copy of that directory into the
  guest. There is no project code in it to leak now, but the
  mount also *hangs* on a host whose firewall drops
  guest-initiated traffic, rather than failing clearly.
- **It forwards the VM-host identity.** `BOMBYX_VM_HOST` and
  `BOMBYX_VM_HOSTNAME` reach your provisioning script as
  environment variables, so it can record which machine the VM
  is running on. See "Telling the VM which host it runs on" in
  [../README.md](../README.md).

Neither bombyx nor Vagrant reads a `Vagrantfile` you commit in
`vagrant/`. bombyx does not send it, and the guest's own clone
is not what Vagrant boots from. Delete it rather than
maintaining it.

The guest clones `[source]` itself and runs the script named
there, which is the file the next section covers.

### `vagrant/provision.sh`

Three facts about how this script runs, because they decide how
you write it.

It runs as `vagrant`, the guest's ordinary user and the account
the agent works as. So anything it installs into a home
directory lands where the agent will find it. Running it as
root instead would put a toolchain in `/root`, which is the
mistake this arrangement exists to avoid.

`sudo` is available for the steps that do need root, which is
why every privileged line in the example below has it.

Its working directory is the clone, at `~/project` in that
user's home. `bombyx shell` leaves you one directory above it,
in that home -- confirmed against a real VM rather than
inferred. The clone is the only copy of your code in the VM.

Write the script to be **re-runnable**. `bombyx provision` runs
it again on an existing VM, so every step should either be
idempotent or check before acting.

```bash
#!/usr/bin/env bash
set -euo pipefail

sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential ca-certificates curl git jq ripgrep tmux

# Some boxes give the vagrant user /bin/sh (dash), which has no
# line editing, so arrow keys print `^[[A` inside `bombyx shell`
# and the prompt is a bare `$ ` instead of bash's
# `user@host:dir$`. Switch to bash when that is the case; on a
# box that already uses bash this check does nothing.
if [ "$(getent passwd vagrant | cut -d: -f7)" != "/bin/bash" ]; then
  sudo chsh -s /bin/bash vagrant
fi

# Swap space keeps a big build from getting OOM-killed. `swapon` lives
# in /sbin, which is not on the non-interactive PATH -- calling
# it through `sudo` is what makes it resolve, because sudo runs
# with root's PATH. The same applies to `ldconfig` and most
# other /sbin tools.
if [ ! -f /swapfile ]; then
  sudo fallocate -l 4G /swapfile
  sudo chmod 600 /swapfile
  sudo mkswap /swapfile
  echo '/swapfile none swap sw 0 0' | sudo tee -a /etc/fstab
fi
sudo swapon --all

# Record which machine this VM is running on. (The hand-off was
# seen working in a guest on 2026-09-05; see **What this was
# checked against** in the header.) The guest cannot work that
# out for itself: `hostname` here answers `myproject`, and the
# guest's DMI describes the emulated machine (`QEMU`), not the
# host -- the guest has no way, at any privilege level, to read
# which host it runs on. The
# two variables reach this script because the Vagrantfile
# above passes them in; bombyx put them on the `vagrant`
# process out on the host. A VM booted by a bare `vagrant up`
# sees neither, which is what the defaults are for.
sudo mkdir -p /etc/bombyx
printf 'host=%s\nhostname=%s\n' \
  "${BOMBYX_VM_HOST:-unknown}" "${BOMBYX_VM_HOSTNAME:-unknown}" \
  | sudo tee /etc/bombyx/vm-host > /dev/null

echo "provisioning done"
```

Add whatever the agent needs on top -- a language toolchain, an
agent CLI. Two rules worth keeping:

- **Do not write credentials into this file.** Git tracks it,
  and the guest clones it into a machine you are treating as
  expendable. Pass a token in at the moment you need it
  instead, from inside `bombyx shell`.
- **Make every step idempotent.** See above; `provision` re-runs it.

### Push it, or the guest has nothing to clone

This is the step the rest of the tutorial depends on. The VM
does not read your working copy -- it clones `source.repo` at
`source.ref` and runs `source.script` from that clone. Until
these files are on the branch `ref` names, `bombyx up` boots a
machine that fails inside the guest.

This assumes the project is already a git repository with a
remote. If it is not, make an empty repository on the host of
your choice first, then:

```bash
git init
git branch -M main
git remote add origin https://github.com/you/myproject
```

Set `source.repo` to that same URL -- the guest clones what
`repo` names, not whatever `origin` happens to be. Then:

```bash
git add .gitignore vagrant/provision.sh
git commit -m "add the provisioning script the guest runs"
git push origin main          # the branch named in source.ref
```

Only the provisioning script goes in the repository. The VM
description stays in your own `config.toml`, so the next person
who wants the same VM copies that table rather than cloning it.

## Part 4: the first boot

### Check the preconditions

From any directory -- bombyx reads nothing out of the project's,
so where you stand makes no difference:

```bash
bombyx --project myproject doctor
```

`doctor` changes nothing and runs every check rather than
stopping at the first failure, so one run tells you everything
that is wrong. You want every row `ok`; fix anything that is not
before continuing, because `up` creates a directory on the host
and writes two files before it runs `vagrant`, so a missing piece
otherwise surfaces half-way through. [usage.md](usage.md) under
**Checking a host with doctor** has a sample run and how to read
each line. `ssh` is the only local program checked, because it is
the only one a VM command runs.

### Look at what `up` would do

Every bombyx command takes `--dry-run`, which prints the exact
shell it would run and touches nothing:

```console
$ bombyx --project myproject --dry-run up
```

`up` is five `ssh` commands, and bombyx runs nothing on your
workstation: it makes a directory on the host, writes the two
files it generates -- the Vagrantfile and `bootstrap.sh` -- down a
pipe so their contents never appear as command arguments, boots
with `vagrant up`, and takes a `fresh-install` snapshot so
`bombyx reset` has a state to return to. A project that sets
`env_file` or `repo_token` gets a sixth or seventh command staging
that file.

**Seeing what would run** in [usage.md](usage.md) walks the plan
line by line: why every line clears the five `VAGRANT_*` variables
first, why the two `BOMBYX_VM_*` variables carry a `$` for the
host's shell to fill in, and why you must never pipe the plan into
a shell (under `dash` it boots against an empty Vagrantfile with a
zero exit). The two file sizes it reports change with almost every
release, so run the command to see the figures for the version you
have.

`--dry-run` is real shell and touches nothing, so it is worth
using whenever you are unsure what a command is about to do,
especially `destroy`.

### Boot it

**Every command from here on takes `--project myproject`.** The
examples leave it out so the line under discussion stays
readable; typed without it, bombyx stops and says the argument
is required.

```bash
bombyx --project myproject up
```

The first run downloads the box on the host and takes a while;
later runs take about as long as the VM takes to boot. The
provisioners run only on this first `up`, when the VM is
created.

Then get in:

```bash
bombyx shell
```

That is `ssh -t` through to `vagrant ssh` on the host. If your
arrow keys print `^[[A`, the `chsh` step in `provision.sh` did
not take -- log out and back in, since a shell change applies to
the next login.

### The snapshot that `reset` returns to

`bombyx reset` restores a snapshot named `fresh-install`, and
the `up` you ran a moment ago already took it. That happens on
the first `up` only: every later one finds the snapshot and
leaves it alone, so `fresh-install` goes on describing the VM
as provisioning left it rather than whatever an agent has since
done to it. [usage.md](usage.md) states the rule and why it is
that way, and [architecture.md](architecture.md) shows the
script the host runs to apply it.

That is the state you want to come back to after an agent has
made a mess, which is why it must not be overwritten quietly.

When you do want to move it, ask for it:

```bash
bombyx snapshot
```

That replaces the existing snapshot without asking. `reset`
can no longer take you back to that state -- it is gone. The VM
itself and its caches are untouched.

Two occasions call for it: a VM whose `fresh-install` snapshot
does not record a fresh install, and a machine you have brought
somewhere worth returning to -- a long dependency build finished,
say. [usage.md](usage.md) under **`bombyx snapshot`** covers both.

## Part 5: living with it

The loop you will actually use:

```bash
bombyx --project myproject shell   # work in the VM
bombyx --project myproject down    # halt it when you are done
bombyx --project myproject up      # boot again, fast, caches warm
bombyx --project myproject reset   # roll back to the snapshot
```

When you change `vagrant/provision.sh`, use `provision`, not
`up`:

```bash
bombyx provision
```

`up` provisions a VM only when it first creates one; every later
`up` leaves the old script in place and still reports success,
which makes the gap easy to miss. `provision` re-runs the
bootstrap, so push your change first. The re-checkout is forced
and detaches HEAD, so an agent's uncommitted edits and in-guest
commits do not survive it -- [usage.md](usage.md) under **Why
`provision` is a separate command** has exactly what is kept and
what is lost.

For untrusted code -- an external PR, an unfamiliar dependency
tree -- use a throwaway VM instead of your project one:

```bash
bombyx scratch pr-1234    # boot a fresh VM under that name
bombyx discard pr-1234    # destroy it and remove its directory
```

And to remove the project VM entirely, naming it as
confirmation:

```bash
bombyx destroy myproject
```

`destroy` prints the resolved `<host>:<directory>` it is about to
remove; check that target, not the name you typed.
[usage.md](usage.md) under **Why `destroy` asks for the project
name** explains why the name alone proves little.

## When something goes wrong

`bombyx doctor` is the first move for anything connection- or
tool-shaped. It skips the remaining host checks once SSH itself
fails, rather than making you wait on a dead host for each one:

```console
$ bombyx doctor
  local   ssh               ok    OpenSSH_for_Windows_9.5p2 3.8.2 in C:\Windo...
  vmhost  ssh               FAIL  ssh: Could not resolve hostname vmhost: No ...
  vmhost  login shell       skip  no ssh
  vmhost  vagrant           skip  no ssh
  vmhost  project dir       skip  no ssh
  vmhost  libvirt provider  skip  no ssh
1 check failed, 4 skipped
```

That is what a missing or misspelled `Host` entry in
`~/.ssh/config` looks like.

The failures you are most likely to hit, and where each one is
dealt with:

- **`Could not resolve hostname`** -- there is no `Host` entry
  for the alias in `~/.ssh/config`, or it is misspelled in your
  `config.toml`. Part 1.
- **SSH asks for a password.** Key auth is not set up, and
  bombyx cannot answer a prompt. Part 1.
- **`vagrant: command not found` over SSH, but it works when
  you log in.** Vagrant is installed outside the
  non-interactive `PATH`. See **Why the non-interactive PATH
  causes trouble** in `vm-host-setup.md`.
- **`doctor` reports `libvirt provider FAIL`.**
  `vagrant-libvirt` is not installed for the user bombyx logs
  in as. Step 3 of `vm-host-setup.md`.
- **`up` fails on the host complaining about a storage pool.**
  The default pool exists but is not set to autostart, so it is
  gone after a reboot. See the storage pool section of
  `vm-host-setup.md`.
- **Edits to `provision.sh` appear to do nothing.** `up` skips
  provisioners once the VM exists; use `bombyx provision`.
  Part 5.
- **Arrow keys print `^[[A` inside the VM.** The box's user
  shell is dash, not bash; the `chsh` step in Part 3 switches
  it at the next login.
- **`reset` says the snapshot was not found.** Two causes. The
  VM has no `fresh-install` snapshot because no `up` has saved
  one. Or an `up` tried and could not: that step is advisory, so
  it warns on stderr and lets `up` succeed, and the warning is
  easy to miss several commands later. Either way, run `snapshot`
  and read what it says. Part 4.
- **A mount or a host service hangs rather than failing.** The
  nftables rules drop guest-initiated traffic to the host. See
  **What this does and does not buy** in `vm-host-firewall.md`.

One habit worth borrowing: when you check whether a bombyx
command succeeded, do not pipe it through `tee` or `tail`. A
shell pipeline reports only its last command's status, so a
failed run reads as a pass. Redirect to a file instead
(`bombyx provision > run.log 2>&1`) and print `$?`.

## Where to go next

- [../README.md](../README.md) -- what bombyx is and the design
  behind it.
- [usage.md](usage.md) -- the full command reference: how the
  generated files are written, what teardown removes, how
  to read `doctor`.
- [vm-host-setup.md](vm-host-setup.md) -- the host in detail,
  including the other distributions.
- [vm-host-firewall.md](vm-host-firewall.md) -- keeping agent
  VMs off your home network with host nftables rules.
