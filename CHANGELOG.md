# Changelog

All notable changes to this project will be documented
in this file.

The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The generated Vagrantfile runs a bootstrap script inside the guest that clones
  `source.repo` at `source.ref` and runs `source.script` from the clone. A
  private repository needs a credential inside the guest; see
  `docs/trust-boundary.md`.
- `[source]` values are checked before they reach the guest: none may look like
  a `git` option, `repo` must be a real URL rather than a `<transport>::<rest>`
  remote helper such as `ext::` (which runs a command), and `script` must stay
  inside the clone.
- The guest refuses a `source.script` that resolves outside the cloned project.
  `chmod` and `exec` follow symlinks, so a repository could otherwise point the
  script at a system file and have it made executable as root.
- The per-developer `config.toml` carries a `[projects.<name>]` table per
  project -- `remote_root`, `[vm]` and `[source]` -- and it is the only place a
  project is described. `--project <name>` picks the table. `[vm]` needs
  `provider`, `box`, `cpus` and `memory`; `[source]` needs `repo`, `ref` and
  `script`. None of the seven has a default, so bombyx guesses neither a base
  image nor a repository to clone.
- Library API: `config::Registry`, `config::Project` and `name::ProjectName`,
  plus a `ConfigError::ProjectNotFound` variant for a registry with no entry for
  the project asked for.
- A `[projects.<name>]` table accepts an optional `host`, naming the machine
  that one project runs on. It outranks the file-wide `host`, and bombyx prints
  a line on stderr naming the table whenever it wins.
- Library API: a `HostOrigin::ProjectEntry` variant carrying the project name.
- Library API: `Config::load_project(name, registry)` is the one loader. It
  reads the `[projects.<name>]` table out of the file `registry` names and
  returns a `(Config, HostOrigin)` pair. New alongside it:
  `config::registry_file()`, which is the path bombyx reads when `--config`
  names none, and a `ConfigError::RegistryNotFound` variant for a machine with
  no registry file at all.
- `--project`, `--config` and `--dry-run` are global arguments, so they are
  accepted after the subcommand as well as before it: `bombyx status --dry-run`
  works where it used to be an argument error.
- When the config file's `host` names the machine bombyx is running on, bombyx
  runs each command through `sh -c` instead of `ssh`, so a workstation that is
  its own VM host needs no SSH server, no key authorized to your own account and
  no loopback alias. The two names must be equal, ignoring case and nothing
  else, so `frosti.lan` gets `ssh` from a machine calling itself plain
  `frosti` -- a bare label is shared too easily for a looser rule to be safe.
  bombyx reads the name only, never `~/.ssh/config`, so an alias pointing at
  loopback still takes the `ssh` route, and an alias named exactly what this
  machine is named is believed; write that one as `user@name` to force `ssh`.
  Windows never takes the local route, since it cannot run libvirt. On the
  local route bombyx clears `VAGRANT_CWD`, `VAGRANT_VAGRANTFILE` and
  `VAGRANT_DOTFILE_PATH` before each script, since `sh -c` inherits the
  environment where `ssh` would not and all three redirect which machine
  vagrant acts on. The local route is announced on stderr on every command, and
  `bombyx doctor` shows its `ssh` and `login shell` rows as skips.

### Changed

- **BREAKING:** bombyx generates the Vagrantfile and writes it on the VM host. A
  project's own Vagrantfile is never read by anything: bombyx does not send it,
  and the guest's clone is not what Vagrant boots from. Vagrant needs that file
  before the VM exists, which is why it cannot come from inside the guest.
- `--dry-run` prints the two generated files as one line each, naming the
  heredoc and how many lines it dropped. The full content is still written to
  the host.
- `bombyx doctor` no longer reports on the project's `Vagrantfile` at all.
  bombyx generates that file and never reads a project-supplied one.
- The generated Vagrantfile disables Vagrant's default `/vagrant` synced folder.
  Leaving it on mounts the VM host's copy of the directory into the guest, and
  hangs on a VM host whose firewall drops guest-initiated NFS.
- bombyx forwards `BOMBYX_VM_HOST` and `BOMBYX_VM_HOSTNAME` into the guest
  itself. This used to be the project's job in its own Vagrantfile; since bombyx
  now overwrites that file, a hand-written block would be deleted on the next
  `up`.
