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
