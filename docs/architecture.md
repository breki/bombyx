# Architecture

bombyx runs `vagrant` on a VM host, usually a second machine
reached over SSH, so an agent works inside a VM and the
workstation stays clean. It composes `ssh` and `vagrant` and
reimplements neither.

## The three machines

```mermaid
flowchart LR
  subgraph ws["workstation"]
    reg["~/.config/bombyx<br/>config.toml"]
    cli["bombyx"]
  end

  subgraph host["VM host"]
    dir["~/vms/{project}<br/>Vagrantfile, bootstrap.sh"]
    vg["vagrant"]
  end

  subgraph guest["agent VM (guest)"]
    clone["~/project<br/>in the agent's home"]
    agent["agent"]
  end

  git[("git host")]

  reg --> cli
  cli -- "ssh, or sh -c here" --> vg
  cli -- "heredoc, same two routes" --> dir
  vg -- creates --> guest
  git -- clone --> clone
  agent --> clone
```

The workstation never runs project code. The VM host runs
`vagrant` and holds a directory per project. The guest is the
only machine that clones the repository.

**Three roles, not necessarily three machines.** The
workstation can be its own VM host, and bombyx notices when it
is. As `config.toml` is read, bombyx compares `host` against
this machine's own name; when the two match it runs the script
through `sh -c` here instead of handing it to `ssh`.

Two rules keep that comparison honest, and both exist because
the wrong answer boots a guest on the workstation while the
operator believes it is elsewhere.

The first is that the two names must be equal, ignoring case
and nothing else. A domain is written to say *which* machine,
and a bare label is shared easily -- `ubuntu`, `vagrant`,
`build01` -- so `build01.dmz.example` and
`build01.corp.example` are two machines, and so are
`frosti.lan` and a machine calling itself plain `frosti`.
The two rules err in opposite directions, and only one of them
is affordable. Matching on less than the whole name errs
towards the local route, which is the dangerous answer: a guest
boots on the workstation and teardown deletes there. Exact
matching errs towards `ssh`, which costs the operator a
handshake they were not expecting -- they see it, and write
what `hostname` prints.

bombyx does not read `~/.ssh/config`, so an alias named exactly
what this machine is named is believed even when it points
elsewhere. `you@name` is the spelling that forces `ssh`, and
`config::transport`'s test table pins it.

The second is that Windows never takes the local route at all,
because it cannot run libvirt. The route would still get far
enough to write files into the MSYS home and later delete them,
which is worse than failing.

One thing both routes share is worth knowing, because it looks
at first like a difference. Three vagrant variables --
`VAGRANT_CWD`, `VAGRANT_VAGRANTFILE` and `VAGRANT_DOTFILE_PATH`
-- override the directory every script bounds itself with, and
two more decide the provider. An operator with `VAGRANT_CWD`
exported would have `destroy` check one project and destroy
another. So `remote::transport` writes an `unset` of all five
in front of every script, on both routes.

The reason differs per route even though the precaution does
not. `sh -c` is a child of bombyx and inherits its whole
environment, so anything the operator exported here arrives.
Over `ssh` bombyx's own environment stays behind, but the VM
host builds one of its own. Three sources reach the command
sshd runs: `pam_env` applies `/etc/environment`, `zsh` sources
`~/.zshenv` on every invocation and so on `zsh -c` too, and a
`bash` export placed above the non-interactive return guard in
`~/.bashrc` survives. The command sshd runs is neither
interactive nor a login shell, which is why `~/.profile` is not
in that list. A variable set on the VM host is as dangerous as
one set here. The `zsh` and `bash` halves are read from those
shells' documented startup order rather than measured
*(unverified)*.

Past that `unset` the script is identical on both routes,
because every command bombyx builds is a POSIX shell script
string and `sh -c` starts the same shell `ssh` would have
started on the host. `config::transport` holds the comparison
and `remote::transport` holds the one wrapper that acts on it.

