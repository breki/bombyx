# Architecture

bombyx runs `vagrant` on a remote VM host over SSH, so an agent
works inside a VM and the workstation stays clean. It composes
`ssh` and `vagrant` and reimplements neither.

## The three machines

```mermaid
flowchart LR
  subgraph ws["workstation"]
    repo["project repo<br/>bombyx.toml"]
    cli["bombyx"]
  end

  subgraph host["VM host"]
    dir["~/vms/{project}<br/>Vagrantfile, bootstrap.sh"]
    vg["vagrant"]
  end

  subgraph guest["agent VM (guest)"]
    clone["/opt/project"]
    agent["agent"]
  end

  git[("git host")]

  repo --> cli
  cli -- ssh --> vg
  cli -- "heredoc over ssh" --> dir
  vg -- creates --> guest
  git -- clone --> clone
  agent --> clone
```

The workstation never runs project code. The VM host runs
`vagrant` and holds a directory per project. The guest is the
only machine that clones the repository.

**Three roles, not necessarily three machines.** `host` is an
SSH alias, so it can name loopback and the workstation can be
its own VM host. There is no local mode and none is needed.
What that costs is the part of the isolation that depends on
the host being elsewhere: a guest that escapes the hypervisor
is already on your workstation, and network isolation from your
own machine means nothing. The VM boundary still holds --
separate kernel, no host filesystem, none of your credentials
(see `docs/trust-boundary.md` for the one the guest does need).
`docs/tutorial.md` has the setup, and `docs/vm-host-wsl2.md`
covers running the host as a WSL2 distribution on a Windows
workstation.

The diagram shows today, not the target. The `project repo`
box and the arrow leaving it are what the design is working to
remove: the goal is that neither the workstation nor the VM
host reads **any** file from the project's repo. The VM host
already reads none -- the push that sent it `vagrant/` is gone.
The workstation still reads `bombyx.toml` out of the working
directory, so it still holds a checkout, and
`project-config-off-repo` is the work that closes that.
`docs/trust-boundary.md` has the reasoning and the remaining
steps.

## Library modules

Everything with a decision in it lives in the library, because
`src/bin/` is outside the coverage gate.

```mermaid
flowchart TD
  main["main (bin)"]

  main --> plan
  main --> update
  main --> doctor
  main --> term
  main --> tool

  plan --> vagrantfile
  plan --> doctor
  plan --> remote
  plan --> config
  plan --> name

  vagrantfile --> remote
  vagrantfile --> config

  update --> remote
  update --> config

  doctor <--> remote

  remote --> config
  remote --> name
  config --> name
```

`doctor` and `remote` reference each other: `remote` builds the
probe commands, `doctor` decides what their output means.

| Module | Owns |
|--------|------|
| `plan` | which commands run, and in what order |
| `config` | `bombyx.toml`, and the `Config` every command reads |
| `config::read` | reading a config file: symlinks, size, TOML errors |
| `config::error` | `ConfigError` for a file, `FieldError` for a value |
| `config::guards` | the rules more than one field shares |
| `config::host` | where the VM host name comes from, and its shape |
| `config::registry` | the operator's `config.toml` and its project tables |
| `config::root` | what `remote_root` may be, and why it is strict |
| `config::source` | `[source]`, and the two checked types it holds |
| `config::vm` | `[vm]`, and the checks a type cannot express |
| `vagrantfile` | rendering the Vagrantfile and the bootstrap |
| `remote` | building `ssh` argv, quoting |
| `remote::write` | the heredoc that writes a generated file |
| `doctor` | preconditions, and what a result means |
| `update` | `self-update`: download, verify, swap |
| `name` | scratch-VM names, and path segments |
| `term` | line endings, per stream |
| `tool` | resolving a program, never via the cwd |

`main` parses arguments, spawns processes and prints. Nothing
else.

## Domain entities

What a project declares:

