# bombyx

Drive isolated AI-agent VMs on a remote libvirt host over
SSH.

*Bombyx mori* is the domesticated silkworm -- the animal
that spins the cocoon. The tool builds the enclosure; the
agent works inside it.

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
perun (workstation)          frosti (VM host)
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
   `scp` and `vagrant`. If it breaks, `ssh frosti` and
   `vagrant up` by hand still work.

## Install

```bash
cargo install --path crates/bombyx
```

## Configure

Drop a `bombyx.toml` in the project you want a VM for:

```toml
host = "frosti"          # SSH host alias of the VM host
project = "phren"        # VM + directory name on the host

# optional, shown with defaults
vagrant_dir = "vagrant"  # dir in this repo with the Vagrantfile
remote_root = "~/vms"    # root on the host for project dirs
```

`host` is an SSH alias, resolved through your
`~/.ssh/config` -- bombyx never handles addresses,
usernames or keys itself.

## Use

```bash
bombyx up                 # push vagrant/, boot the VM
bombyx shell              # open a shell inside the VM
bombyx status             # vagrant status on the host
bombyx reset              # restore the fresh-install snapshot
bombyx down               # halt the VM

bombyx scratch pr-1234    # boot a throwaway VM
bombyx discard pr-1234    # destroy it
```

A scratch VM lives in `<remote_root>/scratch/<project>/<name>`,
so the same name in two projects does not collide.

Two lifecycles, on purpose:

- **Persistent** (`up`/`down`) for your own projects --
  warm caches, fast boots, reset by snapshot.
- **Ephemeral** (`scratch`/`discard`) for untrusted code
  -- external PRs, unfamiliar dependencies. Nothing
  survives, which is the point: malware that persists to
  survive credential rotation has nothing to persist to.

Every command accepts `--dry-run`, which prints the exact
`ssh`/`scp` invocation instead of running it:

```console
$ bombyx --dry-run up
ssh frosti "mkdir -p ~/'vms/phren'"
cd /tmp/.tmpAL8i && tar -czf .bombyx-push-4821-729551000.tar.gz -C /repo/vagrant --exclude=./.vagrant --exclude=./.git .
cd /tmp/.tmpAL8i && scp .bombyx-push-4821-729551000.tar.gz frosti:.bombyx-push-4821-729551000.tar.gz
ssh frosti "{ cd ~/'vms/phren' && tar -xzf ~/'.bombyx-push-4821-729551000.tar.gz'; }; rc=\$?; rm -f ~/'.bombyx-push-4821-729551000.tar.gz'; exit \$rc"
ssh frosti "cd ~/'vms/phren' && vagrant 'up'"
```

The output is real shell: each argument is printed bare only
when it is unambiguous, and quoted otherwise, so what you
read is what runs.

The push ships a tar archive rather than using `scp -r`,
which copies *into* an existing destination and would nest
the directory one level deeper on every push. Extracting a
tar overwrites in place, so repeated pushes are idempotent.
`rsync` would also work but is not present on a stock
Windows workstation; `tar`, `scp` and `ssh` are.

Details that look fussy and are not:

- **The tilde sits outside the quotes** (`~/'vms/phren'`). A
  POSIX shell does not expand `~` inside single quotes, so
  the obvious `'~/vms/phren'` would create a directory
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

`bombyx.toml` travels inside a repo, so it is treated as
untrusted input: `host` must look like an SSH alias (a value
starting with `-` would otherwise be read by `ssh` as an
option such as `-oProxyCommand=...`, running code on your
workstation), and a scratch name must be a single path
segment, so `../../etc` is refused rather than quoted.

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

Early. The command surface works and is covered by tests,
but it has not yet been driven against a real VM host --
`--dry-run` proves the argv, not that the remote side
accepts it.

## Origin

Derived from the [rustbase](https://github.com/breki/rustbase)
template; see `.template-sync.toml` for the commit this
project was created from and `/template-sync` to pull
upstream improvements.

## License

MIT -- see [LICENSE](LICENSE).
