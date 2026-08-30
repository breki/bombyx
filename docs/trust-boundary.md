# Where project code is allowed to exist

This records a decision about the one thing bombyx exists to
control: which machines are allowed to hold the source code of
the project an agent works on. The decision is written down
because the reasoning is easy to lose and expensive to rebuild,
and because several planned pieces of work only make sense once
you know which way it went.

> **This is a decision, not a description. bombyx does not
> work this way yet.**
>
> "The boundary" below states the target. "Where project code
> lives today" states the current behaviour, which is
> different, and which was read from `crates/bombyx/src/plan.rs`
> rather than recalled. Nothing here has been built, and the
> `README.md` still describes the mechanism being replaced.
>
> Treat every section after "Two ways to satisfy the
> constraint" as a design that has been decided and not yet
> tested against anything.

## The boundary

The guest is the only machine that holds the project's source
code. Neither the workstation nor the VM host keeps a copy, a
clone, or a cache of it.

What they do hold is a repository URL, a commit, and host
configuration. That is metadata about the project rather than
the project, and reading it tells an attacker where the code
came from but does not hand them the code or anything derived
from running it.

## Where project code lives today

`bombyx up` builds an archive of the project's `vagrant/`
directory on the workstation and unpacks it on the VM host. The
commands, from the expected argv in
`crates/bombyx/src/plan.rs`:

```
tar -czf .bombyx-push-<n>.tar.gz -C <vagrant_dir> .
scp .bombyx-push-<n>.tar.gz vmhost:.bombyx-push-<n>.tar.gz
ssh vmhost "cd ~/vms/<project> && tar -xzf ~/.bombyx-push-<n>.tar.gz"
```

Three machines end up holding project files. The workstation
holds the checkout the archive is built from. The VM host holds
the unpacked copy. The guest holds a third copy through
Vagrant's synced folder.

The first of those three is the machine the whole design exists
to protect. `README.md` opens by saying that running an agent
on your daily driver puts your credentials one prompt injection
away from exfiltration, and then the tool requires a checkout on
exactly that machine.

## Why the guest cannot simply hold everything

Vagrant reads the Vagrantfile in order to create the VM, so the
file has to exist before the VM does. The project's files
therefore cannot first appear inside the guest. Something
outside the guest has to hold them, and that is the constraint
the rest of this document works around.

The constraint is narrower than it first looks, and the
difference matters. It applies to the Vagrantfile, not to the
project. Anything Vagrant does not need in order to boot a
machine can arrive later, from inside the guest.

## Two ways to satisfy the constraint

**The VM host joins the trusted computing base.** It holds a
clone, Vagrant reads the Vagrantfile out of that clone, and the
workstation is left with nothing. This is the smaller change.
The project keeps its `vagrant/` directory, and only the place
the files come from moves.

**Nothing outside the guest holds project code.** Then the
Vagrantfile cannot come from the project at all, so bombyx has
to generate it, and the guest clones the project itself once it
is running.

The second is the decision.

The argument against the first option is that it relocates the
exposure instead of removing it. If code inside the VM may be
hostile, then any machine holding a copy of that code can be
attacked for it, and under the first option the VM host holds
that copy.

A firewall on the host narrows that, and does not close it.
`docs/vm-host-setup.md` describes an nftables ruleset whose
input chain drops new connections arriving on the guest bridge,
accepting only established traffic and DHCP and DNS from the
gateway address. Once those rules are loaded, a guest cannot
open an SSH session to the host. Two things remain true anyway.

The rules are not loaded. That work is captured as
`host-network-isolation`, the section describing it is marked
*(unverified)*, and applying it needs a password on the host,
so on any host set up as documented today the path is open.

More durably: the rules filter packets, and packets are not the
only way to the host. The host runs the hypervisor that the
guest executes on, so a hypervisor escape reaches the host and
whatever it stores without crossing the bridge. A firewall is
the right precaution and it is not a reason to put the
project's source on the machine running the hypervisor.

That the first option is smaller is true, and it is not
sufficient.

## What the sequence becomes

1. bombyx generates a minimal Vagrantfile from its own
   per-provider template. The project contributes nothing at
   this point, because nothing of the project is available.
2. Vagrant boots a clean VM from a base box.
3. A generic bootstrap provisioner, shipped by bombyx, runs
   inside the guest.
4. The bootstrap clones the project inside the guest.
5. The guest runs the project's own lifecycle hooks.

Generating the Vagrantfile is what makes the rest possible. It
is not a tidying-up step that can be deferred.

## What this costs

Three costs are worth stating plainly, because the boundary
reads as tighter than it is.

**The guest needs a credential, and hostile code can read it.**
Cloning a private repository requires one, and the credential
has to be inside the machine whose contents are assumed
untrustworthy. Scoping it -- read-only, one repository,
short-lived -- limits what stealing it is worth. Scoping does
not prevent the theft. This is an accepted exposure rather than
a solved problem, and it qualifies the phrase "no credentials"
in `README.md`. No alternative has been designed. A forwarded
agent, a fetch proxy on the VM host, or source baked into a
base image would each change the picture, and none of them
exists.

**Nothing can size the VM before the VM exists.** A project
that declares its memory and CPU needs in its own repository
hits the same ordering problem the Vagrantfile does: bombyx
cannot read those numbers until after the machine it needs them
for has booted. Either the sizing lives in configuration held
on the workstation or the VM host, or the boot happens in two
phases. This is unresolved.

**The guest has to reach the git host, and the network rules
may forbid it.** `docs/vm-host-setup.md` describes an nftables
ruleset that keeps agent VMs off the home network. A guest that
cannot resolve and reach the repository cannot clone it, so the
egress allowed by those rules has to include the git host
deliberately. Getting this wrong fails at clone time rather
than at boot, which is late and confusing.

## What is not built yet

None of it. The captured work sits in `docs/todo.md`:

- `generate-vagrantfile` -- bombyx emits the Vagrantfile from
  per-provider templates. Everything else depends on this.
- `minimal-vagrantfile` -- what the generator should emit, and
  nothing more.
- `remote-clone-project-source` -- the workstation supplies a
  URL and a commit, and the guest clones.
- `provision-lifecycle-hooks` -- how step 5 is specified.
- `per-host-resource-profiles` -- carries the sizing question
  above.
- `host-network-isolation` -- carries the egress question
  above.

Until those land, `bombyx up` behaves as described under
"Where project code lives today".