```mermaid
classDiagram
  class Config {
    +String host
    +String project
    +String remote_root
  }
  class Vm {
    +Provider provider
    +String box_name
    +u32 cpus
    +u32 memory
  }
  class Source {
    +RepoUrl repo
    +String git_ref
    +ScriptPath script
  }
  class RepoUrl {
    +String value
  }
  class ScriptPath {
    +String value
  }
  class Provider {
    <<enumeration>>
    Libvirt
    Hyperv
  }
  class Registry {
    +Option~String~ host
  }
  class Project {
    +String remote_root
    +Option~String~ host
  }
  class HostSources {
    +Option~str~ flag
    +Option~str~ env
    +Option~str~ project
    +Option~Path~ user_config_dir
  }
  class HostOrigin {
    <<enumeration>>
    Flag
    Env
    ProjectEntry
    UserFile
  }

  Config *-- Vm : vm
  Config *-- Source : source
  Registry *-- Project : projects
  Project *-- Vm : vm
  Project *-- Source : source
  Project ..> Config : one entry becomes one
  Vm --> Provider
  Source *-- RepoUrl : repo
  Source *-- ScriptPath : script
  HostSources ..> Config : supplies host
  HostSources ..> HostOrigin : reports which won
```

`Config` is what bombyx runs with. `host` is the one field never
read from `bombyx.toml`, because a VM host belongs to a person
rather than a project.

`Registry` and `Project` are a second pair of config types, and
they parse the operator's own `config.toml` rather than
`bombyx.toml`: a top-level `host`, and one `[projects.<name>]`
table per project carrying `remote_root`, `[vm]`, `[source]` and
an optional `host` of its own. `config::host` reads both host
keys out of that file, and ranks the project's above the
top-level one, so an operator who keeps one project on a
different machine writes it in that project's table.

`Config::load_project` turns one entry into a `Config`. It reads
the registry once and takes everything from it: the entry
supplies every setting but the host, and `config::host::rank`
ranks the host across `--host`, `BOMBYX_HOST`, that entry's own
`host` and the file-wide one. One read rather than two, because
a file edited mid-run could otherwise supply a project host and
a file-wide host that never coexisted.

No command calls it yet. One step stands between here and a
command that does: `project-selection-flag` in `docs/todo.md`
adds the `--project` argument that first supplies a name, and
deletes `Config::load` and `bombyx.toml` with it. The
per-project `host` key waits on that step too, because nothing
names a project until it lands.

Two Rust names differ from their TOML keys, because `box` and
`ref` are Rust keywords: `box_name` is `box`, and `git_ref` is
`ref`.

What bombyx does with it:

```mermaid
classDiagram
  class Action {
    <<enumeration>>
    Up
    Provision
    Down
    Shell
    Status
    Reset
    Doctor
    Destroy
    Scratch
    Discard
  }
  class ScratchName {
    +String value
  }
  class RemoteCommand {
    +String program
    +Vec~String~ args
    +Option~PathBuf~ dir
  }
  class Tty {
    <<enumeration>>
    Allocate
    NoPty
  }

  Action --> ScratchName : Scratch and Discard carry one
  Action ..> RemoteCommand : plan() produces a list
  Tty ..> RemoteCommand : decides ssh -t
```

`plan()` turns one `Action` and a `Config` into an ordered
`Vec<RemoteCommand>`, and nothing else in the library spawns a
process. That is what makes `--dry-run` honest and the ordering
testable.

`Scratch` and `Discard` carry a `ScratchName`, which is a
validated newtype rather than a `String`: it must be one path
segment, so a name that would escape the scratch directory
cannot reach `plan()` at all.

## `bombyx up`, end to end

```mermaid
sequenceDiagram
  autonumber
  actor op as operator
  participant cli as bombyx (workstation)
  participant host as shell (VM host)
  participant vg as vagrant (VM host)
  participant guest as guest VM
  participant git as git host

  op->>cli: bombyx up
  cli->>cli: read bombyx.toml, validate
  cli->>host: mkdir -p the project dir
  cli->>host: cat > Vagrantfile (heredoc)
  cli->>host: cat > bootstrap.sh (heredoc)
  cli->>vg: vagrant up
  vg->>guest: create from box
  vg->>guest: run bootstrap.sh
  guest->>git: git clone repo at ref
  guest->>guest: run the script from the clone
  guest-->>op: VM ready
```

