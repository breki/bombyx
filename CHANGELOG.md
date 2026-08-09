# Changelog

All notable changes to this project will be documented
in this file.

The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial scaffold, derived from the
  [rustbase](https://github.com/breki/rustbase) template
  at `f40582f` (v0.17.0), pruned to a CLI-only project.
- `bombyx.toml` project configuration with typed errors
  and remote path resolution (`config` module).
- SSH/scp command construction with POSIX shell quoting
  (`remote` module); commands are built, never spawned,
  so they are testable without a VM host.
- Subcommands `up`, `down`, `shell`, `status`, `reset`,
  `scratch <name>`, `discard <name>`, plus a global
  `--dry-run` that prints the argv instead of running it.
- Pushes ship a tar archive (`tar -czf ... -C <dir> .`,
  `scp`, remote `tar -xzf`) rather than `scp -r`, which
  copies *into* an existing destination and would nest the
  Vagrant directory one level deeper on every push.
  `rsync` was rejected: it is absent on a stock Windows
  workstation, which is where bombyx runs.
- The Vagrant directory is pushed into the same directory
  `vagrant` is then run in, so `up` finds a Vagrantfile.
- `scratch` pushes before booting; it would otherwise run
  `vagrant up` in an empty directory.
- Config and CLI input is validated against an allowlist before it reaches a
  command line. `bombyx.toml` travels inside a repo, so a `host` of
  `-oProxyCommand=...` would otherwise be read by `ssh` as an option and run
  code on the workstation; a scratch name must be a single path segment, so
  `../../etc` is refused rather than quoted into a traversal.
- Remote paths keep a leading `~` outside the quotes (`~/'vms/phren'`). A POSIX
  shell does not expand `~` inside single quotes, so a fully quoted path created
  a directory literally named `~` while `scp` wrote to the real home directory
  -- the two halves of `up` targeted different places.
- The push archive gets a per-run name in a private temporary directory, and
  `tar` and `scp` run in that directory with a bare file name. This keeps
  concurrent runs from colliding, keeps a co-user from pre-creating the path,
  and keeps a Windows drive letter (`C:\...`) out of `scp`, which would read it
  as a host name.
- The push excludes `.vagrant/` and `.git/`. `.vagrant/` holds the VM's identity
  on the host, so shipping a local copy orphaned the running VM.
- Remote archive cleanup runs whether or not extraction succeeded, so a corrupt
  archive is not left in the directory `vagrant up` runs in, and the failing
  exit code is still propagated.
- A failing remote command's exit code is passed through instead of being
  flattened to 1, so `bombyx status` stays scriptable and an `ssh` transport
  failure stays distinguishable from what `vagrant` returned.
- Scratch VMs are scoped per project (`<remote_root>/scratch/<project>/<name>`),
  so the same scratch name in two projects no longer resolves to one directory.