Two consequences are worth stating plainly. The first is that
one `config.toml` now behaves differently depending on which
machine reads it, so bombyx prints a line naming the route
whenever the local one is in force, and `bombyx doctor` shows
its `ssh` and `login shell` rows as skips rather than passes --
neither question has anything left to answer on this route.

The second is what running the guest on your own workstation
costs, which is the part of the isolation that depends on the
host being elsewhere: a guest that escapes the hypervisor is
already on your workstation, and network isolation from your
own machine means nothing. The VM boundary still holds --
separate kernel, no host filesystem, none of your credentials
(see `docs/trust-boundary.md` for the one the guest does need).
`docs/tutorial.md` has the setup, and `docs/vm-host-wsl2.md`
covers running the host as a WSL2 distribution on a Windows
workstation.

The diagram shows no box for the project's repository, and that
is the point: neither the workstation nor the VM host reads any
file from it. The VM host reads none because the push that sent
it `vagrant/` is gone. The workstation reads none because every
setting comes out of `config.toml`, which lives in the
operator's config directory, and `--project` names the project
rather than the working directory implying it. The workstation
therefore needs no checkout at all. `docs/trust-boundary.md`
has the reasoning.

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
| `config` | the registry, and the `Config` every command reads |
| `config::read` | reading a config file: symlinks, size, TOML errors |
| `config::error` | `ConfigError` for a file, `FieldError` for a value |
| `config::guards` | the rules more than one field shares |
| `config::host` | where the VM host name comes from, and its shape |
| `config::registry` | the operator's `config.toml` and its project tables |
| `config::root` | what `remote_root` may be, and why it is strict |
| `config::deploy_key` | what `deploy_key` may be, and why |
| `config::source` | `[source]`, and the three checked types it holds |
| `config::transport` | whether `host` names this very machine |
| `config::vm` | `[vm]`, and the checks a type cannot express |
| `vagrantfile` | rendering the Vagrantfile and the bootstrap |
| `remote` | building the argv for either route, quoting |
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
    +HostName host
    +ProjectName project
    +RemoteRoot remote_root
    -Transport transport
  }
  class Vm {
    +Provider provider
    +BoxName box_name
    +NonZeroU32 cpus
    +NonZeroU32 memory
  }
  class Source {
    +RepoUrl repo
    +GitRef git_ref
    +ScriptPath script
    +Option~DeployKeyPath~ deploy_key
  }
  class RepoUrl {
    +String value
  }
  class ScriptPath {
    +String value
  }
  class GitRef {
    +String value
  }
  class BoxName {
    +String value
  }
  class ProjectName {
    +String value
  }
  class RemoteRoot {
    +String value
  }
  class DeployKeyPath {
    +String value
  }
  class HostName {
    +String value
  }
  class EnvName {
    +String value
  }
  class EnvValue {
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
    +RemoteRoot remote_root
    +Option~String~ host
  }
  class HostOrigin {
    <<enumeration>>
    ProjectEntry
    UserFile
  }
  class Transport {
    <<enumeration>>
    Ssh
    Local
  }

  Config *-- Vm : vm
  Config *-- Source : source
  Config *-- Transport : transport
  Registry *-- Project : projects
  Project *-- Vm : vm
  Project *-- Source : source
  Project ..> Config : one entry becomes one
  Vm --> Provider
  Source *-- RepoUrl : repo
  Source *-- ScriptPath : script
  Source *-- GitRef : ref
  Source *-- DeployKeyPath : deploy_key
  Vm *-- BoxName : box
  Config *-- ProjectName : project
  Config *-- EnvName : env keys
  Config *-- EnvValue : env values
  Project *-- EnvName : env keys
  Project *-- EnvValue : env values
  Registry ..> HostOrigin : ranked to produce one