- A bad `repo` or `script` in a project's entry is refused while the file is
  being read rather than after, so the message names the line and column as well
  as the field and the reason. The rules themselves are unchanged.
- Config values are refused when they begin or end with whitespace. `box`,
  `ref`, `repo` and `script` all reach either the generated Vagrantfile or a
  command line in the guest, where a stray space fails obscurely and late.
- Changing `source.repo` and re-provisioning now re-clones from scratch.
  Fetching over the old clone left files only the previous repository had, so
  the guest could run the old repository's provisioning script and report
  success.
- The message refusing a config value that starts with `-` is worded so it reads
  correctly whichever program it names. It said "which ssh and scp reads as an
  option".
- The message refusing a shallow `remote_root` says what the rule requires: at
  least one directory below `/` or `~`. It said "at least 1 directory deep",
  which reads as a constraint on the wrong thing.
- The help for `bombyx provision` says what a re-provision destroys inside the
  guest.
  The checkout is forced, so it overwrites edits to tracked files, and an
  untracked file too when the fetched commit adds one at the same path. It also
  detaches HEAD, so a commit made in the guest lands on no branch after the
  next provision. The help previously said the script runs from a "fresh
  clone", which reads as losing everything, and then that untracked files
  survive, which reads as a guarantee.
- config.toml.sample is the only full config example. README.md,
  docs/tutorial.md and llms.txt point at it instead of restating it, so the
  copies can no longer disagree -- four of them were unloadable at once a week
  ago. A test loads the sample as shipped.
- **BREAKING:** The `place` field on `ConfigError::HostMissing` was named
  `places`. Only one file can carry a VM host now, so the plural named something
  that no longer exists; a caller matching on that variant by field name must
  rename it.
- **BREAKING:** A project name is capped at 64 characters, the cap scratch VM
  names already had. It shares the segment check with scratch names, so one
  name cannot be legal in one place and illegal in the other.
- **BREAKING:** `config::HostOrigin` is no longer `Copy`, and it now has two
  variants rather than four: `ProjectEntry` and `UserFile`. A caller matching it
  exhaustively or relying on the copy must be updated.
- Reading the `config.toml` now checks every `host` in it -- the file-wide key
  and every `[projects.<name>].host` -- and refuses the file if any is a value
  `ssh` would misread, naming the table it came from. So a typo in a project you
  were not asking about is reported while you have the file open. That one pass
  is the only place the rule runs; nothing downstream re-checks the winner.
- **BREAKING:** Every VM subcommand now requires `--project <name>`, naming the
  `[projects.<name>]` table it acts on. bombyx reads nothing out of the
  project's own directory, so it cannot work the project out from where you ran
  it. `bombyx self-update` needs neither that argument nor a config, as before.
- **BREAKING:** `--config` now names your registry file and defaults to
  `config.toml` in your config directory. It used to default to `bombyx.toml` in
  the working directory.
- The message when no host is configured asks for a `host` line in the registry,
  and names that file. It used to list the flag and the environment variable
  too.
- **BREAKING:** `Config` gains a private `transport` field, read through
  `Config::transport()`. A struct literal naming every field no longer
  compiles outside the crate; use `Config::load_project`. This stops a caller
  choosing the route, which is the point -- the route is derived from `host`
  and is not a setting. It does not stop a caller assigning to `host` on a
  loaded `Config`, so the two can still be made to disagree that way; the
  public fields are a known gap, tracked as
  `newtype-remaining-config-fields`.

### Fixed

- A `remote_root` of `~name` was accepted as an absolute path and then sent to
  the VM host as a relative one, resolved against the SSH login directory. It
  must now start with `/` or `~/`.
- The sample config in README.md, docs/tutorial.md and the sample file could
  not be loaded. All three wrote remote_root after the [source] table, and TOML
  binds a bare key to the table above it, so it parsed as source.remote_root and
  every command failed while reading the file. The sample file also still
  carried the removed vagrant_dir key and had no [vm] or [source] table.
- `bombyx doctor` no longer sends the `vagrant-libvirt` probe to a project
  whose provider is `hyperv`. Hyper-V ships inside Vagrant and has no plugin to
  find, so the row reported a missing plugin that project never needed. Such a
  project now gets a `provider` row reading `skip`, because bombyx has never
  driven a Hyper-V host and has no probe to write for one -- an absent row
  would read as a check that passed.