Two things matter about the order. The directory is created
first, because the heredocs write into it. And `vagrant` runs
last, because it reads the Vagrantfile that the heredocs just
wrote.

Every step is an `ssh`. bombyx runs no program on the
workstation for a VM action, which is why the `cli` lifeline
has no self-call.

`provision` is the same sequence ending in `vagrant provision`,
which exists because vagrant runs provisioners only when it
first creates a machine.

## What config values are checked

`bombyx.toml` travels inside a repo, so its values are treated
as hostile input. Six of them reach the generated files:
`box`, `repo`, `ref`, `script`, `cpus` and `memory`.

Two of those six are enforced by their type. `repo` is a
`RepoUrl` and `script` is a `ScriptPath`, each a newtype whose
constructor holds the rules, so an invalid one cannot be built
-- by a config file or by a library caller. serde runs the
constructor while deserializing, so a bad value is refused
before a `Config` exists, and the error identifies the line.

The other four -- `box`, `ref`, `cpus` and `memory` -- are only
*checked* after parsing. So are `host`, `project` and
`remote_root`, which never reach the generated files but do
reach `ssh`. Seven values in all, spread across
`Config::validate`, `vm::validate` and `source::validate`.
**That is a gap, not a decision we would make again.**

`Project`, the registry's per-project entry, carries the same
values less `project`. Its `host` is optional where `Config`'s
is required, and it reaches `ssh` the same way.

**Every `host` in the registry is checked as the file is read**,
by `config::registry::parse` -- the file-wide one and every
project's, not only the one a command turns out to want. This is
the file where the operator writes host names, so a value bombyx
would refuse is a mistake to report while they are looking at
it; checking only the winner leaves a typo in an unused line
until the day that line wins. Holding a `Registry` is therefore
the proof that every host in it passed, which is why
`Registry::host` and `Registry::project_host` hand their values
out without re-running the rule.

`check_winning_host` still applies the same rule to whatever the
ranking picked, and after the above that only ever bites a
`--host` or `BOMBYX_HOST` value -- the one pair that never came
through the file. One consequence worth knowing: `--host` makes
`resolve_host` skip reading the registry altogether, so a bad
host sitting in a file that run never opens is not reported.
`Config::load_project` has no such gap, because it reads the
file for the project's settings whatever the flag says.

`Config::load_project` then runs `Config::validate` over the
value it assembles, so an entry's fields are checked twice. That
is deliberate rather than an oversight: `validate` is what every
path building a `Config` calls, so a field added to `Config`
without a matching check on the entry is still refused.

Four values in an entry are checked before any lookup: the
project name, because it is the table key and a `ProjectName`;
`repo` and `script`, because their types refuse a bad value; and
`host`, by the pass described above. The rest are checked when
`Registry::project` hands the entry out, so a rule broken in one
project's table is reported when that project is asked for. A
table that does not *parse* is not like that: the whole file
fails, whichever project it belongs to.

A type promises that its rules *ran*. A checking function
promises only that they ran on the paths that call it.
`Config`, `Vm`, `Source` and `Project` all have public fields,
so any code can build one by hand and reach the guest without
`validate` ever being called -- and a field whose rules are
dull is as exposed as one whose rules are sharp. `validate` is
also private, so a library caller cannot even choose to call
it.

Three things keep that survivable meanwhile. `render` escapes
for Ruby whatever it is handed. `bootstrap.sh` passes `--`
before the ref. And inside this crate the only two places that
build a `Config` are `ProjectFile::into_config` and
`Project::to_config`, whose callers both run `validate`
immediately -- so for bombyx's own commands the check does run.

A library consumer is not covered by that, and the public fields
are why: outside the crate, a `Config` built field by field with
a hostile `host` compiles and reaches `plan` with nothing having
checked it. No snippet here, because a struct literal for this
type needs all five fields and a shortened one would not
compile, which is a distraction from the point. That is the argument for
`newtype-remaining-config-fields`, and it is the reason the
paragraph above calls the gap a gap.

