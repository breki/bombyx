# Tutorial: from nothing to a working agent VM

This walks through a complete first setup: the workstation you
work on, the VM host that runs the VMs, and a sample project
that describes one VM. At the end you will have an agent VM you
can open a shell into, halt, boot again, and throw away.

Read it in order. Each part checks its own work before the next
one depends on it, which is the difference between a setup that
works and a setup that fails three steps later for a reason you
can no longer locate.

> **What this was checked against.**
>
> The workstation steps were run on Windows 11 in August 2026,
> against bombyx 0.4.1, including the two failure cases in
> **When something goes wrong**.
>
> **The transcripts in Parts 3 and 4 are not from that run
> (unverified).** They show behaviour that is unreleased at the
> time of writing -- bombyx generating the Vagrantfile, `up` as
> five `ssh` commands, `doctor` without the `tar` and `scp`
> rows -- none of which 0.4.1 could produce. They are written
> from the code rather than captured from a machine. Treat
> your own first `bombyx up` as the real test.
>
> **One route has since been exercised end to end.** On
> 2026-09-05, the whole sequence was run on frosti with `host`
> naming frosti itself -- `doctor`, `up`, `status`, `shell`,
> `down`, `provision`, `scratch`, `discard` and `destroy` --
> against a guest that booted and provisioned to completion.
> So the local route is verified, guest included.
> **Running bombyx against your own machine** carries the
> detail, and
> [the run record](issues/registry-run-against-frosti.md) holds
> what it found. The `ssh`-route transcripts below are still
> written from the code.
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
supported way to run this.** You keep most of what matters -- a
separate kernel, no host filesystem mounted into the guest, no
credentials inside it -- so an agent that merely misbehaves,
runs a hostile `postinstall` or acts on a prompt injection is
still contained. What you give up is the part that depends on
the host being a different machine: a guest that escapes the
hypervisor is already on your workstation, and network
isolation from your own machine is not meaningful. It is a real
trade, not a pointless one, and it needs no special mode --
`host` is an SSH alias, so it can point at your own machine
(see **Running bombyx against your own machine** below).

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
bombyx 0.4.1        # whatever you installed
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
> `ssh_config`, so on a machine with other `IdentityFile`
> entries it can authenticate with a different key and report
> success for one the host has never seen. Add `-F /dev/null` to
> ignore the config when that is the thing you are testing.

### Name your VM host, once

bombyx reads no file out of a project's repository at all, and
the host is the clearest reason why. A project is shared and a
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