- The `llms.txt` sample config could not be loaded either: it still named the
  removed `vagrant_dir` key and had no `[vm]` or `[source]` table.
- bombyx doctor counts skipped checks in its closing line. A report whose only
  non-pass was a skip ended with "all checks passed", which is the reading the
  skip row exists to prevent.
- The tutorial told readers an empty directory would do for trying bombyx out.
  The guest clones source.repo over the network, so a project that was never
  pushed fails at clone time inside the VM. It also offered VirtualBox as a
  provider, which the config refuses: bombyx accepts libvirt and hyperv.
- bombyx self-update needs git as well as curl and tar -- it finds the newest
  release tag with git ls-remote. The clap help and README listed only two of
  the three.

### Removed

- **BREAKING:** bombyx no longer pushes the project directory to the VM host.
  The generated Vagrantfile disables the `/vagrant` share, so no program on the
  host or in the guest read the pushed files. `bombyx up` is now four `ssh`
  commands instead of seven, and bombyx runs nothing on the workstation.
- **BREAKING:** the `vagrant_dir` config key. It existed only to tell the push
  what to archive, and there is nowhere left to write it: the config refuses
  unknown keys, so a `[projects.<name>]` table carrying one makes every command
  fail while loading, with a message naming the line.
- `bombyx doctor` checks one local program, `ssh`, and no longer reports on a
  project `Vagrantfile`. The `tar` and `scp` rows are gone, locally and on the
  host: bombyx runs neither for any VM command. `bombyx self-update` still
  needs `git`, `curl` and `tar`, and `doctor` deliberately says nothing about
  them. On a machine without `tar`, `doctor` used to exit 1 while every VM
  command worked.
- **BREAKING:** The doctor::run_probes and doctor::provider_finding library
  functions. doctor::host_findings composes them and is the supported entry
  point; making the two pub(crate) is what stops a caller assembling a report
  with no provider row.
- **BREAKING:** `bombyx.local.toml`. bombyx no longer reads that file. A
  leftover one is inert: it is never opened, its contents cannot win and cannot
  fail to parse, and bombyx says nothing about it. Move its `host` line into
  the project's own table in your `config.toml`, then **delete the file** -- it
  is no longer gitignored, so a `git add -A` would commit the host name it
  holds.
- **BREAKING:** The `bombyx: bombyx.local.toml overrides bombyx.toml` line on
  stderr is gone with the file. Anything grepping bombyx's stderr for
  `overrides` stops matching.
- **BREAKING:** The `Overlay` and `local_config_path` library items, the
  `HostOrigin::Overlay` enum variant, and the `Config::with_overlay` method.
  `HostOrigin` is not `#[non_exhaustive]`, so a downstream `match` over it must
  drop the arm. A downstream that read project values through an overlay has no
  replacement and should read them out of the registry.
- **BREAKING:** `bombyx.toml`, and the committed project file as a concept.
  Every setting -- `remote_root`, `[vm]` and `[source]` -- moves into a
  `[projects.<name>]` table in your own `config.toml`, and the project name
  becomes the table key rather than a `project` key. `config.toml.sample` is the
  worked example; the sample file was `bombyx.toml.sample`.
- **BREAKING:** `--host` and the `BOMBYX_HOST` environment variable. A one-off
  flag could point `destroy`'s `rm -rf` at a machine the project never named, so
  the VM host is now tied to the project: the file-wide `host` covers every
  project and a `host` inside one project's table overrides it for that project
  alone.
- **BREAKING:** Library API: `Config::load`, `config::HostSources`,
  `config::HOST_ENV`, the `HostOrigin::Flag` and `HostOrigin::Env` variants, and
  the `ConfigError::NotFound` and `ConfigError::HostInProjectFile` variants.
  `Config::load_project(name, registry)` replaces the loader, and neither error
  type is `#[non_exhaustive]`, so a downstream `match` must drop those arms.


## [0.4.1] - 2026-08-18

### Added

### Changed

### Fixed

- self-update's sweep notice agrees with its own count. The first real update
  printed "removed 1 superseded binaries"; the wording moved into the library
  beside the count it describes, where a test covers zero, one and many. Its
  sibling, the leftover-binary notice, moved with it for the same reason, so
  both of the sentences reporting an update's cleanup are now tested rather
  than written where the coverage gate cannot see them.