```

`Config` is what bombyx runs with. `Registry` and `Project`
parse the file it comes out of: a file-wide `host`, and one
`[projects.<name>]` table per project carrying `remote_root`,
`[vm]`, `[source]` and an optional `host` of its own.

`Config::load_project` turns one entry into a `Config`. It reads
the file once and takes everything from it: the entry supplies
every setting but the host, and `config::host::rank` picks
between that entry's own `host` and the file-wide one, the entry
winning. One read rather than two, because a file edited mid-run
could otherwise supply a project host and a file-wide host that
never coexisted.

An operator who keeps one project on a different machine writes
`host` in that project's table, and bombyx then prints a line on
stderr naming the table. That notice exists because both keys
live in one file and `destroy` runs `rm -rf` on whichever wins.

`transport` is the one field of `Config` no key supplies.
`config::transport` derives it from the winning `host` and this
machine's own name. It is derived from the *winner* rather than
from either key, so the machine bombyx runs the commands on and
the machine `destroy` deletes a directory on are always the
same one.

It is also the one field of `Config` that is private, read
through `Config::transport()`. Every other field is public.
Privacy here stops a caller *choosing* the route, and that is
all it stops: `host` is public, so a caller holding a loaded
`Config` can assign a new one, nothing re-checks, and the route
then names one machine while the commands run on another.
`host` is a `HostName`, so the value assigned has passed the
host rule; what nothing re-derives is the route beside it.

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
    Snapshot
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

  op->>cli: bombyx --project p up
  cli->>cli: read config.toml, check every value
  cli->>host: mkdir -p the project dir
  cli->>host: cat > Vagrantfile (heredoc)
  cli->>host: cat > bootstrap.sh (heredoc)
  cli->>host: cd the project dir, then vagrant up
  host->>vg: vagrant up
  vg->>guest: create from box
  vg->>guest: run bootstrap.sh
  guest->>git: git clone repo at ref
  guest->>guest: run the script from the clone
  cli->>host: one script: list, test, save if absent
  host->>vg: vagrant snapshot list
  vg-->>host: the names it holds
  opt fresh-install not among them
    host->>vg: vagrant snapshot save fresh-install
  end
  guest-->>op: VM ready
```

Three things matter about the order. The directory is created
first, because the heredocs write into it. `vagrant up` runs
after them, because it reads the Vagrantfile they just wrote.
And the snapshot is saved after the boot, so it records a
machine that has finished provisioning.

The four arrows from `one script` to the save are a single
command. bombyx sends one shell script holding the listing, the
test and the save, so the host's shell is what reads the listing
and what decides; bombyx receives neither the names nor the
decision. `VM ready` below them is not something bombyx emits at
all -- it is the operator seeing a working machine.
`vagrant snapshot list` exits 0 whether or not the machine has
snapshots, so the script tests its output rather than its
status.

**Every project `vagrant` call carries the two `BOMBYX_VM_*`
names telling the guest which machine it runs on.** Every one
but the teardown also carries `VAGRANT_DEFAULT_PROVIDER`, which
is how bombyx selects a provider rather than merely configuring
one; it goes on more than the boot because the `unset` above
cleared whatever the host had.

`bombyx destroy` is the exemption, and `remote::is_teardown`
holds it. Three facts measured on a libvirt host are the
argument. Vagrant reads an existing machine's recorded provider
and ignores this variable. With no machine yet, an unusable
provider makes it refuse `status`, `halt` and `destroy` as
readily as `up`. And with nothing set at all, a `destroy` in a
directory holding a Vagrantfile and no machine reports "Domain
is not created" and exits 0.

So naming a provider on the teardown can only ever refuse it,
and the directory removal runs after the destroy, which leaves
a refused teardown with nothing able to clear the directory.
Omitting it is safe because a refusal implies no machine
exists. A WSL2 host inverts that, and
[vm-host-wsl2.md](vm-host-wsl2.md) carries the gap.

Pretty-printed, and with the environment prefix left off each
`vagrant` call, that script is:

```sh
cd <project dir> && {
  names=$(vagrant snapshot list) &&
  if ! printf '%s\n' "$names" | grep -qx 'fresh-install'; then
    vagrant snapshot save fresh-install
  fi || printf 'bombyx: could not save ...\n' >&2
}
```

