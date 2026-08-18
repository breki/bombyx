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
  - [Why `host` is not in `bombyx.toml`](#why-host-is-not-in-bombyxtoml)
  - [Where bombyx looks for the host](#where-bombyx-looks-for-the-host)
  - [Per-project overrides](#per-project-overrides)
- [Use](#use)
- [Updating bombyx](#updating-bombyx)
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
kernel, no host filesystem, and no credentials -- on a
machine that is not your laptop.

bombyx is the control plane for that setup: it runs
`vagrant` on the VM host over SSH so you can stay on your
workstation.

## Model

```
workstation                  vmhost (VM host)
  bombyx  ──── ssh ────►  vagrant ──► agent VM
     │                                    │
     └── pushes vagrant/ ─────────────────┘
```

Two rules shape the design:

1. **The repo is the source of truth.** Each project keeps
   its `vagrant/` directory in its own repo. `bombyx up`
   pushes it to the host before booting, so the host holds
   a cache that cannot silently drift.
2. **Wrap, don't reimplement.** bombyx composes `ssh`,
   `scp` and `vagrant`. If it breaks, `ssh vmhost` and
   `vagrant up` by hand still work.

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

If this is your first setup, follow
[docs/tutorial.md](docs/tutorial.md) instead of the sections
below. It goes through all three pieces in order -- the
workstation, the VM host, and a sample project with a
`Vagrantfile` and a provisioning script -- and ends with a VM
you can open a shell into.

## Configure

There are two pieces, and the split is the point. The project
file describes the *project* and is committed. Which machine
runs the VMs is yours, and is configured once, outside any
repo.

Drop a `bombyx.toml` in the project you want a VM for:

```toml
project = "myproject"    # VM + directory name on the host

# optional, shown with defaults
vagrant_dir = "vagrant"  # dir in this repo with the Vagrantfile
remote_root = "~/vms"    # root on the host for project dirs
```

Then name your own VM host once, in a file outside the repo:

```toml
# ~/.config/bombyx/config.toml
# Windows: %APPDATA%\bombyx\config.toml
host = "my-vmhost"
```

`host` is an SSH alias, resolved through your `~/.ssh/config` --
bombyx never handles addresses, usernames or keys itself.

### Why `host` is not in `bombyx.toml`

**A `host` key in `bombyx.toml` is refused, not ignored.** A
project is shared; a VM host is not. Every developer has their
own hardware on their own network, so a committed `host` can
only ever be right for the person who wrote it, and is wrong
for everyone who clones after them. That is not a cosmetic
problem: `bombyx destroy` runs `vagrant destroy` and `rm -rf`
on whatever host is in force.

Refusing it also keeps the value out of reach of a cloned
repo. `host` is handed to `ssh` as its first argument, and
`ssh` reads a leading `-` as an option -- so a repo shipping
`host = "-oProxyCommand=..."` used to be able to run code on
your workstation from a bare `bombyx status`. The charset check
that stopped that is still there, but the value no longer
arrives from the repo at all.

### Where bombyx looks for the host

Four sources, first match wins:

| | Source | Use |
|-|--------|-----|
| 1 | `--host vmhost-b` | a one-off run |
| 2 | `BOMBYX_HOST=vmhost-b` | a shell, CI, or an agent |
| 3 | `bombyx.local.toml` | this project only; gitignore it |
| 4 | your `config.toml` | every project -- the usual one |

If none of them names a host, bombyx stops and lists all four
rather than guessing.

**This section is the authoritative list** -- the sample config
and `llms.txt` point here rather than repeating it, so there is
one place to correct when it changes.

Your `config.toml` lives in the first of these that the
environment names:

| Variable | Config file |
|----------|-------------|
| `BOMBYX_CONFIG_HOME` | `$BOMBYX_CONFIG_HOME/config.toml` |
| `%APPDATA%` (Windows only) | `%APPDATA%\bombyx\config.toml` |
| `XDG_CONFIG_HOME` | `$XDG_CONFIG_HOME/bombyx/config.toml` |
| `HOME` | `$HOME/.config/bombyx/config.toml` |

`BOMBYX_CONFIG_HOME` is there for keeping two setups apart.
`%APPDATA%` is consulted **only** on Windows: it is often
exported under WSL and Wine too, and honouring it there would
read a Windows config directory in preference to
`$HOME/.config`.

Each of those values must be an anchored path. A blank or
relative one counts as unset and the next row is tried, because
a relative path resolves against the working directory -- which
on this tool means taking the VM host out of whatever repo you
happen to be in. An exported-but-empty `BOMBYX_HOST` counts as
unset for the same reason: that is what a shell script means by
it.

### Per-project overrides

`bombyx.local.toml`, beside the project file and gitignored,
overrides any field for one project -- a second VM host for
one repo, a different `remote_root` on one machine:

```toml
host = "other-vmhost"    # just this project
remote_root = "/srv/vms"
```

Every field is optional there, and only the ones present are
replaced. The file is optional too -- most projects never need
one. The name is derived from the config's, so
`--config staging.toml` looks for `staging.local.toml` and the
override is always named after what it overrides.

bombyx prints one line to stderr when an override file is in
force, so the two states are distinguishable without opening
either file. It prints a second line naming where the host came
from, unless that was your own `config.toml` -- the ordinary
case, and not worth a line on every command. Between them, the
host that `destroy` would run `rm -rf` on is always visible
without reasoning about precedence.

Validation runs *after* the merge, so an override is subject to
exactly the same checks as the committed file rather than being
a way around them. That includes the rule that `vagrant_dir`
must stay inside the project: it is joined onto the working
directory, and an absolute path would silently replace it,
making `up` archive somewhere else entirely.

## Use

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

bombyx self-update        # update this binary to the newest release
```

Two lifecycles, on purpose:

- **Persistent** (`up`/`down`) for your own projects --
  warm caches, fast boots, reset by snapshot.
- **Ephemeral** (`scratch`/`discard`) for untrusted code
  -- external PRs, unfamiliar dependencies. Nothing
  survives, which is the point: malware that persists to
  survive credential rotation has nothing to persist to.

Every command accepts `--dry-run`, which prints the exact
`ssh`/`scp` invocation instead of running it. Run `bombyx
doctor` first on a new host: `up` creates a directory and ships
a tarball before it runs `vagrant`, so without it a missing
piece is reported half-way through.

[docs/usage.md](docs/usage.md) is the full reference. It covers
why `provision` is a separate command, why `destroy` asks for
the project name, what teardown removes, how to read the
`doctor` report, and how the push is built -- including the
quoting details that keep a config file from running code on
your workstation.

## Updating bombyx

```bash
bombyx self-update
```

**This path has not been run end to end yet *(unverified)*.** No
published release carries a `SHA256SUMS`, because the workflow
that attaches one landed after `v0.2.0`, so the first version this
can actually update *to* is the next one. Run against a release
published before that, it correctly refuses and prints the manual
`cargo install` line. The pieces below are each verified -- the
digest against the SHA-256 specification's own test vectors, the
`tar` and `curl` invocations against real archives and a real
404 -- but their composition has never completed a single update.

It finds the newest release tag with `git ls-remote --tags`,
downloads that release's archive for your platform with `curl`,
checks it against the release's `SHA256SUMS`, and only then
extracts the binary over the installed one. `curl` and `tar` are
the only extra requirements, and no Rust toolchain is involved.

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
patch tag instead. It stays idempotent for the case that made it
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
the same file. The *update as a whole* has not been, per the note
at the top of this section.

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

Getting them the rest of the way is the project's job, because
the project owns its `Vagrantfile` and its provisioning script.
It takes two steps, and the first one is easy to miss.

**The variables reach the `vagrant` process on the host, not the
guest.** Vagrant does not export its own environment into the
VM: a provisioner script runs inside the guest, under the
guest's environment, so anything from the host has to be handed
over deliberately. The `Vagrantfile` is Ruby running on the
host, so it can read them and pass them on:

```ruby
config.vm.provision "shell",
  path: "provision.sh",
  privileged: false,
  env: {
    "BOMBYX_VM_HOST"     => ENV.fetch("BOMBYX_VM_HOST", "unknown"),
    "BOMBYX_VM_HOSTNAME" => ENV.fetch("BOMBYX_VM_HOSTNAME", "unknown"),
  }
```

If the provisioner already has an `env:` hash, merge into it
rather than adding a second one -- a repeated key silently wins
over the earlier value, and the variable that disappears is the
one nobody was looking at.

Then the script writes the values somewhere a status line can
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
