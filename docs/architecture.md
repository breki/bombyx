# Architecture

bombyx runs `vagrant` on a remote VM host over SSH, so an agent
works inside a VM and the workstation stays clean. It composes
`ssh`, `scp`, `tar` and `vagrant` and reimplements none of them.

> The diagrams are PlantUML. GitHub does not render it inline,
> so read them as source or paste them into a renderer.
> **They have not been rendered**: this workstation has neither
> `plantuml` nor a JVM, so the syntax is unverified.

## The three machines

```plantuml
@startuml
skinparam componentStyle rectangle

node "workstation" as ws {
  component "bombyx" as cli
  folder "project repo\nbombyx.toml, vagrant/" as repo
}

node "VM host" as host {
  component "vagrant" as vg
  folder "~/vms/<project>\nVagrantfile, bootstrap.sh" as dir
}

node "agent VM (guest)" as guest {
  folder "/opt/project\n(cloned here)" as clone
  component "agent" as agent
}

cloud "git host" as git

repo --> cli
cli --> vg : ssh
cli --> dir : scp + heredoc
vg --> guest : creates
clone <-- git : clone
agent --> clone

@enduml
```

The workstation never runs project code. The VM host runs
`vagrant` and holds a directory per project. The guest is the
only machine that clones the repository.

`docs/trust-boundary.md` records which machines are allowed to
hold project source, and what is still unfinished.

## Library modules

Everything with a decision in it lives in the library, because
`src/bin/` is outside the coverage gate.

```plantuml
@startuml
skinparam componentStyle rectangle

package "bin" {
  [main]
}

package "bombyx (lib)" {
  [plan]
  [vagrantfile]
  [doctor]
  [update]
  [remote]
  [config]
  [name]
  [term]
  [tool]
}

[main] --> [plan]
[main] --> [update]
[main] --> [term]
[main] --> [tool]
[main] --> [doctor]

[plan] --> [vagrantfile]
[plan] --> [doctor]
[plan] --> [remote]
[plan] --> [config]
[plan] --> [name]

[vagrantfile] --> [remote]
[vagrantfile] --> [config]

[update] --> [remote]
[update] --> [config]

[doctor] <--> [remote]

[remote] --> [config]
[remote] --> [name]
[config] --> [name]

@enduml
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

## `bombyx up`, end to end

```plantuml
@startuml
autonumber

actor operator
participant "bombyx\n(workstation)" as cli
participant "shell\n(VM host)" as host
participant "vagrant\n(VM host)" as vg
participant "guest VM" as guest
participant "git host" as git

operator -> cli : bombyx up
cli -> cli : read bombyx.toml, validate
cli -> host : mkdir -p ~/vms/<project>
cli -> cli : tar -czf <archive> -C vagrant/
cli -> host : scp <archive>
cli -> host : tar -xzf, rm <archive>
cli -> host : cat > Vagrantfile <<'BOMBYX_EOF'
cli -> host : cat > bootstrap.sh <<'BOMBYX_EOF'
cli -> vg : vagrant up
vg -> guest : create from box
vg -> guest : run bootstrap.sh
guest -> git : git clone <repo> <ref>
guest -> guest : run <script> from the clone
guest --> operator : VM ready

@enduml
```

Two things about the order. The generated files land **after**
the archive is unpacked, or the push would overwrite them. And
`vagrant` runs last, because it reads the Vagrantfile.

`provision` is the same sequence ending in `vagrant provision`,
which exists because vagrant runs provisioners only when it
first creates a machine.

## Where the rules live

A rule goes in the module that owns the value, not at each
call site.

```plantuml
@startuml
skinparam componentStyle rectangle

file "bombyx.toml" as toml
component "config::validate" as val
component "vagrantfile::render" as render
component "remote::write_file" as write
file "Vagrantfile\n(on VM host)" as out

toml --> val
val --> render : validated Config
render --> write : Ruby text
write --> out : quoted heredoc

note bottom of val
  refuses what breaks Ruby,
  what git reads as an option,
  and paths leaving the clone
end note

note bottom of write
  lengthens the delimiter until
  no payload line equals it
end note

@enduml
```

Each stage is safe on its own. `render` escapes Ruby even
though `validate` already refused the characters, and
`write_file` picks a delimiter no payload can contain rather
than trusting either. A guard in another module is the one a
new field gets added without.

## Quality gates

`cargo xtask validate` runs nine, cheapest first: dependency
cooldown, formatting, duplication, licences, clippy, doc links,
tests, coverage, security audit. `CLAUDE.md` has the detail,
including why `audit` degrades to a warning inside `validate`
and errors when run alone.