Three parts of that carry weight. Capturing the listing rather
than piping it into `grep` is what stops a listing vagrant could
not produce being read as an empty one, because a pipeline
reports only its last command's status. The braces keep the `cd`
outside the `||`, so a project directory that has gone away
still fails the step. And the `||` itself makes the snapshot
advisory: it is the last step of `up`, and without it a VM that
booted correctly would report failure because a snapshot could
not be taken.

The `if` is what keeps `fresh-install` meaning what it says.
Only the first `up` finds the name missing; every later one
follows arbitrary use of the machine and must not overwrite the
point `reset` returns to. `bombyx snapshot` is the way to
overwrite it deliberately, and it passes `-f` rather than
sharing this guard.

Every step is one command, and which command depends on the
route. Over SSH each step is an `ssh`. Running on the VM host
itself each step is an `sh -c` carrying the same script. Either
way bombyx spawns exactly one process per step and interprets
none of the script itself. The one `cli` self-call in the
diagram is reading the config, which happens before there is a
plan to run.

`provision` is the same sequence ending in `vagrant provision`,
which exists because vagrant runs provisioners only when it
first creates a machine.

### Who runs the project's script

`bootstrap.sh` runs as root, and hands over to the project's
own script as the unprivileged user the box logs in as --
`vagrant` on every box bombyx assumes. The script resolves
`runuser`'s path once into `$runuser_bin`, refusing the run if
it cannot find it, and its last line is
`exec -- "$runuser_bin" -u "$OWNER" -- "$script_real"`.

The split follows what each half actually needs. Two things in
that script need root: clearing a key an earlier bombyx left in
`/root/.ssh`, and being able to drop privilege at all.

The clone is not one of them. It lives in the agent's own home,
read from that account's passwd entry, so the agent creates and
removes it and every command bombyx runs that modifies it runs
as that user -- which is what took the last root operation off a
tree the agent controls.

The project's own script has `sudo` and runs with that tree as
its working directory, so it can leave content bombyx's own
commands then cannot change -- a root-owned file, a directory
with no write bit, a mount point. Every place bombyx updates or
removes the clone therefore checks whether it succeeded and
says what to clear when it did not, rather than letting a bare
`git` or `rm` message be the whole diagnosis.

The project's script is not one either, and running it as root
has a consequence that is easy to miss: whatever it
installs -- a language toolchain, an agent's own configuration,
a shell profile -- lands in `/root` instead of in the home
directory of the account the agent logs in as. The agent then
finds none of it, and nothing fails while that happens.

That is not hypothetical. It is how the first real run against
a project went, and the symptom was a `/root/.rustup` and a
second clone at `/root/jutro` in a VM whose agent works as
`vagrant`.

Root is still reachable from the project's script through
`sudo`, which every Vagrant box configures for that user. That
is the right shape: a script asks for root at the steps that
need it rather than holding it throughout.

The hand-over uses `runuser` and not `sudo`, because `runuser`
is a root-only tool that needs no sudoers entry, so it works on
a box with `sudo` locked down. It sets `HOME`, `USER`,
`LOGNAME` and `SHELL` for the target user and passes the rest
of the environment through, which is what the `BOMBYX_*`
variables depend on.

## What config values are checked

**The registry is usually the operator's own file, and bombyx
cannot assume it.** Two arguments point the loader elsewhere.
`--config <path>` reads any file at all, including one committed
in a clone. `BOMBYX_CONFIG_HOME` only has to be *anchored*, so
an absolute path into a clone is accepted, and a per-directory
environment tool (`direnv`, `mise`, a CI job) sets it from
inside one. Either way the values are then repo-supplied.

So the allowlist is a boundary rather than a typo check. Each
of those rules is what stops a repo-supplied value reaching
`ssh` or `rm -rf`, so none of them is there to catch a typo.
Membership of the guarded set turns on one question: does the
operator choose the value's text? Six of them reach the
generated files and so the guest: `box`, `repo`, `ref`,
`script`, `cpus` and `memory`. The last two are on that list
although they are not strings, because the operator still
chooses the number and a floor is what guards them.

`remote_root` reaches `rm -rf` on the VM host. A config out of
a clone naming `remote_root = "/etc"` gets
`rm -rf /etc/<project>` there, which is `RemoteRoot`'s depth
floor doing the work it exists for.

