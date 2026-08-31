# Architecture

bombyx runs `vagrant` on a remote VM host over SSH, so an agent
works inside a VM and the workstation stays clean. It composes
`ssh`, `scp`, `tar` and `vagrant` and reimplements none of them.

## The three machines

```mermaid
flowchart LR
  subgraph ws["workstation"]
    repo["project repo<br/>bombyx.toml, vagrant/"]
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
  cli -- "scp + heredoc" --> dir
  vg -- creates --> guest
  git -- clone --> clone
  agent --> clone
```

The workstation never runs project code. The VM host runs
`vagrant` and holds a directory per project. The guest is the
only machine that clones the repository.

The diagram shows today, not the target. The `project repo`
box and the arrow leaving it are what the design is working to
remove: the goal is that neither the workstation nor the VM
host reads **any** file from the project's repo, `bombyx.toml`
and `vagrant/` included. Both still do.
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
| `config` | `bombyx.toml`, host resolution, every field rule |
| `config::vm` | `[vm]`, `[source]`, and their guards |
| `vagrantfile` | rendering the Vagrantfile and the bootstrap |
| `remote` | building `ssh`/`scp`/`tar` argv, quoting |
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
    +String vagrant_dir
    +String remote_root
  }
  class Vm {
    +Provider provider
    +String box_name
    +u32 cpus
    +u32 memory
  }
  class Source {
    +String repo
    +String git_ref
    +String script
  }
  class Provider {
    <<enumeration>>
    Libvirt
    Hyperv
  }
  class Overlay {
    +all fields optional
  }
  class HostSources {
    +Option~str~ flag
    +Option~str~ env
    +Option~Path~ user_config_dir
  }
  class HostOrigin {
    <<enumeration>>
    Flag
    Env
    Overlay
    UserFile
  }

  Config *-- Vm : vm
  Config *-- Source : source
  Vm --> Provider
  Overlay ..> Config : replaces fields of
  HostSources ..> Config : supplies host
  HostSources ..> HostOrigin : reports which won
```

`Config` is what bombyx runs with. `host` is the one field never
read from `bombyx.toml`, because a VM host belongs to a person
rather than a project.

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
  class PushArchive {
    +PathBuf dir
    +String name
  }
  class Tty {
    <<enumeration>>
    Allocate
    NoPty
  }

  Action --> ScratchName : Scratch and Discard carry one
  Action ..> RemoteCommand : plan() produces a list
  PushArchive ..> RemoteCommand : names the tar and scp
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
  cli->>cli: tar -czf the archive from vagrant/
  cli->>host: scp the archive
  cli->>host: tar -xzf, then remove it
  cli->>host: cat > Vagrantfile (heredoc)
  cli->>host: cat > bootstrap.sh (heredoc)
  cli->>vg: vagrant up
  vg->>guest: create from box
  vg->>guest: run bootstrap.sh
  guest->>git: git clone repo at ref
  guest->>guest: run the script from the clone
  guest-->>op: VM ready
```

Two things about the order. The generated files land **after**
the archive is unpacked, or the push would overwrite them. And
`vagrant` runs last, because it reads the Vagrantfile.

`provision` is the same sequence ending in `vagrant provision`,
which exists because vagrant runs provisioners only when it
first creates a machine.

## What config values are checked

`bombyx.toml` travels inside a repo, so its values are treated
as hostile input. Six reach the generated files.

| Field | Refused | Because |
|-------|---------|---------|
| `box` `repo` `ref` `script` | empty or blank | no meaning when blank |
| `box` `repo` `ref` `script` | control characters | end the line in a Ruby file |
| `box` `repo` `ref` `script` | `"` or `\` | end or escape the Ruby literal |
| `box` `repo` `ref` `script` | `#{` | Ruby interpolation is evaluated |
| `repo` `ref` `script` | leading `-` | `git` reads it as an option |
| `repo` | anything but an `https` `http` `ssh` `git` URL, or `user@host:path` | `ext::` and the other remote helpers run a command instead of cloning |
| `script` | leading `/` or `\`, a `..` segment, surrounding space | it is made executable and run as root inside the clone |
| `cpus` `memory` | zero | vagrant would refuse it on the VM host, after the push changed state |

The older fields have their own rules, in the same place:
`project` must be one path segment, `vagrant_dir` must stay
inside the project, and `remote_root` must be anchored and
several directories deep, because bombyx runs `rm -rf` on a path
derived from it.

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

The repetition is not redundant. `Config`'s fields are public
and both functions are public, so a library caller reaches them
without passing through `validate` at all. A guard that lives in
another module is the one a new field gets added without.

## Quality gates

`cargo xtask validate` runs nine, cheapest first: dependency
cooldown, formatting, duplication, licences, clippy, doc links,
tests, coverage, security audit. `CLAUDE.md` has the detail,
including why `audit` degrades to a warning inside `validate`
and errors when run alone.