*Verified on frosti (Ubuntu, vagrant 2.4.9, vagrant-libvirt
0.12.2) on 2026-09-05: every VM command ran through `sh -c`,
the two generated files arrived intact, and a guest booted on
this route and provisioned to completion.
[The run record](issues/registry-run-against-frosti.md) lists
what ran and what it did not cover.*

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
host = "frosti"     # this machine, so no ssh hop
```

**Write the name exactly.** The comparison ignores case and
nothing else, so `host` has to be what your machine calls
itself, character for character. Run `hostname` and copy what
it prints. A domain counts: on a machine answering
`frosti.lan`, `host = "frosti"` gets you the SSH route.

That strictness is on purpose. A bare label is easy to share --
plenty of machines are called `ubuntu`, `vagrant` or `build01`
-- and the domain is the part that says which one you mean.
Matching on the label alone would let bombyx start a guest on
your workstation while you believed it was on the isolated
host, and then delete the workstation's directory on teardown.
Getting it wrong the other way just gives you the SSH route,
which you will notice immediately.

**bombyx never reads your `~/.ssh/config`.** It compares the
name you wrote and nothing else. Usually that is what you want:
write `host = "selfhost"` with `selfhost` aliased to
`127.0.0.1` and you get the SSH route, because you asked for it
by name. It works against you in one case, and it is the one to
know about: an alias named exactly what your own machine is
named, pointing at a *different* machine, is believed. Write
that one as `you@name` and the SSH route is forced, because the
`you@` makes the two names differ.

On Windows the local route is never taken, whatever the names
say. A Windows machine cannot run libvirt, so the local route
there could only ever be a mistake -- and a quiet one, since
Git for Windows supplies an `sh` for it to run.

You can tell which route is in force. bombyx prints a line on
stderr whenever it is running here, and `bombyx doctor` reads
differently in two ways. Its first row names `sh` rather than
`ssh`, because that is the program bombyx will actually start.
And two host rows come back as skips rather than passes:
`ssh`, which is not used, and `login shell`, because bombyx
starts `sh` itself rather than asking your login shell to
interpret anything. The `doctor` transcript further down this
document is an `ssh`-route run, so it shows neither. That notice is
worth reading rather than tuning out: **Before you start**
above says what you give up by putting the guest on the same
machine you work on, and the local route is what makes that
arrangement easy to reach by accident.

Nothing else changes. bombyx still writes the generated files
and still runs `vagrant`, and the script it builds is
identical on both routes -- `sh -c` is the same POSIX shell
`ssh` would have started on a remote host.

On the `ssh` route the host's login shell has to be POSIX,
because bombyx sends `mkdir -p` and a `cat > file <<'EOF'`
heredoc for the far side to interpret. On Linux that is already
true. On Windows, OpenSSH Server starts `cmd.exe` and those
commands fail, and the fix is the `DefaultShell` registry
value. That is what `doctor`'s `login shell` row checks. The
local route asks nothing of your login shell, because bombyx
starts `sh` itself.

One more thing about Windows, since the paragraph above sent
you elsewhere. Hyper-V is the other way to run VMs there, and
bombyx accepts it as a `provider` value -- `libvirt` and
`hyperv` are the two it takes, and VirtualBox is not one of
them. It does not give you the local route, though, and it
comes with a caveat of its own: its provider needs an elevated
shell, which an SSH session does not have. bombyx does pass the
provider to vagrant, so setting `hyperv` on a host that cannot
supply it fails the boot rather than quietly building a libvirt
machine, as long as the VM does not exist yet -- vagrant
records the provider it built a machine with and reads that
back afterwards, so changing the key later needs a
`bombyx destroy` first. That refusal was run on a Linux host
and it works. Whether a Windows VM host then boots the machine
is *(unverified)*: nobody has run bombyx against one.

### Optional: keep the VM from reaching your home network

By default a libvirt guest can reach everything the host can --
your LAN, your router, the host's own SSH port. If the VM is
going to run code you do not trust, that is worth closing.
`scripts/agent-vm-firewall.sh` in this repo loads an nftables
ruleset that allows outbound internet and refuses private
destinations, and **Keeping agent VMs off your home network** in
`vm-host-setup.md` explains what it does and does not buy. That
section is marked unverified, so read it before applying it.

You can skip this and come back to it. Nothing below depends on
it.

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

The guest clones with no credential of its own, so this tutorial
uses a public repository. A private one needs a credential
inside the VM, and code in the VM can read it -- see
[trust-boundary.md](trust-boundary.md) for what that costs.

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
Its comments explain every key, and a test loads that file as
shipped, so it cannot drift from what bombyx accepts -- which is
worth something, because it has been unloadable twice.

Copy the `[projects.myproject]` block out of it and append it to
the `config.toml` you wrote in Part 1, below the `host` line.
Then change these:

| Key | This tutorial uses |
|-----|--------------------|
| the table key | `myproject` -- names the VM and its directory on the host |
| `vm.box` | `generic/ubuntu2204` -- it carries `git`; see below |
| `source.repo` | the URL you push this repository to |
| `source.ref` | the branch you push, `main` here |

**Do not reach for `debian/bookworm64` here**, which is the
obvious Debian choice and the box two later passages of this
tutorial are written around. It has no `git`, so a first `up`
on it cannot finish. Booting it on frosti on 2026-09-05
confirmed that: the VM comes up, and then the provisioning
prints these two lines and exits 1. Vagrant prefixes each with
`default:`, and the second reaches the terminal as one long
line.

```
bombyx: git is not installed in this box.
bombyx: install it in the box, or choose one with git, so the guest can clone the project.
```

**Your own `provision.sh` cannot save you here**, and this is
the part that surprises people. bombyx runs one provisioner in
the guest, its own bootstrap script, and that script's first
job is to clone your repository. `provision.sh` lives inside
that repository. So the `apt-get install ... git` you are
about to write under **`vagrant/provision.sh`** below never
runs: `git` is what fetches the file that would have installed
it.

`generic/ubuntu2204` carries `git`. A guest booted on it on
2026-09-05 and provisioned to completion, and it is the value
in `config.toml.sample`. Both boots are recorded under
`tutorial-box-lacks-git` in `docs/todo.md`.

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
except `provider` is required too. bombyx builds the VM from
the first and the guest clones the second, so there is nothing
sensible for bombyx to guess: a base image is a choice, and a
repository bombyx invented would be cloned into the guest and
run as root.

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
the VM exists, so a project-supplied one has to sit on a machine
outside the guest -- and keeping project code off those machines
is the whole point. [trust-boundary.md](trust-boundary.md)
records the
reasoning.

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

A `Vagrantfile` committed in `vagrant/` is read by nothing.
bombyx does not send it, and the guest's own clone is not what
Vagrant boots from. Delete it rather than maintaining it.

The guest clones `[source]` itself and runs the script named
there, which is the file the next section covers.

### `vagrant/provision.sh`

Write this to be **re-runnable**. `bombyx provision` runs it
again on an existing VM, so every step should either be
idempotent or check before acting.

```bash
#!/usr/bin/env bash
set -euo pipefail

sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential ca-certificates curl git jq ripgrep tmux

# The Debian box creates its user with /bin/sh, which is dash --
# no line editing at all, so arrow keys print `^[[A` inside
# `bombyx shell`. The tell is a bare `$ ` prompt instead of
# bash's `user@host:dir$`. dash never consults TERM or
# terminfo, which is why checking those comes back clean.
if [ "$(getent passwd vagrant | cut -d: -f7)" != "/bin/bash" ]; then
  sudo chsh -s /bin/bash vagrant
fi

# Swap, so a big build does not get OOM-killed. `swapon` lives
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
# host -- there is nothing to read at any privilege level. The
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

- **No credentials in this file.** It is committed, and the
  guest clones it into a machine you are treating as
  expendable. Pass a token in at the moment you need it
  instead, from inside `bombyx shell`.
- **Everything idempotent.** See above; `provision` re-runs it.

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

```console
$ bombyx --project myproject doctor
  local   ssh               ok    OpenSSH_for_Windows_9.5p2 3.8.2 in C:\Windo...
  vmhost  ssh               ok
  vmhost  login shell       ok    posix
  vmhost  vagrant           ok    /usr/bin/vagrant
  vmhost  project dir       ok    /home/igor (will create /home/igor/vms/myproject)
  vmhost  libvirt provider  ok    vagrant-libvirt (0.12.2, global)
all checks passed
```

`doctor` changes nothing and runs every check rather than
stopping at the first failure, so one run tells you everything
that is wrong. Fix anything that is not `ok` before continuing:
`up` creates a directory on the host and writes two files before
it runs `vagrant`, so a missing piece otherwise surfaces
half-way through. `usage.md` explains how to read each line.
`ssh` is the only local program checked, because it is the only
one a VM command runs.

### Look at what `up` would do