`deploy_key` belongs to neither of those groups. Its text
reaches the generated Vagrantfile and the `ssh` script
`remote::require_file` composes, and stops on the VM host --
what reaches the guest is the *file's contents*, at a path
bombyx fixes. So it is the one value whose effect is to move a
file rather than to run a command, and whoever writes the
config chooses which file. Nothing in `DeployKeyPath`
constrains that: a config out of a clone naming
`deploy_key = "~/.ssh/id_ed25519"` uploads the VM host's own
SSH key into a VM the project's code is about to run in. The
rules check the path's *shape*, never what it names, which is
why "do not pass `--config` a path inside a repository you did
not write" is what carries this one.

`provider` is the one value that answers the question the other
way, so it carries no guard. It reaches as far as any of them --
into the generated Vagrantfile, and onto the command line
bombyx hands to `ssh` or to `sh -c`, where
`VAGRANT_DEFAULT_PROVIDER` tells vagrant which provider to use
rather than letting it choose. Every project vagrant call but
the teardown carries it, and **`bombyx up`, end to end** above
holds why: `remote::is_teardown` is the exemption, argued there
from three facts measured on a libvirt host. But `Provider` is
a closed enum, and serde admits only the two words `libvirt`
and
`hyperv` while the file is read, so nothing an operator typed
reaches the shell or the guest and a guard would have nothing to
check. `remote::vagrant_command` quotes it regardless, so the
assignment matches every other one in the script.

What the guards do *not* stop is the redirect itself: bombyx
opens no file in a project's directory of its own accord, and it
opens the one `--config` names without asking where it came
from. `docs/usage.md` under **What is checked, and what is not**
is the operator-facing half of this.

Ten values are enforced by a newtype of bombyx's own:
`remote_root` is a `RemoteRoot`, `repo` a `RepoUrl`, `script` a
`ScriptPath`, `box` a `BoxName`, `ref` a `GitRef`, `deploy_key`
a `DeployKeyPath`, `project` a `ProjectName`, `host` a
`HostName`, and each `[env]` entry is an `EnvName` keying an
`EnvValue`. Each constructor holds the rules, so an invalid one
cannot be built -- by a config file or by a library caller. All
ten but `host` run their constructor as serde deserializes,
so a bad value is refused before a `Config` exists and the
error identifies the line.

`EnvName` is the second newtype in bombyx that arrives as a map
key rather than as a field, `ProjectName` being the first. That
is the reason it is a type: nothing calls a checking function
on a key while serde is building the map.

`deploy_key` is the only optional one. It is an
`Option<DeployKeyPath>`, so a project cloning a public
repository leaves the key out, the generated Vagrantfile
carries no upload block, and the plan carries no check step.

The value reaches two places and neither is an argv slot. One
is a double-quoted Ruby literal in the generated Vagrantfile,
which `vagrant` expands with `File.expand_path`. The other is a
quoted shell assignment in the script `remote::require_file`
builds, which travels over `ssh` like every other script bombyx
composes -- so shell safety does matter here, and
`quote_remote_path` is what provides it.

So `config/deploy_key.rs` runs the Ruby-literal rule, the
remote-path charset, and anchoring rules of its own. It leaves
out the leading-dash rule, because no program is handed the
value as an argument and anchoring already refuses every value
that could read as an option.

**Where the key's existence is checked is a decision, not an
accident.** `plan::write_then` puts `remote::require_file`
ahead of the `mkdir`, so `up`, `provision` and `scratch` refuse
a missing key before creating a directory or writing a file.
The generated Vagrantfile could test the file itself and
`raise`, saving a round trip, and must not: `vagrant destroy`
loads that Vagrantfile too, so a raise there leaves a directory
no bombyx command can remove -- teardown stops at the failing
destroy and never reaches `remove_dir`.
`remote::destroy_vm_if_present` records the same hazard for a
Vagrantfile reading an environment variable with no default.
bombyx knows which verb is running and the Vagrantfile does
not, so the refusal lives in the plan and the upload stays
conditional.

