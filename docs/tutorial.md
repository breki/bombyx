# Tutorial: from nothing to a working agent VM

This tutorial introduces bombyx by walking through a complete
first setup. We will touch three machines, or roles, in turn: the
workstation you work on, the VM host that runs the virtual
machines, and a sample project that describes a single VM. By the
end you will have an agent VM that you can open a shell into, halt,
boot again, and throw away.

We recommend that you read the parts in order. Each one checks its
own work before the next comes to depend on it, so that a mistake
announces itself where it was made rather than three steps
downstream, where it would be far harder to trace.

> **Note -- what this tutorial was checked against.**
>
> The workstation steps (Parts 1 and 2) were run on Windows 11 in
> August 2026, and this includes the two failure cases described
> in **When something goes wrong**.
>
> The transcripts in Parts 3 and 4, however, were written from the
> code rather than captured from a run, and are marked
> *(unverified)* accordingly. That bombyx generates the
> Vagrantfile, that `up` expands to five `ssh` commands, and that
> `doctor` omits the `tar` and `scp` rows are all current
> behaviour; only the exact output is transcribed from the source,
> not produced by running against a remote VM host. Because they
> are written from the source rather than captured, treat your own
> first `bombyx up` as the real test.
>
> One route has since been exercised from end to end. On
> 2026-09-05 the whole sequence -- `doctor`, `up`, `status`,
> `shell`, `down`, `provision`, `scratch`, `discard` and `destroy`
> -- was run on a Linux workstation with `host` naming that
> machine itself, against a guest that booted and provisioned to
> completion. The local route is therefore verified, guest
> included; see [local-host.md](local-host.md) for the detail. The
> `ssh`-route transcripts below remain written from the code.
>
> The VM host steps summarise [vm-host-setup.md](vm-host-setup.md),
> which records what it in turn was verified against; follow that
> page when you want the detail.
>
> Finally, the sample `provision.sh` in Part 3 is *(unverified)* as
> written here, having been assembled from a working setup rather
> than copied from one. bombyx writes the Vagrantfile itself, so
> Part 3 has no Vagrantfile to sample. The comments explain what
> each setting is for, so that a failure should be diagnosable
> rather than mysterious.

## Contents

