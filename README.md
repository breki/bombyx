# bombyx

Drive isolated AI-agent VMs on a remote libvirt host over
SSH.

*Bombyx mori* is the domesticated silkworm -- the animal
that spins the cocoon. The tool builds the enclosure; the
agent works inside it.

## Contents

- [Why](#why)
- [Model](#model)
- [Install](#install)
- [Configure](#configure)
  - [Why nothing bombyx reads is
    committed](#why-nothing-bombyx-reads-is-committed)
  - [Where bombyx looks for the host](#where-bombyx-looks-for-the-host)
  - [Which host a command is about to use](#which-host-a-command-is-about-to-use)
- [Use](#use)
- [Updating bombyx](#updating-bombyx)
  - [A version number is the only thing
    compared](#a-version-number-is-the-only-thing-compared)
  - [Why the binary is renamed](#why-the-binary-is-renamed)
- [Telling the VM which host it runs on](#telling-the-vm-which-host-it-runs-on)
- [Development](#development)
- [Status](#status)
- [Origin](#origin)
- [License](#license)

## Why

Running an AI coding agent on your daily driver puts your
password manager, SSH keys, cloud credentials and browser
profiles one prompt injection or malicious `postinstall`
away from exfiltration. The defence is a VM with its own
kernel, no host filesystem, and none of your credentials --
on a machine that is not your laptop.

"None of your credentials" is the honest form of that claim,
and the distinction will start to matter. A VM that fetches
a private repository by itself needs a credential of its own
to do it, and code inside the VM can read it. See
[trust-boundary.md](docs/trust-boundary.md) for what is
accepted there and why.

bombyx is the control plane for that setup: it runs
`vagrant` on the VM host over SSH so you can stay on your
workstation.

## Model

```
workstation                  vmhost (VM host)
  bombyx  ──── ssh ────►  vagrant ──► agent VM
     │                          ▲          │
     └── writes Vagrantfile ────┘          │
         and bootstrap.sh                  │
                                  clones the repo itself
```

Two rules shape the design:

1. **Neither your workstation nor the VM host reads your
   project's files.** bombyx sends the VM host two files and
   generates both: a Vagrantfile from your `[vm]` settings, and
   a bootstrap script. Vagrant needs the Vagrantfile before the
   VM exists, so it cannot come from inside the guest. Once the
   VM is up, the guest clones the project itself from
   `[source]`.

   Every setting comes out of one file that is yours, in your
   own config directory, and you name the project you mean on
   the command line. bombyx opens nothing in the project's own
   directory, so it needs no checkout on the workstation at
   all. The argument, and what it costs, are in
   [trust-boundary.md](docs/trust-boundary.md).
2. **Wrap, don't reimplement.** bombyx composes `ssh` and
   `vagrant`. If it breaks, `ssh vmhost` and `vagrant up` by
   hand still work.

## Install

```bash
cargo install --path crates/bombyx
```

That installs the CLI on your workstation. The VM host needs
libvirt, Vagrant and its `libvirt` provider -- see
[docs/vm-host-setup.md](docs/vm-host-setup.md), which also
covers the non-interactive `PATH` trap that makes Vagrant
invisible to bombyx while still working when you log in.

To use a WSL2 distribution on your own Windows machine as the
host, add [docs/vm-host-wsl2.md](docs/vm-host-wsl2.md). It works,
and it costs you the isolation that motivates the tool -- that
page says what is lost and what is not.

[docs/architecture.md](docs/architecture.md) is the short
version of how the pieces fit: the three machines, the module
graph, and what `bombyx up` actually does.

If this is your first setup, follow
[docs/tutorial.md](docs/tutorial.md) instead of the sections
below. It goes through all three pieces in order -- the
workstation, the VM host, and a sample project with a
`Vagrantfile` and a provisioning script -- and ends with a VM
you can open a shell into.

## Configure

One file, and it is yours. Copy
[config.toml.sample](config.toml.sample) to your own config
directory -- `~/.config/bombyx/config.toml`, or
`%APPDATA%\bombyx\config.toml` on Windows -- and edit it. That
file is the one place a full example lives, and a test loads it
as shipped, so it cannot drift from what bombyx accepts.

It names the machine your VMs run on, then carries one table per
project:

```toml
host = "my-vmhost"

[projects.myproject]
remote_root = "~/vms"

[projects.myproject.vm]
provider = "libvirt"
box = "generic/ubuntu2204"
cpus = 4
memory = 8192

[projects.myproject.source]
repo = "https://github.com/you/myproject"
ref = "main"
script = "vagrant/provision.sh"
```

What a project's table holds:

| Key | |
|-----|---|
| `[vm]` | required; `provider`, `box`, `cpus`, `memory`, no defaults |
| `[source]` | required; `repo`, `ref`, `script` -- what the guest clones |
| `remote_root` | optional, `~/vms`; must sit above the two tables |
| `host` | optional; only for a project that runs elsewhere |

The table key is the project name, and nothing inside the table
repeats it. A name containing a `.` has to be quoted --
`[projects."a.b"]` -- because TOML reads a bare dot as nesting.

**Name the project on every command**: `bombyx --project
myproject up`. bombyx reads nothing out of the project's own
directory, so it cannot work out which project you mean from
where you happen to be standing. `--config <path>` reads a
different registry file, which is how you keep two setups
apart.

**bombyx writes the Vagrantfile; the project does not.** It is
generated from `[vm]` and written on the VM host on every `up`,
`provision` and `scratch`. A `Vagrantfile` committed in your
project is never read by anything: bombyx does not send it, and
the guest's clone is not what Vagrant boots from.

Neither `[vm]` nor `[source]` has defaults, and both are
required. There is no defensible default for a base image, and
a repository bombyx guessed at would be cloned into the guest
and run as root.

**`[source]` is fetched by the guest, not by you.** The
generated Vagrantfile runs a bootstrap script inside the VM
that clones `repo` at `ref` and runs `script` from the clone.
A private repository therefore needs a credential inside the
guest, which is an accepted and unsolved exposure -- see
[trust-boundary.md](docs/trust-boundary.md).

Then name your own VM host once, in a file outside the repo:

```toml
# ~/.config/bombyx/config.toml
# Windows: %APPDATA%\bombyx\config.toml
host = "my-vmhost"
```

`host` is an SSH alias, resolved through your `~/.ssh/config` --
bombyx never handles addresses, usernames or keys itself.

### Why nothing bombyx reads is committed

**No file inside a repository configures bombyx.** A project is
shared; a VM host is not. Every developer has their own
hardware on their own network, so a committed `host` could only
ever be right for the person who wrote it, and would be wrong
for everyone who cloned after them. That is not a cosmetic
problem: `bombyx destroy` runs `vagrant destroy` and `rm -rf`
on whatever host is in force.

Keeping the whole config out of the repository also keeps every
value out of reach of a branch. `host` is handed to `ssh` as its
first argument, and `ssh` reads a leading `-` as an option, so a
value such as `-oProxyCommand=...` runs code on your workstation
from a bare `bombyx status`. bombyx refuses any host beginning
with `-` wherever it came from, and no clone can supply one in
the first place.

### Where bombyx looks for the host

Two keys, and the project's own wins:

| | Key | Use |
|-|-----|-----|
| 1 | `host` inside `[projects.<name>]` | the one project you keep elsewhere |
| 2 | `host` at the top of the file | every other project -- the usual one |

Both live in the same file. Writing the machine name once, at
the top, covers every project; renaming the machine is then one
line rather than one per project. If neither key names a host,
bombyx stops and says which line to add rather than guessing.

**This section is the authoritative list** -- the sample config
and `llms.txt` point here rather than repeating it, so there is
one place to correct when it changes.

Unless `--config` names a file outright, your `config.toml`
lives in the first of these that the environment names:

| Variable | Config file |
|----------|-------------|
| `BOMBYX_CONFIG_HOME` | `$BOMBYX_CONFIG_HOME/config.toml` |
| `%APPDATA%` (Windows only) | `%APPDATA%\bombyx\config.toml` |
| `XDG_CONFIG_HOME` | `$XDG_CONFIG_HOME/bombyx/config.toml` |
| `HOME` | `$HOME/.config/bombyx/config.toml` |

`BOMBYX_CONFIG_HOME` is another way to keep two setups apart.
It is also the one thing a per-directory environment tool can
redirect from inside a clone, which "Which host a command is
about to use" below returns to.
`%APPDATA%` is consulted **only** on Windows: it is often
exported under WSL and Wine too, and honouring it there would
read a Windows config directory in preference to
`$HOME/.config`.

Each of those values must be an anchored path. A blank or
relative one counts as unset and the next row is tried, because
a relative path resolves against the working directory -- which
on this tool means taking the VM host out of whatever repo you
happen to be in.

### Which host a command is about to use

The `host` at the top of your file specifies the VM host you use
for everything, and a project's own `host` key redirects that
project. Whenever a project's key wins, bombyx prints one line
on stderr naming the table:

```
bombyx: host vmhost-b from [projects."myproject"].host in config.toml
```

The top-of-file `host` gets no such line. It is the ordinary
case, and a line on every command is noise nobody reads. So
silence means the host came from the top of a `config.toml`, and
a line means one project's own table overrode it.

Note what silence does *not* promise. It says the host came
from the top of a `config.toml`, not that it came from *yours*:
`BOMBYX_CONFIG_HOME` chooses which config directory bombyx
reads, and a per-directory environment tool such as `direnv`
can set it from inside a clone. `destroy` is the command that
shows the host regardless, in the `host:directory` line it asks
you to confirm.

Every `host` in the file is checked as the file is read, not
only the one that wins. A value starting with `-` is refused
rather than handed to `ssh` as an option, and the error names
the line that carries it -- so a typo in a project you were not
even asking about is reported while you have the file open.

## Use

```bash
bombyx doctor             # check the preconditions, change nothing
bombyx up                 # write the generated files, boot the VM
bombyx provision          # re-run provisioning in the guest
bombyx shell              # open a shell inside the VM
bombyx status             # vagrant status on the host
bombyx reset              # restore the fresh-install snapshot
bombyx down               # halt the VM
bombyx destroy myproject  # destroy the VM and remove its dir
                          # (every line above takes --project)

bombyx scratch pr-1234    # boot a throwaway VM
bombyx discard pr-1234    # destroy it

bombyx self-update        # update this binary to the newest release
```

Two lifecycles, on purpose:

- **Persistent** (`up`/`down`) for your own projects --
  warm caches, fast boots, reset by snapshot.
- **Ephemeral** (`scratch`/`discard`) for untrusted code
  -- external PRs, unfamiliar dependencies. Nothing
  survives, which is the point: malware that persists to
  survive credential rotation has nothing to persist to.

Every command accepts `--dry-run`, which prints the exact `ssh`
invocation instead of running it. Run `bombyx doctor` first on a
new host: `up` creates a directory and writes two files before
it runs `vagrant`, so without it a missing piece is reported
half-way through.

[docs/usage.md](docs/usage.md) is the full reference. It covers
why `provision` is a separate command, why `destroy` asks for
the project name, what teardown removes, how to read the
`doctor` report, and how the generated files are written --
including the quoting details that keep a config file from
running code on your workstation.

## Updating bombyx

```bash
bombyx self-update
```

**Verified end to end on 2026-08-18**, updating an installed
`0.3.0` to the published `0.4.0` on Windows 11. Tag discovery, the
per-platform archive URL, the checksum against the release's
`SHA256SUMS`, extraction, the rename-aside dance below, and the
sweep of an earlier leftover all ran in one invocation. The whole
output, unedited:

```
bombyx: updating 0.3.0 -> 0.4.0
bombyx: bombyx-v0.4.0-x86_64-pc-windows-msvc.tar.gz matches its published checksum
bombyx: removed 1 superseded binaries
bombyx: C:\Users\igor\.cargo\bin\bombyx.exe.old-41780-606660100 is still in use; the next self-update removes it
bombyx: updated to 0.4.0 in C:\Users\igor\.cargo\bin
```

Two of those sentences are quoted with their defects intact,
because **an update always runs the old binary's code** -- the file
being replaced is the one doing the replacing. So `0.3.0` wrote
them, and both have since been corrected: the plural now agrees
with its count, and the leftover notice says "the next update that
replaces the binary" rather than "the next self-update", since the
sweep runs only when an update installs something. Which means the
run above verifies `0.3.0`'s copy of this path; the two corrected
sentences have not themselves run against a real release
*(unverified)*.

Also unexercised *(unverified)*: updating on Linux or macOS, and
the release workflow's refusal to replace an already-published
version's assets, which needs a re-pushed tag to reach.

It finds the newest release tag with `git ls-remote --tags`,
downloads that release's archive for your platform with `curl`,
checks it against the release's `SHA256SUMS`, and only then
extracts the binary over the installed one. `git`, `curl` and
`tar` are the extra requirements, and no Rust toolchain is
involved.

**Verification fails closed.** A missing `SHA256SUMS`, no entry
for your platform's archive, or a digest that does not match all
refuse the update. There is no flag to skip the check: the one
outcome worse than not updating is replacing the binary with
something unverified. When a release cannot be verified, the
error prints the `cargo install --git --tag --locked` line to run
by hand instead, so you are never simply stuck.

Two things it will not do. It never installs a pre-release, so a
`v1.0.0-rc1` tag is ignored -- consistent with the release
workflow publishing those as GitHub pre-releases. And it never
downgrades: a binary you built from a bumped `Cargo.toml` is
newer than any release, and it says so rather than replacing it.

### A version number is the only thing compared

`self-update` decides by comparing `MAJOR.MINOR.PATCH`, so it has
no notion of "this version's bytes changed". If a published
release's assets were ever replaced, anyone already on that
version is told they are up to date and never receives the
replacement, and anyone mid-download sees a checksum mismatch
whose message says the bytes are not the ones that were released --
which reads as tampering when the cause was a re-publish.

So the release workflow refuses to overwrite the assets of a
release that already carries a `SHA256SUMS`, and asks for a new
patch tag instead *(unverified: reaching that refusal needs a
re-pushed tag, and none has been pushed since it landed)*. It
stays idempotent for the case that made it
idempotent in the first place: re-running a release whose upload
never finished, which is a repair rather than a redefinition. The
refusal can be overridden with a repository variable
(`ALLOW_RELEASE_REPLACE=true`), and if you use it, understand that
existing installations will not pick the change up. Cut a patch
release for that.

### Why the binary is renamed

On Windows the update renames `bombyx.exe` aside before writing
the new one, and deletes the old copy on the *next* run.

Windows refuses to overwrite the image of a running process, and
`bombyx self-update` is itself a running bombyx -- the very file
being replaced. Writing over it fails with
`Access is denied (os error 5)`. Renaming a running binary is
permitted, and the running process keeps working from the
renamed file, which is what makes updating in place possible at
all. That copy usually cannot be deleted until nothing is using
it, hence the sweep on the following run.

Those two facts -- the refused overwrite and the permitted
rename -- were measured on Windows 11 while a second bombyx held
the same file, and the dance has since run for real: the
`0.3.0` -> `0.4.0` update at the top of this section left
`bombyx.exe.old-41780-606660100` behind, held by the process doing
the updating, exactly as described here.

The directory updated is the one holding the **running**
executable, not one derived from `CARGO_HOME`. Those differ more
often than they look -- `cargo install --root`, a copy into
`~/bin`, a Scoop shim -- and writing to the wrong one would
report success while leaving the binary you actually invoke
untouched.

Unix needs none of this: replacing a path there unlinks the old
inode and leaves running processes on it, so the extraction
simply succeeds.

## Telling the VM which host it runs on

An agent working inside the VM has no way to find out which
machine is underneath it. There is no synced folder to read,
`hostname` inside the guest answers with the guest's own name,
and libvirt does not pass the host's name in at all -- the
guest's SMBIOS/DMI describes the *emulated* machine, so
`/sys/class/dmi/id/sys_vendor` reads `QEMU` and `product_name`
names a QEMU machine type. Those files are readable; they simply
hold nothing about the host, and the root-only ones
(`product_serial`) carry no host name either. There is nothing to
read at any privilege level. Once you have more than one VM
host -- a workstation's WSL2 distribution and a real machine in
the next room, say -- "where is this actually running" stops
being a rhetorical question, and a status line that cannot
answer it is a status line that will mislead you.

So bombyx puts two environment variables on every `vagrant`
invocation it makes on the host:

| Variable | Holds |
|----------|-------|
| `BOMBYX_VM_HOST` | The SSH alias you configured, e.g. `frosti` |
| `BOMBYX_VM_HOSTNAME` | What the host machine calls itself (`hostname -s`) |

Both are passed because they need not agree: an alias in your
`~/.ssh/config` can be any name you like, and often is. Show the
alias -- it is the name you chose, so it is the one you
recognise -- and keep the other for the day the two disagree and
you need to know which machine actually answered.

They can also legitimately agree. A WSL2 distribution that has
not been given a name of its own reports the Windows machine's
name, so on that kind of host `BOMBYX_VM_HOSTNAME` may equal your
workstation's. That is expected rather than a sign that something
expanded on the wrong side, and it means `BOMBYX_VM_HOST` is the
value that actually distinguishes one host from another.

They are set on every command that runs `vagrant` in a project
directory, not only the ones that provision. `halt` and `status`
have no use for them; setting them in one place is what keeps the
next command that *does* need them from being the one that was
forgotten -- `destroy` was exactly that, and it was caught in
review rather than by the tests.

`doctor` is the one exemption. Its probes run in your login
directory on the host, not a project directory, so they evaluate
no `Vagrantfile` and there is nothing there to read the values.

**The variables reach the `vagrant` process on the host, not the
guest.** Vagrant does not export its own environment into the
VM: a provisioner script runs inside the guest, under the
guest's environment, so anything from the host has to be handed
over deliberately. The `Vagrantfile` is Ruby running on the
host, so it can read them and pass them on.

**bombyx does that for you now.** This used to be the project's
job, and the README told you to write the `env:` block into your
own `Vagrantfile`. bombyx generates that file and overwrites
what the project ships, so a hand-written block would be deleted
on the next `up`. The generated file forwards both variables
into the guest alongside the `[source]` settings.

What is still the project's job is the other half: your
provisioning script decides what to do with them. It can write
the values somewhere a status line can
read them without asking for a password:

```sh
# Which machine this VM is running on. bombyx sets these two
# variables on the vagrant invocation; nothing inside the guest
# can work the answer out for itself.
sudo mkdir -p /etc/bombyx
printf 'host=%s\nhostname=%s\n' \
  "${BOMBYX_VM_HOST:-unknown}" "${BOMBYX_VM_HOSTNAME:-unknown}" \
  | sudo tee /etc/bombyx/vm-host > /dev/null
```

The defaults on both sides matter: a VM booted by a bare
`vagrant up` on the host, rather than through bombyx, sees
neither variable. Writing `unknown` is more use than an empty
value that reads as a broken script.

Provisioners run when the machine is first created and on
`bombyx provision`, so an existing VM picks the file up on the
next `bombyx provision` rather than on the next boot.

A host with no `hostname` command leaves `BOMBYX_VM_HOSTNAME`
empty rather than failing the boot. Refusing to start a VM over
a status line would be the wrong trade.

What has been checked, and what has not.

Both variables were confirmed to arrive at a real `Vagrantfile`'s
Ruby on a live host, and to be absent without the prefix. On that
host `hostname -s` answered with a name different from the
workstation's, which is what proves `$(hostname -s)` runs on the
far side rather than here. That check works because those two
names differ; on a WSL2 host that reports the Windows machine's
name it would prove nothing, per the note above.

That the guest's DMI holds nothing about the host was measured
inside a running guest as the unprivileged user.

The `env:` hand-off into the guest and the `provision.sh` line
above are Vagrant's ordinary shell-provisioner behaviour and have
**not** been exercised end to end in a booted VM *(unverified)*.

## Development

```bash
cargo xtask validate      # full quality gate
cargo xtask test [filter] # tests only
cargo run -p bombyx -- --dry-run up
```

```powershell
.\build.ps1 validate      # same, on Windows
```

Gates: clippy pedantic with zero warnings, 90% coverage
(85% per module), <= 6% duplication, RUSTSEC clean, and a
14-day dependency cooldown.

## Status

Early, but no longer untested against reality. Every
command has been driven against a real libvirt host
(Ubuntu 24.04, Vagrant 2.4.9, vagrant-libvirt 0.12.2):
`doctor`, `up`, `provision`, `shell`, `status`, `down`,
`reset`, `destroy`, `scratch` and `discard`, including
repeat runs for idempotency and a rejected traversal.

Two things are still worth knowing. `reset` restores a
`fresh-install` snapshot that no bombyx command creates,
so it fails on a VM you have not snapshotted by hand.
And `scratch` boots from the same base box as everything
else, so a throwaway VM costs a full boot until there is
a pre-baked image.

## Origin

Derived from the [rustbase](https://github.com/breki/rustbase)
template; see `.template-sync.toml` for the commit this
project was created from and `/template-sync` to pull
upstream improvements.

## License

MIT -- see [LICENSE](LICENSE).