`cpus` and `memory` are `std::num::NonZeroU32`, which is the
whole rule either has and makes a zero unrepresentable for a
library caller as surely as a newtype would. What the standard
type does not do is say *which* key was wrong: serde's message
for it reads `invalid value: integer 0, expected a nonzero u32`.
bombyx prints `toml`'s `message()` rather than its `Display`,
because `Display` quotes the source line into the output, and
the key appears only in that quoted line. So the two would have
been the only config values whose refusal did not name a key.

`config::vm::positive_cpus` and `positive_memory` are what put
the name back. serde reads each field through one of them, and
both delegate to `at_least_one`, which reads a plain `u32` and
turns every refusal -- the zero, a negative, a value past
`u32::MAX`, a quoted number -- into a `FieldError` naming the
field. The operator reads
``invalid `cpus`: must be at least 1`` with the line and column
beside it.

`HostName` is the exception, and it has no `#[serde(try_from =
"String")]`. The registry carries a `host` key per project and
one more below them all, so the field name `host` does not tell
an operator which line to edit. Instead `config::host::checked`
takes a `HostOrigin` and names the source, and serde cannot
supply one because it does not know which key it is reading.
Trading that answer for a line number would be the worse deal,
so the host rule runs where the origin is known.

**Parsing is where every rule runs.** serde has applied all of
them by the time a `Config` exists, so a `Vm`, a `Source`, a
`Project` or a `Config` that exists at all is one whose values
passed, whoever built it and however -- with one exception.
`Project::host` is a bare `Option<String>`, and its rule runs
in `registry::parse` rather than in a type, because the field
name cannot say which of the two `host` keys carried the value.
So the `host` guarantee belongs to `registry::parse` rather
than to the `Project` type. Nothing outside the crate can hold
a `Project` to begin with: `mod registry` is private, the type
is not re-exported, and `Project::to_config` is `pub(super)`.

`Project`, the registry's per-project entry, carries the same
values less `project`, which is its table key.

### The host rule runs in two places, and this is the owner

The argument lives here. `config::host`, `config::registry` and
`llms.txt` each state the local fact and point at this heading,
so there is one copy to correct rather than five.

**Every `host` in the registry is checked as the file is read**,
by `config::registry::parse` -- the file-wide one and every
project's, not only the one a command turns out to want. This is
the file where the operator writes host names, so a value bombyx
would refuse is a mistake to report while they are looking at
it. Checking only the winner leaves a typo in an unused line
until the day that line wins.

`config::host::rank` then runs the rule again on the one value
it is about to hand on, and builds it into a `HostName`. The two
passes divide by job. The first exists to report a value nobody
asked about. The second exists so that `rank` produces a
`HostName` without depending on where the value came from.

The second pass cannot fail on a `Registry` that came through
`parse`, because `parse` refused every bad host before the
`Registry` existed, and `parse` is the only production route to
one. `rank`'s doc comment says so rather than implying an error
an operator could provoke.

`Registry::host` and `Registry::project_host` hand raw values
out without running the rule. They can, because
`config::registry::parse` already ran it on every `host` in the
file, and `parse` is the only way to build a `Registry`.

**Every value in an entry is now checked before any lookup.**
The project name, because it is the table key and a
`ProjectName`; `host`, by the pass described above; and the
rest, because their types refuse a bad value while the table
parses. So one project's broken table fails the whole file,
whichever project the operator asked for. That is the price of
the guarantee, and it is the same price a table that does not
parse has always cost.

A type promises that its rules *ran*. A checking function
promises only that they ran on the paths that call it. `Vm`,
`Source` have nothing but public fields, so any code can build
one by hand and reach the guest without calling anything, and a
private checking function is not something a
library caller could reach for even if it wanted to. That is
why every rule here belongs to a type.

`Config` is one step better and not two. Its private
`transport` field stops a struct literal outside this crate, so
a `Config` has to come from `Config::load_project`. Every other
field is public, so a caller can still assign to one on the
`Config` it was handed, and only the fields with types refuse a
bad value when they do.