- [The three pieces, and why they are separate](#the-three-pieces-and-why-they-are-separate)
- [Before you start](#before-you-start)
- [Part 1: the workstation](#part-1-the-workstation)
  - [Install bombyx](#install-bombyx)
  - [Give the VM host an SSH alias](#give-the-vm-host-an-ssh-alias)
  - [Name your VM host, once](#name-your-vm-host-once)
- [Part 2: the VM host](#part-2-the-vm-host)
  - [Running bombyx against your own machine](#running-bombyx-against-your-own-machine)
  - [Optional: keep the VM from reaching your home network](#optional-keep-the-vm-from-reaching-your-home-network)
- [Part 3: the sample project](#part-3-the-sample-project)
  - [The project's table in `config.toml`](#the-projects-table-in-configtoml)
  - [`.gitignore`](#gitignore)
  - [The Vagrantfile: bombyx writes it](#the-vagrantfile-bombyx-writes-it)
  - [`.bombyx/provision.sh`](#bombyxprovisionsh)
  - [Push it, or the guest has nothing to clone](#push-it-or-the-guest-has-nothing-to-clone)
- [Part 4: the first boot](#part-4-the-first-boot)
  - [Check the preconditions](#check-the-preconditions)
  - [Look at what `up` would do](#look-at-what-up-would-do)
  - [Boot it](#boot-it)
  - [The snapshot that `reset` returns to](#the-snapshot-that-reset-returns-to)
- [Part 5: living with it](#part-5-living-with-it)
- [When something goes wrong](#when-something-goes-wrong)
- [Where to go next](#where-to-go-next)

## The three pieces, and why they are separate

Before we begin, it helps to have a picture of how the pieces fit
together:

```
workstation                     VM host
  bombyx  ──── ssh ────►    vagrant ──► agent VM
     │                                     │
     └── writes Vagrantfile ───────────────┘
         and two guest scripts     clones the repo itself

  the project repo:
    .bombyx/           holds the provisioning script the guest runs
      provision.sh

  your own machine, outside any repo:
    config.toml        which VM host is yours, and one table
                       per project
```

There are three roles here, and it is worth being clear about
what each one does and, just as importantly, what it does not:

- **The workstation** is your daily machine. It holds bombyx, your
  SSH configuration and your `config.toml`. Note that it never
  runs a VM, and that it does not even need a checkout of the
  project.
- **The VM host** is usually a different machine, and this
  separation is precisely what puts your credentials out of reach:
  an agent that escapes its VM lands on a machine that holds none
  of them. It is the VM host that runs libvirt, Vagrant and the
  VMs themselves.
- **The project** is a repository somewhere the guest can reach.
  The only part of it that concerns bombyx is a provisioning
  script, and even that bombyx never transmits; the guest clones
  the repository for itself and runs the script out of its own
  clone.
- **Every setting** lives in your own `config.toml`, outside any
  repository. That file names the VM host and carries one
  `[projects.<name>]` table per project. Part 1 creates the file
  and Part 3 adds a table to it.

Note that bombyx ships neither that file nor the `.bombyx/`
directory on your behalf. Parts 1 and 3 write both by hand, once.

## Before you start

You will need two machines and about an hour, most of which is
spent waiting for packages to install and for the first box to
download.

| Where | What |
|-------|------|
| Workstation | `git`, `ssh`, `curl`, `tar` |
| VM host | A Linux machine with hardware virtualisation, reachable over SSH |
| Both | Key-based SSH from the workstation to the host, no password |

On Windows, `ssh` is provided by the OpenSSH client that ships
with Windows 11, and `tar` comes with either Windows or Git for
Windows; you do not need WSL for this. `curl` and `tar` are what
fetch and unpack the release archive, both for the first install
in Part 1 and for `bombyx self-update` later; no VM command runs
either. `git` is for pushing your project in Part 3.

A spare desktop or a home server makes the best VM host, because a
separate machine is what keeps your credentials out of reach: an
agent that breaks out of the VM then lands on a machine that holds
nothing of yours, and the host firewall rules can keep it off your
LAN as well.

Your workstation may, however, serve as the VM host too, and this
is a supported way to run bombyx. You retain a separate kernel, no
host filesystem mounted into the guest, and no credentials inside
the guest, so that an agent which misbehaves, runs a hostile
`postinstall`, or acts on a prompt injection is still contained.
What you give up is the very thing that required two machines in
the first place: an escaped guest is already on your workstation,
and there is no separate network to isolate it from. This is a
genuine trade -- you surrender network isolation, and in return
you avoid running a second machine at all. It requires no special
mode, since `host` is simply an SSH alias and may therefore point
at your own machine (see [local-host.md](local-host.md)).

## Part 1: the workstation

### Install bombyx

Install bombyx from a release package; there is no need to build
it from source, and end users never should.
[quickstart.md](quickstart.md) under **Install** has the
download-and-verify steps for Linux, macOS and Windows.

Check that it landed:

```console
$ bombyx --version
bombyx <version>    # whatever you installed
```

### Give the VM host an SSH alias

bombyx itself never handles addresses, usernames or keys. It runs
`ssh <alias>`, and everything about how that connection is made
belongs to your `~/.ssh/config`. Add an entry for the host:

```sshconfig
Host vmhost
    HostName 192.168.1.50
    User igor
    IdentityFile ~/.ssh/id_ed25519
```

The alias is what you will place in your own `config.toml`, which
the next section writes. Name it whatever you like; `vmhost` is
the name used throughout this tutorial.

Note that if your VM host is this very machine, you should skip
ahead. On a Linux workstation with libvirt installed, bombyx runs
`vagrant` here and needs no alias, no key and no SSH server;
[local-host.md](local-host.md), which Part 2 points to, replaces
this step and the `host` line that goes with it.

Otherwise, prove that the alias works without prompting for a
password, since that is the form bombyx requires:

```console
$ ssh vmhost true
$ echo $?
0
```

Should that ask for a password, copy your key across (with
`ssh-copy-id vmhost`, or by appending the public key to
`~/.ssh/authorized_keys` on the host) and try again. Do not
continue until the command is silent.

> **Warning -- testing one specific key honestly.** The invocation
> `ssh -i key -o IdentitiesOnly=yes` does *not* ignore identities
> named in `ssh_config`, so on a machine that has other
> `IdentityFile` entries it may silently authenticate with a
> different key. The success it then reports belongs to that other
> key, not to the one you meant to test. Add `-F /dev/null` to
> ignore the configuration when the key is the thing you are
> testing.

### Name your VM host, once

Because a VM host is never shared in the way a project's
repository is, bombyx reads no file out of that repository at all.
The reasoning is worth spelling out: a project is shared and a VM
host is not, since everyone has their own hardware on their own
network. A value committed to the repository would therefore be
wrong for everyone but its author -- and, since `bombyx destroy`
runs `vagrant destroy` and `rm -rf` on whatever host is in force,
being wrong about the host is not a harmless mistake.

Write yours once, then, outside any repository. This is the same
file to which Part 3 will add a project table, so keep the path in
mind:

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

That single line covers every project on this machine. A project
that runs somewhere else may be given a `host` of its own inside
its table, a point to which Part 3 returns, and that key then wins
for that project alone. If neither the top-level key nor a
project's table names a host, bombyx stops and tells you which
line to add rather than guessing.

## Part 2: the VM host

This part is done on the host, whether over SSH or at its console.
The real reference is [vm-host-setup.md](vm-host-setup.md), which
has the exact commands, the package names that changed in Ubuntu
24.04, and what to do when a step fails; what follows is only the
shape of it:

1. **QEMU and libvirt**, with your user added to the `libvirt`
   group. Log out and back in for the group to take effect.
2. **Vagrant**, from HashiCorp's repository. Note that Ubuntu
   24.04 removed its own `vagrant` package.
3. **The libvirt provider plugin**, installed with
   `vagrant plugin install vagrant-libvirt`.
4. **The default storage pool set to autostart.** The pool creates
   itself but does not return after a reboot, which surfaces weeks
   later as a confusing `vagrant up` failure.

Then check the one thing that is easy to get wrong and hard to
diagnose. Run this **from the workstation**, not on the host:

```console
$ ssh vmhost vagrant --version
Vagrant 2.4.9
```

This is not the same test as logging in and typing `vagrant
--version`. The form `ssh host "cmd"` starts a non-interactive
shell, which skips the startup files that ordinarily extend
`PATH`; a Vagrant installed outside that non-interactive `PATH`
works perfectly when you log in and is nonetheless invisible to
bombyx. [vm-host-setup.md](vm-host-setup.md) explains the mechanism
and the fix under **Why the non-interactive PATH causes trouble**.

### Running bombyx against your own machine

Your workstation may serve as its own VM host, on Linux, with
bombyx running `vagrant` through `sh -c` rather than `ssh`. That
route replaces the SSH alias and `host` line from Part 1, so if it
applies to you, read it before you do the rest of Part 2.
[local-host.md](local-host.md) is the whole procedure: how bombyx
decides the route from `host`, why the name must match exactly,
and the Windows and Hyper-V caveats.

### Optional: keep the VM from reaching your home network

By default a libvirt guest can reach everything the host can reach
-- your LAN, your router, the host's own SSH port. If the VM is
going to run code you do not trust, that is worth closing off. The
script `scripts/agent-vm-firewall.sh` in this repository loads an
nftables ruleset that permits outbound internet traffic and
refuses private destinations, and **Keeping agent VMs off your
home network** in `vm-host-firewall.md` explains what it does and
does not buy you. That page is marked unverified, so read it
before applying it.

You may skip this and return to it later; the rest of the tutorial
does not depend on it.

## Part 3: the sample project

This part is done on the workstation, inside whatever repository
you want a VM for.

It must be a real repository, pushed somewhere the guest can
reach. bombyx sends no project file anywhere: the VM clones
`source.repo` at `source.ref` for itself and runs `source.script`
out of that clone. A directory that was never pushed leaves the
guest failing at clone time, which is both late and confusing.

An empty repository will not do, for the same reason. By the end
of this part the repository must hold `.bombyx/provision.sh`, on
the branch you name in `ref`, and pushed. Part 3 writes that file
and ends with the step that pushes it.

This tutorial uses a public repository, so that the guest clones
with no credential of its own. A private repository needs a
credential inside the VM: name a deploy key on the VM host with
`deploy_key` in `[source]`, and `vagrant` uploads it into the
guest before provisioning. Bear in mind that code in the VM can
read that key -- see [trust-boundary.md](trust-boundary.md) for
what that costs.

A project's own secrets travel in the other direction. `env_file`
in `[source]` names a file on the machine you are typing on,
usually the project's untracked `.env`, which bombyx carries into
the guest; your provisioning script then copies it into place from
`$BOMBYX_ENV_FILE`. The sample config explains this in full.

Alongside it, `repo_token` and `repo_user` clone a private
repository over `https` instead, authenticating with a token that
lives in that same secrets file. On Bitbucket this is the only
arrangement an agent can push with, because an ssh access key
there is read-only. The sample config explains both.

The layout, in the two places it occupies:

```
myproject/                  your project repo
  .gitignore
  .bombyx/              the guest runs this from its own clone
    provision.sh

~/.config/bombyx/
  config.toml           the host from Part 1, plus the project
                        table this part adds
```

### The project's table in `config.toml`

Open `config.toml.sample` from the bombyx repository, at
<https://github.com/breki/bombyx/blob/main/config.toml.sample>. Its
comments explain every key. A test loads that file as shipped, so
that the sample cannot silently stop parsing; it has failed to
load twice in the past, which is why the test was added.

Copy the `[projects.myproject]` block out of it and append it to
the `config.toml` you wrote in Part 1, below the `host` line. Then
change the following:

| Key | This tutorial uses |
|-----|--------------------|
| the table key | `myproject` -- names the VM and its directory on the host |
| `vm.box` | `generic/ubuntu2204` -- it carries `git`; see below |
| `source.repo` | the URL you push this repository to |
| `source.ref` | the branch you push, `main` here |

Pick a box that carries `git`. `generic/ubuntu2204` does, and it
is the value in `config.toml.sample`. A box without it --
`debian/bookworm64`, say -- cannot finish the first `up`: the
guest boots, and then the provisioner refuses at clone time and
exits 1. Your own `provision.sh` cannot rescue such a box, since
`git` is exactly what would fetch that file in the first place.

A GitHub or Bitbucket URL over ssh needs `curl` in the box, and
`jq` as well for GitHub. The reason is that, before it clones,
bombyx has the guest fetch that host's published ssh keys, so as
to tell the real server from an impostor rather than trusting
whatever answers on port 22; `docs/trust-boundary.md` explains
why. The fetch runs `curl` for either host. Reading GitHub's
answer needs `jq` on top of that, because GitHub publishes its
keys as JSON while Bitbucket publishes finished `known_hosts`
lines -- so a Bitbucket clone asks for no `jq` at all. A box
missing a program it needs is refused by name, in the same way a
missing `git` is:

```
bombyx: jq is not installed in this box, and bombyx needs it to read github.com's published ssh host keys. Install jq in the box, or choose one that has it.
```

A second `bombyx:` line follows it, the same one the `git` passage
above shows.

*(unverified)* We have not booted a guest to establish which of
these boxes carries `jq`. Ubuntu and Debian cloud images generally
do not, so expect to install it -- and note again that your own
`provision.sh` cannot do so, for the reason the `git` passage
gives. An `https://` URL needs neither program, since it opens no
ssh connection at all.

Keeping the Debian box would mean installing `git` into it and
repackaging it, which this tutorial does not cover.

The table key is the project name, so nothing inside the table
repeats it. It is `--project myproject`, given on every command,
that selects this table: bombyx opens no file in the project's
directory, and so cannot infer which project you mean from where
you happen to be standing.

Leave `provider = "libvirt"` as it is. Deleting the line amounts
to the same thing, since libvirt is what bombyx assumes when the
key is absent.

Leave `remote_root` where the sample puts it, above
`[projects.myproject.vm]`. A bare key belongs to the table header
above it, so written below that header this one would parse as
`projects.myproject.vm.remote_root`, and the whole file would be
refused.

`[vm]` and `[source]` are required, and every key within them --
except `provider`, `deploy_key`, `env_file`, `repo_token` and
`repo_user` -- is required too. bombyx builds the VM from `[vm]`
and the guest clones the repository named in `[source]`, so there
is nothing sensible for bombyx to invent on your behalf: a base
image is a choice, and a repository bombyx made up would be cloned
into the guest and have its script run there.

`remote_root` is optional, and is shown with its default.

If this one project runs on a machine different from your usual
one, add a `host` line inside its table, above the two tables. It
wins for this project, and bombyx prints a line on stderr to say
so on every command -- because `destroy` runs `rm -rf` on
whichever host wins.

### `.gitignore`

```gitignore
.vagrant/
```

The directory `.vagrant/` holds a VM's identity, written by
`vagrant` if you ever run it in this repository yourself. bombyx
neither reads nor sends it; ignoring it prevents a stale copy
from entering the repository and confusing the next reader.

### The Vagrantfile: bombyx writes it

You do not write a Vagrantfile. bombyx renders one from `[vm]` and
writes it onto the VM host on every `up`, `provision` and
`scratch`, together with a small bootstrap script.

This is not merely a convenience. Vagrant reads the Vagrantfile
before the VM exists, so a project-supplied Vagrantfile would have
to sit on a machine outside the guest -- and keeping project code
off those machines is the entire point.
[trust-boundary.md](trust-boundary.md) records the reasoning.

Two things the generated file does are worth knowing:

- **It disables the default `/vagrant` share.** Vagrant would
  otherwise mount the VM host's copy of that directory into the
  guest. There is no project code in it to leak now, but the mount
  also *hangs* on a host whose firewall drops guest-initiated
  traffic, rather than failing clearly.
- **It forwards the VM-host identity.** `BOMBYX_VM_HOST` and
  `BOMBYX_VM_HOSTNAME` reach your provisioning script as
  environment variables, so that it can record which machine the
  VM is running on. See "Telling the VM which host it runs on" in
  [../README.md](../README.md).

Neither bombyx nor Vagrant reads a `Vagrantfile` you commit in
your repository: bombyx does not send it, and the guest's own
clone is not what Vagrant boots from. Delete such a file rather
than maintaining it.

The guest clones `[source]` itself and runs the script named
there, which is the file the next section covers.

### `.bombyx/provision.sh`

There are three facts about how this script runs, and they
between them decide how you should write it.

First, it runs as `agent`, an account bombyx creates in each VM
for the agent to work as. You can choose another name with
`guest_user` in the `[vm]` table. It is not `vagrant`: Vagrant
keeps that account for logging in, and nothing of yours runs as
it. Anything the script installs into a home directory therefore
lands where the agent will find it. Running it as root instead
would put a toolchain in `/root`, which is exactly the mistake this
arrangement exists to avoid.

Second, `sudo` is available for the steps that genuinely need
root: bombyx gives the agent's account passwordless `sudo`, which
is why every privileged line in the example below has it.

Third, its working directory is the clone, which bombyx names
after your project -- `~/myproject` here, in that user's home, so
several agent VMs are told apart by their directory rather than by
asking which repository each one holds. `bombyx shell` opens in
that same directory, so you land where the script runs. The clone
is the only copy of your code in the VM.

Save the script below as `.bombyx/provision.sh`, creating the
directory as you go: `mkdir -p .bombyx`. The leading dot makes
`.bombyx/` a hidden directory, so create it from a shell rather
than a file manager.

> **Note**: On Windows, File Explorer will not create a folder
> whose name begins with a dot. Make the directory from a shell
> instead -- `New-Item -ItemType Directory .bombyx` in PowerShell,
> or `mkdir -p .bombyx` in Git Bash or WSL.

Write the script so that it is re-runnable. `bombyx provision`
runs it again on an existing VM, so every step should either be
idempotent or check before it acts.

```bash
#!/usr/bin/env bash
set -euo pipefail

sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential ca-certificates curl git jq ripgrep tmux

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

Add whatever the agent needs on top of this -- a language
toolchain, an agent CLI. Two rules are worth keeping in mind:

- **Do not write credentials into this file.** Git tracks it, and
  the guest clones it into a machine you are treating as
  expendable. Pass a token in at the moment you need it instead,
  from inside `bombyx shell`.
- **Make every step idempotent**, as noted above, since
  `provision` re-runs it.

### Push it, or the guest has nothing to clone

This is the step on which the rest of the tutorial depends. The VM
does not read your working copy -- it clones `source.repo` at
`source.ref` and runs `source.script` from that clone. Until these
files are on the branch that `ref` names, `bombyx up` boots a
machine that fails inside the guest.

We assume here that the project is already a git repository with a
remote. If it is not, make an empty repository on the host of your
choice first, and then:

```bash
git init
git branch -M main
git remote add origin https://github.com/you/myproject
```

Set `source.repo` to that same URL -- the guest clones what `repo`
names, not whatever `origin` happens to be. Then:

```bash
git add .gitignore .bombyx/provision.sh
git commit -m "add the provisioning script the guest runs"
git push origin main          # the branch named in source.ref
```

Only the provisioning script goes into the repository. The VM
description stays in your own `config.toml`, so that the next
person who wants the same VM copies that table rather than cloning
it.

## Part 4: the first boot

### Check the preconditions

From any directory -- bombyx reads nothing out of the project's,
so where you stand makes no difference:

```bash
bombyx --project myproject doctor
```

`doctor` changes nothing, and runs every check rather than
stopping at the first failure, so that a single run tells you
everything that is wrong at once. You want every row to read `ok`,
and you should fix anything that does not before continuing:
because `up` creates a directory on the host and writes three files
before it runs `vagrant`, a missing piece would otherwise surface
half-way through. [usage.md](usage.md) under **doctor** has a
sample run and how to read it. Note that `ssh` is the only local
program checked, since it is the only one a VM command runs.

### Look at what `up` would do

Every bombyx command accepts `--dry-run`, which prints the exact
shell it would run and touches nothing:

```console
$ bombyx --project myproject --dry-run up
```

`up` amounts to seven `ssh` commands, and bombyx runs nothing on
your workstation. It checks whether the machine is already
running; makes a directory on the host; writes the three files it
generates -- the Vagrantfile, `bootstrap.sh` and `account.sh` --
down a pipe, so that their contents never appear as command
arguments; boots with `vagrant up`; and takes a `fresh-install`
snapshot, so that `bombyx reset` has a state to return to. A
project that sets `env_file` or `repo_token` gets an eighth or
ninth command, staging that file.

**Read the plan; never pipe it into a shell.**
`bombyx --dry-run up | sh` writes the two generated files empty --
or not at all, depending on the shell -- and can leave
`vagrant up` running against an empty Vagrantfile, with a zero
exit that reads as success. The file sizes the plan reports change
with almost every release, so run the command to see the figures
for your version.

`--dry-run` is real shell and touches nothing, so it is worth
using whenever you are unsure what a command is about to do, and
especially so for `destroy`.

### Boot it

Every command from here on takes `--project myproject`. The
examples leave it out so that the line under discussion stays
readable; typed without it, bombyx stops and says the argument is
required.

```bash
bombyx --project myproject up
```

The first run downloads the box on the host and takes a while;
later runs take about as long as the VM needs to boot. The
provisioners run only on this first `up`, when the VM is created.

Then get in:

```bash
bombyx shell
```

This is `ssh -t` through to `vagrant ssh` on the host. Vagrant
logs in as its own account, and the guest then switches to the
agent's account with `sudo -u` and opens a shell in the clone.

### The snapshot that `reset` returns to

`bombyx reset` restores a snapshot named `fresh-install`, and the
`up` you ran a moment ago has already taken it. This happens on
the first `up` only: every later one finds the snapshot and leaves
it alone, so that `fresh-install` goes on describing the VM as
provisioning left it, rather than whatever an agent has since done
to it. [usage.md](usage.md) states the rule and the reasoning
behind it, and [architecture.md](architecture.md) shows the script
the host runs to apply it.

That is the state you will want to come back to after an agent has
made a mess, which is why it must not be overwritten quietly.

When you do want to move it, ask for it explicitly:

```bash
bombyx snapshot
```

This replaces the existing snapshot without asking, and `reset`
can no longer take you back to the old state -- it is gone. The VM
itself and its caches are untouched.

Two occasions call for it: a VM whose `fresh-install` snapshot no
longer records a fresh install, and a machine you have brought to
a state worth returning to, such as one where a long dependency
build has finished. [usage.md](usage.md) under **reset and
snapshot** covers both.

## Part 5: living with it

Here is the loop you will actually use:

```bash
bombyx --project myproject shell   # work in the VM
bombyx --project myproject down    # halt it when you are done
bombyx --project myproject up      # boot again, fast, caches warm
bombyx --project myproject reset   # roll back to the snapshot
```

When you change `.bombyx/provision.sh`, use `provision`, not `up`:

```bash
bombyx provision
```

The reason is that `up` provisions a VM only when it first creates
one; every later `up` leaves the old script in place and still
reports success, which makes the gap easy to miss. `provision`
re-runs the bootstrap, so push your change first. The re-checkout
is forced and detaches HEAD, so that an agent's uncommitted edits
and in-guest commits do not survive it; [usage.md](usage.md) under
**up and provision** sets out what is kept and what is lost.

For untrusted code -- an external PR, an unfamiliar dependency
tree -- use a throwaway VM rather than your project one:

```bash
bombyx scratch pr-1234    # boot a fresh VM under that name
bombyx discard pr-1234    # destroy it and remove its directory
```

And to remove the project VM entirely, naming it as confirmation:

```bash
bombyx destroy myproject
```

`destroy` prints the resolved `<host>:<directory>` it is about to
remove; check that target, not the name you typed.
[usage.md](usage.md) under **destroy and discard** explains why
the name alone proves little.

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
dealt with, are these:

- **`Could not resolve hostname`** -- there is no `Host` entry for
  the alias in `~/.ssh/config`, or it is misspelled in your
  `config.toml`. See Part 1.
- **SSH asks for a password.** Key authentication is not set up,
  and bombyx cannot answer a prompt. See Part 1.
- **`vagrant: command not found` over SSH, though it works when
  you log in.** Vagrant is installed outside the non-interactive
  `PATH`. See **Why the non-interactive PATH causes trouble** in
  `vm-host-setup.md`.
- **`doctor` reports `libvirt provider FAIL`.** `vagrant-libvirt`
  is not installed for the user bombyx logs in as. See step 3 of
  `vm-host-setup.md`.
- **`up` fails on the host, complaining about a storage pool.**
  The default pool exists but is not set to autostart, so it is
  gone after a reboot. See the storage pool section of
  `vm-host-setup.md`.
- **Edits to `provision.sh` appear to do nothing.** `up` skips
  provisioners once the VM exists; use `bombyx provision`. See
  Part 5.
- **Arrow keys print `^[[A` inside the VM.** The agent's login
  shell is dash, not bash. bombyx creates the account with
  `/bin/bash`, but an account the box already had under that name
  keeps its own shell; `sudo chsh -s /bin/bash <guest_user>`
  switches it at the next login.
- **`reset` says the snapshot was not found.** There are two
  causes. Either the VM has no `fresh-install` snapshot because no
  `up` has saved one; or an `up` tried and could not, in which
  case, that step being advisory, it warns on stderr and lets `up`
  succeed -- and the warning is easy to miss several commands
  later. Either way, run `snapshot` and read what it says. See
  Part 4.
- **A mount or a host service hangs rather than failing.** The
  nftables rules drop guest-initiated traffic to the host. See
  **What this does and does not buy** in `vm-host-firewall.md`.

One habit is worth borrowing. When you check whether a bombyx
command succeeded, do not pipe it through `tee` or `tail`: a shell
pipeline reports only its last command's status, so a failed run
reads as a pass. Redirect to a file instead
(`bombyx provision > run.log 2>&1`) and print `$?`.

## Where to go next

- [../README.md](../README.md) -- what bombyx is, and the design
  behind it.
- [usage.md](usage.md) -- the full command reference: the
  commands, the two lifecycles, and how to read `doctor` and
  `list`.
- [vm-host-setup.md](vm-host-setup.md) -- the host in detail,
  including the other distributions.
- [local-host.md](local-host.md) -- running bombyx against your
  own machine, when the workstation is also the VM host.
- [vm-host-firewall.md](vm-host-firewall.md) -- keeping agent VMs
  off your home network with host nftables rules.