```console
$ bombyx --project myproject --dry-run up
ssh vmhost "mkdir -p ~/'vms/myproject'"
ssh vmhost "cat > ~/'vms/myproject/Vagrantfile' <<'BOMBYX_EOF' (33 lines elided)
ssh vmhost "cat > ~/'vms/myproject/bootstrap.sh' <<'BOMBYX_EOF' (266 lines elided)
ssh vmhost "cd ~/'vms/myproject' && BOMBYX_VM_HOST='vmhost' BOMBYX_VM_HOSTNAME=\$(hostname -s) VAGRANT_DEFAULT_PROVIDER='libvirt' vagrant 'up'"
ssh vmhost "cd ~/'vms/myproject' && { names=\$(BOMBYX_VM_HOST='vmhost' BOMBYX_VM_HOSTNAME=\$(hostname -s) vagrant 'snapshot' 'list') && if ! printf '%s\\n' \"\$names\" | grep -qx 'fresh-install'; then BOMBYX_VM_HOST='vmhost' BOMBYX_VM_HOSTNAME=\$(hostname -s) vagrant 'snapshot' 'save' 'fresh-install'; fi || printf 'bombyx: could not save the fresh-install snapshot for %s; re-run this command with snapshot in place of up\\n' 'myproject' >&2; }"
```

Five commands, and every one of them is an `ssh`: make the
directory, write the two files bombyx generates, boot, then save
the `fresh-install` snapshot if the VM does not already have
one. bombyx runs nothing on your workstation.

The two writes print as one line each. Each carries a whole
file, and printing both in full would bury the plan they belong
to, so the line identifies the heredoc and how many lines it dropped.
The full contents are written to the host either way -- this is
the one place `--dry-run` summarises rather than showing you
everything.

The two variables on the last line are how the guest learns
which machine it is running on. The generated Vagrantfile
forwards them into the VM, where `provision.sh` can read them.
The `\$` is deliberate: that name has to be filled in by the
host's shell, not by yours. Every bombyx command accepts
`--dry-run`, and the output is real shell -- worth using
whenever you are unsure what a command is about to touch,
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
the first `up` only. The last step of `up` is a small script the
*host* runs: it lists the snapshots, tests for the name, and
saves one only when the name is missing. bombyx never sees the
list and never makes the decision -- it sends the script and
reads an exit status. So every later `up` finds the snapshot and
leaves it alone, and `fresh-install` goes on describing the VM
as provisioning left it rather than whatever an agent has since
done to it.

That is the state you want to come back to after an agent has
made a mess, which is why it must not be overwritten quietly.

When you do want to move it, ask for it:

```bash
bombyx snapshot
```

That replaces the existing snapshot without asking, so the state
`reset` would have returned to is gone. The VM itself and its
caches are untouched.

Two occasions call for it. The first is a VM you created before
this behaviour existed, and what you have depends on whether you
have run `up` since. If you have, that `up` took a snapshot of
the machine as it stood, which was not a fresh install. If you
have not, there is no snapshot at all. Either way `bombyx
snapshot` is how you set the point you want. The second occasion
is a machine you have brought somewhere worth returning to -- a
long dependency build finished, say -- and want that to be the
new starting point.

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

`up` provisions a VM only when it first creates one. Every later
`up` leaves the guest running the script it cloned when the VM
was created -- and reports success, which is what makes the gap
easy to miss. `provision` re-runs the bootstrap, which fetches
your repository again and checks it out in the clone the guest
already has, so push the change first. That checkout is forced:
it overwrites edits to tracked files, and an untracked file
where the new commit adds one at the same path. It also
detaches HEAD, so committing inside the guest is not enough --
the next `provision` leaves that commit on no branch. See
[usage.md](usage.md) for what survives.

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

`destroy` prints the resolved `<host>:<directory>` it is about
to remove. Read that, not the name you typed -- you gave that
name to `--project` a moment earlier, so typing it again
confirms only that you can read your own command line.

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
- **Arrow keys print `^[[A` inside the VM.** The box created
  its user with dash rather than bash. Part 3.
- **`reset` says the snapshot was not found.** Two causes. The
  VM predates this behaviour and has had no `up` since, so
  nothing ever saved one. Or an `up` tried and could not: that
  step is advisory, so it warns on stderr and lets `up`
  succeed, and the warning is easy to miss several commands
  later. Either way, run `snapshot` and read what it says.
  Part 4.
- **A mount or a host service hangs rather than failing.** The
  nftables rules drop guest-initiated traffic to the host. See
  **What this does and does not buy** in `vm-host-setup.md`.

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
  including the network isolation rules and other distributions.