`Project` is the case where the guarantee belongs to a
different type. Holding a `Registry` proves every host in the
file passed, since `config::registry::parse` is the only way to
build one, and holding a `&Project` proves the same thing only
because `Registry::project` handed it over. `Project` derives
`Deserialize` and has a public `host`, so within the crate a
serde call on arbitrary text would produce one carrying a `host`
no rule has touched. What keeps that from reaching a library
consumer is visibility rather than a rule on the field: the
`registry` module is private, `Project` is not exported, and
`Registry` is `pub(crate)`.

### The heading spelling has one owner

Three error messages quote a project name back at the operator
inside a TOML table heading: `ConfigError::ProjectNotFound`,
`ConfigError::RegistryNotFound`, and `HostOrigin::describe` when
a project entry supplied the host. All three ask
`config::registry::heading` for it, and that function is the only
place the spelling exists.

The spelling is not obvious, which is why it needs an owner. A
project name may contain a `.` -- `name::check_segment` allows
one after the first character -- and TOML reads a bare dot in a
heading as nesting. So `[projects.a.b]` declares `b` inside
`projects.a`, `deny_unknown_fields` refuses the whole file, and
an operator who follows that advice breaks every project rather
than fixing one. Quoting is valid TOML for every name the check
accepts, so `[projects."a.b"]` is right for all of them and the
message never has to guess which names need it.

Three separate messages spell that heading, which is `/review`'s
"the rule has no single home" pattern: each copy can drift on its
own, and a reader checking one learns nothing about the other two.

`every_message_spells_a_project_heading_the_same_way` in
`config/registry.rs` is what holds it: it asserts one spelling
across all three messages, so a fourth message spelling the
heading itself would pass whatever test it brought and fail that
one. Unquoting `heading` fails nine tests.

The test fixtures write their headings out rather than calling
`heading`. That is deliberate: a fixture and the message checked
against it must not come from the same code, or a wrong spelling
agrees with itself and every test still passes.

### Two traps a reader cannot see from the code

`CLAUDE.md` under **Code comments** says a trap aimed at a future
editor lives here rather than in a comment. Neither of these two
is visible at the place it matters.

The first is the one above: the `Project` guarantee is a
property of `Registry`, not of `Project`.

The second is that `--project` is required by hand rather than
by clap. clap cannot mark one global argument required for some
subcommands and not others, and `self-update` is the subcommand
that must run on a machine with no registry at all. So `main`
states the requirement itself, after the `self-update` branch has
already returned. A third config-less subcommand added to `Cmd`
gets that for free; one added to `VmCmd` does not, and would
fail at the requirement rather than at compile time.

### Every field of a Config carries its own rule

`Config` and its two tables hold nine values. Every one of them
is a type that refuses a bad value as it is built, so a caller
assigning to a public field of a loaded `Config` gets the same
check the config file got.

That matters because `load_project` hands the caller an owned
`Config` with public fields. `cfg.project = ProjectName::parse(
"...")?` compiles; `cfg.project = "../etc".to_owned()` does not.
A type carries its proof to every use site, and a checking
function carried it only to the paths that called it.

Two defences behind the types are worth keeping anyway.
`vagrantfile::render` escapes for Ruby whatever it is handed,
and `bootstrap.sh` passes `--` before the ref. Neither is
reachable through a checked value any more, which is what makes
them precautions: they hold if a rule is ever loosened, and
`vagrantfile`'s own test exercises the escaping directly
because no config can reach it.

