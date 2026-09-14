# bombyx

Run AI coding agents in isolated VMs on a libvirt host, driven from
your workstation over SSH.

*Bombyx mori* is the domesticated silkworm, the animal that spins the
cocoon. The tool builds the enclosure; the agent works inside it.

## Why

An AI coding agent on your own machine can read whatever you can: your
SSH keys, cloud credentials, password manager and browser profiles.
One prompt injection or one malicious `postinstall` is enough to send
them somewhere. The defence is a VM with its own kernel, none of your
files, and none of your credentials -- ideally on a machine that is
not your laptop.

bombyx is the control plane for that setup. It runs `vagrant` on a
libvirt VM host -- another machine over SSH, or this one directly --
and generates the files that machine needs, so you stay on your
workstation. It wraps `ssh` and `vagrant` rather than reimplementing
them: if bombyx breaks, `ssh vmhost` and `vagrant up` by hand still
work.

For what a VM can and cannot protect -- including the one credential
that has to live inside the guest -- see
[docs/trust-boundary.md](docs/trust-boundary.md).

## How it works

```
workstation                  vmhost (VM host)
  bombyx  ──── ssh ────►  vagrant ──► agent VM
     │                          ▲          │
     └── writes Vagrantfile ────┘          │
         and bootstrap.sh                  │
                                  clones the repo itself
```

bombyx sends the VM host two generated files: a Vagrantfile built from
your VM settings, and a bootstrap script. Neither your workstation nor
the VM host reads your project -- once the VM is up, the guest clones
the project itself.

[docs/architecture.md](docs/architecture.md) walks through the three
machines and what `bombyx up` does step by step.

## Install

```bash
cargo install --path crates/bombyx
```

That installs the CLI on your workstation. The VM host needs libvirt,
Vagrant and its `libvirt` provider --
[docs/vm-host-setup.md](docs/vm-host-setup.md) covers preparing one,
and [docs/vm-host-wsl2.md](docs/vm-host-wsl2.md) covers using a WSL2
distribution on your own Windows machine instead.

New here? [docs/tutorial.md](docs/tutorial.md) builds a working setup
from nothing; [docs/quickstart.md](docs/quickstart.md) is the short
path when you already have a repository.

## Configure

bombyx reads one file, and it is yours -- nothing lives in the
project's own repository. Copy
[config.toml.sample](config.toml.sample) into your config directory
(`~/.config/bombyx/config.toml`, or `%APPDATA%\bombyx\config.toml` on
Windows) and edit it. A test loads that sample as shipped, so it
cannot drift from what bombyx accepts.

The file names the machine your VMs run on, then carries one table per
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

`[vm]` describes the machine to boot; `[source]` is the repository the
guest clones and the script it runs. Both are required. Optional keys
handle private repositories and secrets --
[docs/usage.md](docs/usage.md) covers `deploy_key`, `env_file`,
`repo_token`, and where bombyx looks for this file.

Name the project on every command but `list`: `bombyx --project
myproject up`. bombyx reads nothing from the project's directory, so it
cannot guess which project you mean from where you are standing.

## Use

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
                          # (every line above takes --project)

bombyx scratch pr-1234    # boot a throwaway VM
bombyx discard pr-1234    # destroy it

bombyx list               # every project and its VM state
bombyx self-update        # update this binary to the newest release
```

Two lifecycles, on purpose:

- **Persistent** (`up`/`down`) for your own projects -- warm caches,
  fast boots, reset by snapshot.
- **Ephemeral** (`scratch`/`discard`) for untrusted code such as
  external PRs -- nothing survives, which is the point.

Every command accepts `--dry-run`, which prints the exact `ssh`
invocation instead of running it. Run `bombyx doctor` first on a new
host. [docs/usage.md](docs/usage.md) is the full reference.

## Updating

```bash
bombyx self-update
```

This finds the newest release tag, downloads the archive for your
platform, checks it against the release's `SHA256SUMS`, and only then
replaces the installed binary. Verification fails closed: a missing or
mismatched checksum refuses the update, and there is no flag to skip
it. It never installs a pre-release and never downgrades.
[docs/usage.md](docs/usage.md) covers the rest.

## Development

```bash
cargo xtask validate      # full quality gate
cargo xtask test [filter] # tests only
cargo run -p bombyx -- --dry-run up
```

```powershell
.\build.ps1 validate      # same, on Windows
```

Gates: clippy pedantic with zero warnings, 90% coverage (85% per
module), <= 6% duplication, RUSTSEC clean, and a 14-day dependency
cooldown.

## Status

Early, but tested against reality. Every command that drives a VM has
been run against a real libvirt host (Ubuntu 24.04, Vagrant 2.4.9,
vagrant-libvirt 0.12.2). Those runs took the local route, where the VM
host is the workstation, so the `ssh` spelling still rests on
`--dry-run`, and no provider but libvirt has been tried. `self-update`
is not on that list because it talks to GitHub rather than a VM host.

## Origin

Derived from the [rustbase](https://github.com/breki/rustbase)
template; see `.template-sync.toml` for the commit this project was
created from, and `/template-sync` to pull upstream improvements.

## License

MIT -- see [LICENSE](LICENSE).
