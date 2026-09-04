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
> four `ssh` commands, `doctor` without the `tar` and `scp`
> rows -- none of which 0.4.1 could produce. They are written
> from the code rather than captured from a machine, and none
> of it has run against a real VM host. Treat your own first
> `bombyx up` as the real test.
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
    bombyx.toml        the VM to build, and what to clone
    vagrant/           the provisioning script the guest runs

  your own machine, outside any repo:
    config.toml        which VM host is yours
```

- **The workstation** is your daily machine. It holds bombyx,
  your SSH config, your `config.toml`, and the project repo. It
  never runs a VM.
- **The VM host** is usually a different machine, and that is
  what puts your credentials out of reach: an agent that escapes
  its VM lands somewhere holding none of them. It runs libvirt,
  Vagrant and the VMs.
- **The project** is a directory in your own repo. It holds a
  `bombyx.toml` naming the project, the VM to build and the
  repository to clone, and a `vagrant/` directory holding the
  provisioning script. bombyx never sends that directory
  anywhere: the guest clones the repository itself and runs the
  script out of its own clone.
- **Which host** is deliberately *not* in the project repo. It
  is personal, so it lives in your own `config.toml` -- Part 1
  sets that up.

bombyx ships neither the config nor the `vagrant/` directory for
you. Part 3 writes both by hand, once, and after that they live
in the project's repo like any other file.

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

The alias is what goes in your own `config.toml`, which Part 1
sets up below -- never in `bombyx.toml`, which bombyx refuses a
`host` key outright. Name it whatever you like; `vmhost` is
used throughout this tutorial.

Then prove it works without a password prompt, because that is
the form bombyx needs:

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

bombyx does **not** read the host from the project's
`bombyx.toml`, and refuses one written there. A project is
shared and a VM host is not: everyone has their own hardware on
their own network, so a committed value would be wrong for
everyone but its author -- and `bombyx destroy` runs
`vagrant destroy` and `rm -rf` on whatever host is in force.

Write yours once, outside any repo:

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

That covers every project on this machine. Two other sources
override it when you need them, highest first:

| | Source | Use |
|-|--------|-----|
| 1 | `--host other` | one run against another machine |
| 2 | `BOMBYX_HOST=other` | a shell, CI, or an agent |

If neither of those and no `config.toml` names a host, bombyx
stops and lists all three rather than guessing.

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

### Running bombyx against your own machine *(unverified)*

bombyx has no "local mode" and does not need one. `host` is an
SSH alias, so pointing it at your own machine is a matter of
running an SSH server there and giving the loopback an alias:

```sshconfig
Host selfhost
    HostName 127.0.0.1
    User igor
```

Everything else is unchanged -- bombyx still writes the
generated files and runs `vagrant` over SSH, just to a host that
happens to be this one. Read **Before you start** above for what
you give up.

Two requirements are easy to miss, because they are about the
machine being a *host*, not about bombyx:

- **The remote side must be POSIX.** bombyx sends `mkdir -p`
  and a `cat > file <<'EOF'` heredoc, so the SSH login shell
  has to be `bash` or `sh`. On a Linux workstation that is
  already true. On Windows, OpenSSH Server starts `cmd.exe`
  by default and those commands fail; you would have to set
  the `DefaultShell` registry value to a POSIX shell, which
  is why `doctor` has a `login shell` check.
- **libvirt has to run there.** That means a Linux
  workstation. A Windows machine cannot be its own libvirt
  host: the options are a Linux VM or WSL2 distro with
  nested virtualization acting as the host -- see
  [vm-host-wsl2.md](vm-host-wsl2.md), which is verified end to
  end -- or Hyper-V.
  bombyx accepts two provider values, `libvirt` and `hyperv`,
  and refuses anything else -- VirtualBox is not one of them.
  Hyper-V comes with two caveats of its own: its provider
  needs an elevated shell, which an SSH session does not
  have, and bombyx does not yet tell vagrant which provider
  to use, so setting `hyperv` today gets you whatever the
  host chooses. See `provider-configured-not-selected` in
  [todo.md](todo.md).

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

The layout:

```
myproject/                  your project repo
  bombyx.toml           which VM, and what the guest clones
  .gitignore
  vagrant/              the guest runs this from its own clone
    provision.sh