- Output no longer staircases on a Windows console. Every line was starting at
  the column where the previous one ended, because nothing emitted a carriage
  return. Two causes: the remote's stdout was a pipe, so its tty never
  translated LF to CRLF; and bombyx's own doctor table staircased as well, which
  happens after a command that runs ssh and never in self-update -- the console
  is being left in a state where a line feed does not return the carriage,
  though exactly what leaves it that way is unverified. Windows runs with both
  streams on a terminal now ask ssh for a pseudo-terminal, and bombyx's own
  multi-line output gets CRLF per stream. Piped or redirected output is
  unchanged, byte for byte, so captured logs gain no carriage returns and no
  colour codes; Linux and macOS are untouched, since a Unix terminal needs no
  translation and a pseudo-terminal would only fold the remote's stderr into
  stdout.

### Removed

## [0.4.0] - 2026-08-18

### Added

- Licence checking, in two halves. `cargo xtask deny` gates licences, banned
  crates and registry sources with cargo-deny -- offline, so it runs on every
  push in CI as well as in validate, unlike the advisory audit. And `cargo xtask
  licenses` generates a THIRD-PARTY-LICENSES file, now included in every release
  archive: MIT and Apache-2.0 both require attribution to travel with a
  distributed binary, and the archives previously carried only bombyx own
  LICENSE. The list is what goes into building that binary -- normal
  dependencies of a distributed workspace member, resolved for the one target
  passed with `--target` -- so each archive carries its own platform's set. It
  is over-inclusive within that, since compile-time-only crates are listed too,
  and the file says so rather than claiming they are linked in. `COPYRIGHT` and
  `AUTHORS` are collected as notice files, which matters because that is where
  rustix and linux-raw-sys explain their triple licence and the LLVM exception,
  but a notice alone does not satisfy the gate: a crate shipping no licence
  terms fails the command, and `--max-missing` raises that bar deliberately.
  The generator runs in every-push CI as well as the release, so a dependency
  with no licence text fails while it is still a diff rather than after the tag.

### Changed

- The release workflow refuses to overwrite the assets of a release that already
  carries a SHA256SUMS, and asks for a new patch tag. Replacing them redefines a
  published version, and self-update compares only MAJOR.MINOR.PATCH -- so
  anyone already on that version is told they are up to date forever. Re-running
  a release whose upload never finished still works, and
  ALLOW_RELEASE_REPLACE=true overrides deliberately.

### Fixed

- self-update no longer promises a cleanup it may not perform. The
  leftover-binary notice said "the next self-update removes it", but the sweep
  runs only when an update actually replaces the binary, so an up-to-date run
  cleaned nothing. Sweeping on every invocation was tried and reverted: it
  widened the window in which a concurrent update can delete another one rescue
  copy, and it deleted hand-made backups matching the same name prefix. The
  message now says what happens.
- self-update now re-checks the downloaded archive after extraction, before the
  binary is installed, and refuses if it no longer matches. The digest was
  computed from one read and tar opened the same path again, so `matches its
  published checksum` was printed about bytes that need not be the bytes
  extracted. This detects an unreverted swap; it does not close the window, and
  the code says so -- a writer inside the private temp directory is already the
  same user or root, and can overwrite the installed binary directly without
  racing anything.
- A malformed bombyx.toml no longer echoes the offending source line. The toml
  crate renders it into its error text, which bombyx printed to stderr, so a
  bombyx.toml symlinked at a private key had a line of it disclosed. The
  position and the reason are kept; the file's own contents are not.

### Removed

## [0.3.0] - 2026-08-18

### Added

- bombyx self-update: replace the installed binary with the newest release.
  Finds the tag with `git ls-remote`, downloads that platform archive with
  `curl`, and verifies it against the release SHA256SUMS before extracting.
  Fails closed -- a missing or mismatched checksum refuses the update and prints
  a `cargo install` line to run by hand. Never installs a pre-release, and never
  downgrades a local build newer than any release. On Windows the running binary
  is renamed aside, since Windows refuses to overwrite a running image.

### Changed

- Release workflow: attach a SHA256SUMS covering every asset, publish a .tar.gz
  for every target (Windows keeps its .zip as well) so self-update has one
  extraction path, and update an existing release in place instead of failing
  when a tag is re-pushed.
