# bombyx

Run AI coding agents in isolated VMs on a libvirt host, driven from
your workstation over SSH.

*Bombyx mori* is the domesticated silkworm, the animal that spins the
cocoon. The tool builds the enclosure; the agent works inside it.

![A Bombyx mori silk moth (macro of the head)](docs/images/bombyx-moth.jpg)

<sub>Photo by [CSIRO][moth-src], [CC BY 3.0][moth-lic].</sub>

[moth-src]: https://commons.wikimedia.org/wiki/File:CSIRO_ScienceImage_10746_An_adult_silkworm_moth.jpg
[moth-lic]: https://creativecommons.org/licenses/by/3.0/

## Why

An AI coding agent on your own machine can read whatever you can: your
SSH keys, cloud credentials, password manager and browser profiles.
One prompt injection or one malicious `postinstall` is enough to send
them somewhere. The defence is a VM with its own kernel, none of your
files, and none of your credentials -- ideally on a machine that is
not your laptop.

bombyx is the control plane for that setup. It runs `vagrant` on a
libvirt VM host, either a separate machine over SSH or this one
directly. It generates the files that host needs, and you stay on
your workstation the whole time.

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

No Rust toolchain needed -- each release carries a prebuilt binary.
Download the archive for your platform from the
[releases page](https://github.com/breki/bombyx/releases), check it
against `SHA256SUMS`, and put the binary on your `PATH`.
[docs/quickstart.md](docs/quickstart.md) gives the exact commands
for Linux, macOS and Windows, with the verification detail. After
the first install, `bombyx self-update` does the download and
verify for you.

That installs the CLI on your workstation. The VM host needs libvirt,
Vagrant and its `libvirt` provider --
[docs/vm-host-setup.md](docs/vm-host-setup.md) covers preparing one,
and [docs/vm-host-wsl2.md](docs/vm-host-wsl2.md) covers using a WSL2
distribution on a Windows machine instead.

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
script = ".bombyx/provision.sh"
```

`[vm]` describes the machine to boot; `[source]` is the repository the
guest clones and the script it runs. Both are required. Optional keys
handle private repositories and secrets --
`config.toml.sample` documents `deploy_key`, `env_file` and
`repo_token`, and [docs/trust-boundary.md](docs/trust-boundary.md)
explains what putting a credential in the VM costs.

Name the project on every command but `list`: `bombyx --project
myproject up`.

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

bombyx keeps two lifecycles separate on purpose:

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
[docs/quickstart.md](docs/quickstart.md) shows the manual
download-and-verify that `self-update` automates.

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

Still in alpha, under active development.

## License

MIT -- see [LICENSE](LICENSE).