```

### `bombyx.toml`

Copy `bombyx.toml.sample` from bombyx's own repository into your
project as `bombyx.toml`. Its comments explain every key, and a
test loads that file as shipped, so it cannot drift from what
bombyx accepts -- which is worth something, because it has been
unloadable twice.

Then change four values:

| Key | This tutorial uses |
|-----|--------------------|
| `project` | `myproject` -- names the VM and its directory on the host |
| `vm.box` | `debian/bookworm64`; `provision.sh` runs `chsh` because of it |
| `source.repo` | the URL you push this repository to |
| `source.ref` | the branch you push, `main` here |

Leave `provider = "libvirt"`, leave `remote_root` where the
sample puts it -- above `[vm]`, because a bare key belongs to
the table header above it, and below `[vm]` this one would
parse as `vm.remote_root` and the whole file would be refused.

`[vm]` and `[source]` are required and have no defaults. bombyx
builds the VM from the first and the guest clones the second,
so there is nothing sensible for bombyx to guess: a base image
is a choice, and a repository bombyx invented would be cloned
into the guest and run as root.

`remote_root` is optional, shown with its default. There
is no `host` here: it went into your own `config.toml` back in
Part 1, and bombyx refuses one in this file. This is the file
you commit, so it should hold only what is true for anyone who
clones the repo.

If this one project needs a different machine from your usual
one, pass `--host` on the run that needs it. There is no way to
record the choice yet; `docs/issues/project-config-off-repo.md`
plans a per-project entry in your `config.toml`.

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

# Record which machine this VM is running on. (Unverified -- see
# the *(unverified)* note in the header.) Nothing inside the
# guest can work that out: `hostname` here answers `myproject`,
# and the guest's DMI describes the emulated machine (`QEMU`),
# not the host -- there is nothing to read at any privilege
# level. The two variables reach this script because the
# Vagrantfile above passes them in; bombyx put them on the
# `vagrant` process out on the host. A VM booted by a bare
# `vagrant up` sees neither, which is what the defaults are for.
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
git add bombyx.toml .gitignore vagrant/provision.sh
git commit -m "add the bombyx VM definition"
git push origin main          # the branch named in source.ref
```

`bombyx.toml` itself is not read from the repository -- bombyx
reads it from your working directory -- but committing it is
how the next person gets the same VM.

## Part 4: the first boot

### Check the preconditions

From the project directory:

```console
$ bombyx doctor
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
$ bombyx --dry-run up
ssh vmhost "mkdir -p ~/'vms/myproject'"
ssh vmhost "cat > ~/'vms/myproject/Vagrantfile' <<'BOMBYX_EOF' (33 lines elided)
ssh vmhost "cat > ~/'vms/myproject/bootstrap.sh' <<'BOMBYX_EOF' (265 lines elided)
ssh vmhost "cd ~/'vms/myproject' && BOMBYX_VM_HOST='vmhost' BOMBYX_VM_HOSTNAME=\$(hostname -s) vagrant 'up'"
```

Four commands, and every one of them is an `ssh`: make the
directory, write the two files bombyx generates, boot. bombyx
runs nothing on your workstation.

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

```bash
bombyx up
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

### Take the snapshot that `reset` needs

`bombyx reset` restores a snapshot named `fresh-install`, and
**no bombyx command creates it**. Take it by hand now, while the
VM is exactly as provisioning left it:

```bash
ssh vmhost "cd ~/vms/myproject && vagrant snapshot save fresh-install"
```

Without it, `reset` fails with `The snapshot name fresh-install
was not found for the virtual machine`. Taking it now is the
whole value of it: this is the state you want to come back to
after an agent has made a mess.

## Part 5: living with it

The loop you will actually use:

```bash
bombyx shell              # work in the VM
bombyx down               # halt it when you are done
bombyx up                 # boot it again, fast, caches warm
bombyx reset              # roll back to the fresh-install snapshot
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
to remove. Read that, not the name you typed -- `project` comes
from the same `bombyx.toml` that decides what gets deleted.

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
- **`reset` says the snapshot was not found.** Nothing ever
  took the `fresh-install` snapshot. Part 4.
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