`remote_root` should stop being a `String` first: it reaches
`rm -rf`, and `config::root` already holds all of its rules in
a single function, so the constructor would wrap something
that exists. Captured as `newtype-remaining-config-fields` in
`docs/todo.md`.

| Field | Refused | Because |
|-------|---------|---------|
| `box` `repo` `ref` `script` | empty or blank | no meaning when blank |
| `box` `repo` `ref` `script` | leading or trailing whitespace | almost always a copy-paste artifact, and it fails far from here — a trailing space on `repo` comes back from the guest as `repository '...' does not exist` |
| `box` `repo` `ref` `script` | control characters | end the line in a Ruby file |
| `box` `repo` `ref` `script` | `"` or `\` | end or escape the Ruby literal |
| `box` `repo` `ref` `script` | `#{` | Ruby interpolation is evaluated |
| `repo` `ref` `script` | leading `-` | `git` would treat it as an option |
| `host` `project` `remote_root` | leading `-` | the program each one reaches would treat it as an option. For `host` that is live — it is `ssh`'s first positional argument. For the other two it is a precaution, since both are shell-quoted before `ssh` carries them |
| `repo` | anything but an `https` `http` `ssh` `git` URL, or `user@host:path` | `ext::` and the other remote helpers run a command instead of cloning |
| `script` | leading `/`, a `..` segment | it is made executable and run as root inside the clone |
| `cpus` `memory` | zero | vagrant would refuse it on the VM host, after bombyx had already created a directory there |

`project` and `remote_root` have rules of their own in the same
places: `Config::validate` for `project`, and `config::root` for
every rule `remote_root` must pass.
`project` must be one path segment, because it becomes one
directory name on the VM host.

`remote_root` has the strictest rules of the three, because
bombyx runs `rm -rf` on a path derived from it. All of them live
in `config::root`, blank and leading-dash included, so a second
caller cannot pick up half the set.

It must start with `/` or `~/`. A bare
`~name` is refused even though it looks anchored: to a shell
that means another user's home directory, and
`quote_remote_path` leaves the tilde outside the quotes only for
`~` and `~/`. So `~vms` would be emitted fully quoted and the
remote shell would read it as an ordinary relative name,
resolved against the SSH login directory — the outcome the
anchoring rule exists to prevent.

It must also contain **at least one directory below that root**,
so `~/vms` and `/srv/vms` are accepted while `~`, `/` and `~/`
are refused. Joining the project name onto it then makes the
directory bombyx creates and deletes at least two deep, which
keeps a configuration mistake from targeting a top-level or
home-adjacent directory.

A `.` or `..` segment is refused as well: either one moves where
the path resolves without changing how deep it counts, and `/.`
with `project = "etc"` would otherwise pass as two segments
while resolving to `/etc`.

## Why three stages and not one

```mermaid
flowchart TD
  toml["bombyx.toml"]
  val["config::validate"]
  render["vagrantfile::render"]
  write["remote::write_file"]
  out["Vagrantfile<br/>on the VM host"]

  toml --> val
  val -- "validated Config" --> render
  render -- "Ruby text" --> write
  write -- "quoted heredoc" --> out
```

Each stage is safe on its own rather than trusting the one
before it. `render` escapes every `"`, `\` and `#` even though
`validate` already refused them. `write_file` lengthens its
heredoc delimiter until no payload line equals it, rather than
assuming the payload came from `render`.

The repetition is not redundant, and the newtypes narrowed it
rather than removing it. A library caller can no longer build a
bad `repo` or `script`, because those fields hold `RepoUrl` and
`ScriptPath` and their inner values are private. `box` and
`git_ref` are still plain `String` fields on a public struct, so
`render` can still be handed a quote, and `write_file` can still
be handed a payload no renderer produced. A guard that lives in
another module is the one a new field gets added without.

## Quality gates

`cargo xtask validate` runs every gate in one pass. The
dependency cooldown goes first so nothing compiles a too-new
crate; after that they run cheapest first, ending with the
network audit. `CLAUDE.md` under **Definition of Done** lists
them in run order and is the one place that does, so this
paragraph does not repeat the list -- two copies of it drifted
apart once already. It also says why `audit` degrades to a
warning inside `validate` and errors when run alone.