| Field | Refused | Because |
|-------|---------|---------|
| `box` `repo` `ref` `script` `deploy_key` `[env]` values | empty or blank | no meaning when blank |
| `box` `repo` `ref` `script` `deploy_key` `[env]` values | leading or trailing whitespace | almost always a copy-paste artifact, and it fails far from here — a trailing space on `repo` comes back from the guest as `repository '...' does not exist` |
| `box` `repo` `ref` `script` `deploy_key` `[env]` values | control characters | end the line in a Ruby file |
| `box` `repo` `ref` `script` `deploy_key` `[env]` values | `"` or `\` | end or escape the Ruby literal |
| `box` `repo` `ref` `script` `deploy_key` `[env]` values | `#{` | Ruby interpolation is evaluated |
| `repo` `ref` `script` | leading `-` | `git` would treat it as an option |
| `host` `project` `remote_root` | leading `-` | the program each one reaches would read it as an option |
| `repo` | anything but an `https` `http` `ssh` `git` URL, or `user@host:path` | `ext::` and the other remote helpers run a command instead of cloning |
| `script` | leading `/`, a `..` segment | root makes it executable, and it is then run inside the clone |
| `deploy_key` | anything but a `/` or `~/` anchor | `vagrant` runs in the project's directory on the VM host, so a relative path would look for the key under a directory bombyx creates, writes and deletes |
| `deploy_key` | a `.` or `..` segment, `//`, a `~` past the first character, a trailing `/`, no file below the anchor, any character outside letters, digits, `.` `_` `-` `/` `~` | the VM host expands the path and the operator never sees the result, so a value that resolves somewhere other than where it reads is refused rather than reported |
| `[env]` names | anything but a leading letter or `_` followed by letters, digits and `_` | the guest exports each one as a shell variable, and `9LIVES=1` is a syntax error while `WITH-DASH=1` is read as a command to run |
| `[env]` names | a leading `BOMBYX_` | the generated Vagrantfile writes bombyx's own variables and the project's into one Ruby hash literal, and a repeated key there takes its last value, so a project could otherwise choose which script bombyx runs |
| `cpus` `memory` | zero | vagrant would refuse it on the VM host, after bombyx had already created a directory there |

The dash rule is the one row where what it buys differs by
field, and all three carry it in a constructor. `project`
inherits it from `check_segment`, which refuses any first
character that is not a letter or a digit.

For `host` the rule is live: the value is `ssh`'s first
positional argument, so `-oProxyCommand=...` would run as an
instruction. Running on the VM host itself no argv position
holds it, and the rule still applies there, because the same
`config.toml` carried to another machine takes the `ssh` route.

For `project` and `remote_root` it is a precaution. Both are
shell-quoted before the far shell receives them, so the dash
cannot be read as an option today. The rule is kept because that
quoting lives in another file, where somebody may rewrite it
without knowing it is what makes these two safe.

`project` and `remote_root` have rules of their own beyond the
table above, and each runs them in its own constructor.
`project` must be one path segment, because it becomes one
directory name on the VM host, and `ProjectName` shares that
rule with `ScratchName` through `name::check_segment`.
`RemoteRoot`'s rules run when serde builds it, while the table
parses.

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
  toml["config.toml"]
  val["config::load_project"]
  render["vagrantfile::render"]
  write["remote::write_file"]
  out["Vagrantfile<br/>on the VM host"]

  toml --> val
  val -- "checked Config" --> render
  render -- "Ruby text" --> write
  write -- "quoted heredoc" --> out
```

Each stage is safe on its own rather than trusting the one
before it. `render` escapes every `"`, `\` and `#` even though
`BoxName`, `RepoUrl`, `GitRef`, `ScriptPath` and
`DeployKeyPath` already refused them. `write_file` lengthens
its heredoc delimiter until no payload line equals it, rather
than assuming the payload came from `render`.

The repetition is not redundant, and the newtypes narrowed it
rather than removing it. A library caller can no longer hand
`render` a quote at all: every value it writes into the Ruby is
a newtype whose inner value is private. What survives is the
stage below it -- `write_file` can still be handed a payload no
renderer produced -- and the fact that a guard living in another
module is the one a new field gets added without.

## Quality gates

`cargo xtask validate` runs every gate in one pass. The
dependency cooldown goes first so nothing compiles a too-new
crate; after that they run cheapest first, ending with the
network audit. `CLAUDE.md` under **Definition of Done** lists
them in run order and is the one place that does, so this
paragraph does not repeat the list -- two copies of it drifted
apart once already. It also says why `audit` degrades to a
warning inside `validate` and errors when run alone.