- Releases now audit dependencies as a blocking gate, in two places: `cargo
  xtask audit` runs in the release workflow gates job and as its own step in
  /release. Standalone rather than via validate, because inside validate a
  missing cargo-audit or unreachable RUSTSEC database degrades to a warning --
  so "Validate OK" did not imply the dependencies were audited. cargo-audit is
  installed pinned and uncached in CI, since a cached copy would be the tool the
  gate consists of. The release job is also the only one granted a write-scoped
  token now.

## [0.2.0] - 2026-08-18

### Added

- Pass the VM host identity into the guest: every `vagrant` invocation that runs
  in a project directory now carries `BOMBYX_VM_HOST` (the SSH alias) and
  `BOMBYX_VM_HOSTNAME` (the host machine's `hostname -s`). A guest cannot work
  this out for itself -- there is no synced folder, `hostname` answers with the
  guest name, and the guest's DMI describes the emulated machine rather than the
  host. `doctor` is exempt: its probes run in the login directory and evaluate
  no `Vagrantfile`. See "Telling the VM which host it runs on" in README.md for
  the `Vagrantfile` and `provision.sh` lines that carry the values the rest of
  the way.

## [0.1.0] - 2026-08-16

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
  command line. A `host` of `-oProxyCommand=...` would otherwise be read by
  `ssh` as an option and run code on the workstation, so the charset is
  restricted whichever source supplied it; a scratch name must be a single path
  segment, so `../../etc` is refused rather than quoted into a traversal.
  `bombyx.toml` travels inside a repo, which is why every field it *can* carry
  is treated as untrusted.
- Remote paths keep a leading `~` outside the quotes (`~/'vms/myproject'`). A
  POSIX shell does not expand `~` inside single quotes, so a fully quoted path
  created a directory literally named `~` while `scp` wrote to the real home
  directory -- the two halves of `up` targeted different places.
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
- `bombyx destroy <project>` destroys the persistent project VM and removes its
  directory on the host. The ephemeral lifecycle was symmetric
  (`scratch`/`discard`) but the persistent one was not: `up` created and nothing
  removed. It takes the project name as confirmation and refuses a mismatch.
- `remote_root` must now be an anchored path (`~` or `/`) of at least one
  directory, with no `.` or `..` segment. Rejected when the config loads, not
  at teardown, so the write path (`mkdir`, `tar -xzf`) and the removal path
  agree on which roots are usable. `bombyx.toml` travels inside a repo, and
  bombyx deletes the directory it derives from this value.
- `bombyx doctor` checks the preconditions before they cost anything. `up`
  creates a remote directory and ships a tarball before running `vagrant`, so a
  missing piece was reported half-way through. It runs every check rather than
  stopping at the first failure, changes nothing on the host, and exits non-zero
  if any fails.
- `doctor` asks the host's **non-interactive** shell where `vagrant` is. That is
  the shell bombyx gets. A `vagrant` installed outside that shell's `PATH` works
  when you log in and type it, and is invisible to bombyx -- and vagrant cannot
  report this itself, because it is not running.
- `bombyx provision` pushes the Vagrant directory and re-runs provisioning on
  the project VM. Vagrant provisions only when it first creates a VM, so every
  later `up` shipped an edited provisioning script to the host without executing
  it -- and the push reported success. Requires a VM that already exists.
- `scripts/agent-vm-firewall.sh`, an nftables ruleset for the VM host that keeps
  agent VMs off its LAN, overlay networks, Docker and its own services while
  leaving outbound internet working. Read-only by default; `apply`, `persist`
  and `revert` are explicit. Documented in `docs/vm-host-setup.md`, and marked
  unverified until it has been applied to a real host.
- A `bombyx.local.toml` beside the config overrides any of its fields, so one
  project can point at a different machine, or use a different `remote_root`,
  without touching the committed file. Every field in it is optional and the
  file itself is optional; `--config x.toml` reads `x.local.toml`. Validation
  runs after the merge, so an override is subject to the same checks as the
  committed file rather than a way around them.
- The VM host is resolved from four sources, first match winning: `--host`, the
  `BOMBYX_HOST` environment variable (blank counts as unset), a gitignored
  per-project `bombyx.local.toml`, and a per-developer `config.toml` --
  `%APPDATA%\bombyx` on Windows, else `$XDG_CONFIG_HOME/bombyx` or
  `$HOME/.config/bombyx`, relocatable with `BOMBYX_CONFIG_HOME`. With none of
  them set, bombyx stops and names all four instead of guessing. A config
  directory from the environment must be an *anchored* path; a blank or relative
  one counts as unset, since it would otherwise resolve against the working
  directory and take the host out of whatever repo bombyx ran in. The
  per-developer file may be a symlink, as every dotfile manager makes it.
- bombyx names the source the host came from, unless it was the per-developer
  `config.toml` (the ordinary case, and noise on every command). With a
  `bombyx.local.toml` present, the override notice alone read as though that
  file's host was in force even when the flag, the environment, or nothing in
  it at all had decided the host. An error about a bad host names the file or
  flag that supplied it rather than the project config, which is the one file
  forbidden to carry one.
- `docs/tutorial.md`: an end-to-end walkthrough covering the workstation, the VM
  host and a sample project with a Vagrantfile and provisioning script.
  `docs/usage.md`: the full command reference, split out of the README.
- `docs/vm-host-wsl2.md`: using a WSL2 distribution on your own Windows machine
  as the VM host. Covers the four failures particular to WSL -- a guest bridge
  that outlives the distribution and blocks libvirt, Vagrant demanding Windows
  interop because its Hyper-V provider reads WSL as Windows, WSL stopping idle
  distributions out from under running guests (and `vmIdleTimeout` measurably
  not preventing it), and reaching the host through a one-shot `sshd -i` over a
  ProxyCommand so no port is exposed to the guests. Verified end to end, and
  explicit about the isolation such a host gives up.
- Prebuilt binaries for Linux, Windows and both macOS architectures, published
  as a GitHub Release when a `vX.Y.Z` tag is pushed. Installing bombyx
  previously required a Rust toolchain and `cargo install`. A release is gated
  twice: `/release` runs `cargo xtask validate` before it tags, and the workflow
  re-runs tests, formatting, clippy, docs, coverage and duplication before any
  binary is built.

### Changed

- `discard` now removes the scratch directory after destroying the VM, so the
  README's claim that nothing survives a scratch VM is true. Previously the
  directory and its pushed Vagrantfile were left behind, one per discarded VM.
- Teardown is re-runnable. `destroy` and `discard` skip the VM destroy when the
  directory holds no Vagrantfile instead of failing, so an interrupted first
  push can no longer strand a directory that no bombyx command could remove.
- Every command resolves the programs it needs (`ssh`, `scp`, `tar`) against
  `PATH` before running any of them, and never against the working directory. On
  Windows the OS search includes the current directory, so a repo shipping a
  `tar.exe` was workstation code execution -- in `doctor`, the command the docs
  say to run first in a fresh clone. Resolving up front also means a missing
  tool fails before `up` has created the remote directory.
- `host` is no longer read from `bombyx.toml`, and a `host` key there is now
  refused with an error naming where to move it. The VM host belongs to whoever
  drives bombyx, not to the project: each developer has their own hardware on
  their own network, and `destroy` runs `vagrant destroy` and `rm -rf` on
  whichever host is in force, so a committed value aimed everyone's teardown at
  one person's machine.

### Fixed

- Two broken links in the API documentation: `config`'s module page pointed at
  the private `Config::validate`, and an xtask doc comment linked `test`
  ambiguously (both a function and an attribute macro). Neither failed the
  build, because rustdoc reports link problems as warnings.
- `vagrant_dir` must now be a plain relative path. It is joined onto the working
  directory, and `Path::join` discards the left side for an absolute operand, so
  a repo shipping `vagrant_dir = "C:/Users/you/.ssh"` made a plain `bombyx up`
  tar that directory and scp it to the host named in the same file. Rooted
  paths, drive letters, `~` and `..` are all refused; a Windows drive is checked
  explicitly because it is not absolute on Unix and the file travels between
  platforms.
- `xtask` failed to compile on Linux and macOS: `clean_cache` imported
  `is_reparse_or_symlink_meta` unconditionally while only its `#[cfg(windows)]`
  branch uses it, and the workspace denies warnings. Nothing on a Windows
  workstation could see it, and there was no CI until now.
- Clippy failed on Linux and macOS: off Windows `is_reparse_or_symlink_path` can
  never return `Err`, so `clippy::unnecessary_wraps` fired on a signature that
  has to serve both platforms.

