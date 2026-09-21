# Where project code is allowed to exist

This document records one decision: which machines may hold the
source code of the project an agent works on. The reasoning is
written down because it is easy to lose and expensive to rebuild,
and because several planned pieces of work depend on which way it
went.

> **Neither statement below is confirmed against a remote VM
> host.** The first statement is also a property of a *machine*
> rather than of bombyx: a workstation someone develops on holds
> the code whatever bombyx does. What this work reached is that
> bombyx neither requires a checkout nor puts a project file
> anywhere outside the guest.

## Contents

- [The boundary](#the-boundary)
- [Where project code lives today](#where-project-code-lives-today)
- [Why the guest cannot hold everything](#why-the-guest-cannot-hold-everything)
- [Two ways to satisfy the constraint](#two-ways-to-satisfy-the-constraint)
- [What this costs](#what-this-costs)
  - [The guest needs a credential](#the-guest-needs-a-credential)
  - [The agent can read the credential](#the-agent-can-read-the-credential)
  - [Host-key verification](#host-key-verification)
  - [A revoked credential persists](#a-revoked-credential-persists)
  - [Open problems](#open-problems)

## The boundary

There are two statements, and the second is stronger.

**The guest is the only machine that holds the project's source
code.** Neither the workstation nor the VM host keeps a copy, a
clone, or a cache.

**Neither machine reads any file from the project's repository.**
Not the source, and not a config file or a `vagrant/` directory. A
repository bombyx never opens cannot decide what runs on the
machines outside the VM.

The workstation may still hold a repository URL, a commit, and
host configuration. These live in its own configuration, not in
the repository. They are metadata about the project rather than
the project: they tell an attacker where the code came from
without handing over the code or anything derived from running it.

Two qualifications go with the boundary, and this document owns
both. The first is the guest's disk image, described under **Where
project code lives today**. The second is that "reads no file from
the repository" is a rule about files, not about everything a
repository can reach. bombyx reads the file `--config <path>`
names, whatever it is, and `BOMBYX_CONFIG_HOME` only has to be
anchored, so a per-directory tool (`direnv`, `mise`, a CI job) can
redirect the loader from inside a clone. `docs/usage.md` under
**What is checked, and what is not** lists what the values are
checked for; those checks are a boundary rather than a typo check,
because the values can come from a repository.

## Where project code lives today

The guest clones its own copy. bombyx puts a project file on
neither of the other two machines and reads one from neither, so
the only copy either can hold is one the operator put there for
their own reasons. This behaviour was read from
`crates/bombyx/src/plan.rs` rather than recalled.

That claim carries one qualification from the threat model. The
guest's virtual disk is a file on the VM host's filesystem, and
the clone lives inside it, so root on the VM host can read the
project by mounting that image. What bombyx controls is that it
puts no project file into the host's *own* filesystem, where any
process could read it without touching the guest. A compromised VM
host still learns the project, because the host runs the
hypervisor; that stronger property was never on offer.

bombyx writes two files onto the VM host, both generated from
`[vm]` and `[source]` rather than taken from the repository: a
Vagrantfile and a bootstrap script. They travel on the commands'
standard input rather than in their arguments, and this is a trust
decision. Every account on a Unix machine can list the full
command line of every running process, so a file passed as an
argument would be readable by every other account while the write
runs. `docs/usage.md` under **How the generated files are
written** describes the mechanism.

The generated Vagrantfile carries every value from the project's
`[env]` table and stays on the VM host for the life of the VM. At
the account's default umask it would be world-readable, so the
write restricts the mode to the owner. The VM host's owner and
root can still read it; a mode stops other accounts, not the
administrator. The Vagrantfile also disables Vagrant's default
`/vagrant` share, which would otherwise mount its own directory
into the guest.

## Why the guest cannot hold everything

Vagrant reads the Vagrantfile in order to create the VM, so the
file must exist before the VM does. The project's files therefore
cannot first appear inside the guest. Something outside the guest
has to hold them, and that is the constraint the rest of this
document works around.

The constraint is narrower than it looks: it applies to the
Vagrantfile, not to the project. Anything Vagrant does not need in
order to boot a machine can arrive later, from inside the guest.

## Two ways to satisfy the constraint

**Let the VM host join the trusted computing base.** It holds a
clone, Vagrant reads the Vagrantfile from that clone, and the
workstation holds nothing. This is the smaller change: the project
keeps its `vagrant/` directory, and only the source of the files
moves.

**Let nothing outside the guest hold project code.** Then the
Vagrantfile cannot come from the project, so bombyx generates it,
and the guest clones the project once it is running.

bombyx takes the second option. The first relocates the exposure
instead of removing it: if code inside the VM may be hostile, any
machine holding a copy of that code can be attacked for it, and
under the first option the VM host holds that copy.

A firewall on the host narrows this but does not close it.
`docs/vm-host-firewall.md` describes an nftables ruleset that
drops new connections on the guest bridge, so a guest cannot open
an SSH session to the host. Two things remain true. First, the
rules are not loaded: that work is `host-network-isolation`, its
section is marked *(unverified)*, and applying it needs a password
on the host, so on a host set up as documented today the path is
open. Second, the rules filter packets, and packets are not the
only way to the host: the host runs the hypervisor the guest
executes on, so a hypervisor escape reaches it without crossing
the bridge. A firewall is the right precaution, not a reason to
put the source on the machine running the hypervisor.

This is not hypothetical. Docker Sandboxes isolates coding agents
the same way -- a small VM with the project shared in -- and kept
a host-guest file share and a guest-to-host socket relay. Both
were escaped from inside the sandbox by planted symlinks in
September 2026 (CVE-2026-77179, CVE-2026-79994), giving in-sandbox
code read and write on host files. bombyx runs no such share.

The resulting sequence -- bombyx generates the Vagrantfile,
Vagrant boots a clean VM, a bombyx bootstrap provisioner runs, it
clones the project inside the guest, and the guest runs the
project's own hooks -- is the `bombyx up` flow in
`docs/architecture.md`.

## What this costs

The boundary reads as tighter than it is. The costs below are the
accepted exposures; the code and `docs/usage.md` hold how each one
is implemented.

### The guest needs a credential

Cloning a private repository requires a credential, and that
credential has to be inside the machine whose contents are assumed
untrustworthy. Scoping it -- read-only, one repository,
short-lived -- limits what a stolen copy reaches but does not
prevent the theft. This is an accepted exposure, and it qualifies
the phrase "no credentials" in `README.md`.

Three secrets can reach the guest: a `deploy_key`, a private key
named on the VM host; an `env_file`, a file of secrets named on
the workstation; and a `repo_token`, a git token built from a
variable inside that file. Each takes a route that keeps it off
both machines' command lines and out of every generated file, and
the VM host holds its copy only for the length of the `vagrant`
run. The `config` modules and `docs/usage.md` describe the
staging. The outcome is the same for all three, and it is the
cost: the agent needs the value to work, so code in the guest can
read it. The design changes how many machines hold a copy on the
way, not whether one arrives.

The choice of token decides the blast radius. On Bitbucket a token
is the only credential an agent can push with, because an ssh
access key there is read-only. A repository access token reaches
one repository; an Atlassian API token reaches every repository
the account can see, and Jira and Confluence with it. Which token
is in the VM is the operator's choice.

The presence check for `deploy_key` runs in bombyx, not in the
Vagrantfile, and the obvious place is wrong: `vagrant destroy`
loads the Vagrantfile too, so a Vagrantfile that tested the key
and raised would leave a directory no bombyx command could tear
down. bombyx knows which verb it is running, and the Vagrantfile
does not.

### The agent can read the credential

The workstation never holds the deploy key: the path travels in
the Vagrantfile, and `vagrant` on the VM host reads it, so the key
exists on the VM host and in the guest and nowhere else. Inside
the guest the key is owned by the agent's own user, and that is
deliberate. Work leaves the VM by being pushed, and pushing needs
the key, so a key the agent could not read would be a key the
agent could not push with. `bootstrap.sh` tightens the mode to
keep *other* accounts in the guest out, but against the agent
itself the mode buys nothing, and nothing can: an agent that can
push is an agent that holds the credential. Running the
provisioner unprivileged does not change this, because that user
has passwordless `sudo`.

### Host-key verification

The guest verifies the git host when bombyx knows where the host
publishes its keys, and trusts it on first sight otherwise. The
table in `crates/bombyx/src/hostkeys.rs` holds two: `github.com`,
which publishes its keys at `https://api.github.com/meta`, and
`bitbucket.org`, at `https://bitbucket.org/site/ssh`. For those,
`bootstrap.sh` fetches the published keys before it clones and
checks the host strictly against them. The trust does not
disappear, it moves: it now rests on the certificate authority
that vouches for those HTTPS endpoints -- a chain bombyx already
depends on to download the box -- rather than on whatever answers
on port 22.

A failed fetch refuses the run rather than falling back to the
weaker check. Someone able to impersonate the git host on port 22
is usually able to block an HTTPS request too, so a fallback to
`accept-new` would be a check they could switch off at will. A
fetch that fails, or that returns nothing naming the host, stops
provisioning with a message naming the URL.

Every other host -- a self-hosted server, or anything absent from
the table -- gets `accept-new`: the guest accepts the key it is
offered on the first connection and refuses a change afterward. An
attacker on that first connection does not get the private key,
because SSH public-key authentication signs the session
identifier, which is bound to the server's host key, so the
signature cannot be replayed against the real git host. What they
get is the ability to impersonate the host and serve a repository
of their own, and `bootstrap.sh` runs the script from whatever it
clones, as the agent's own user -- the key's owner. So the key
does leave, by code execution rather than by the handshake. The
egress rules under `host-network-isolation` would narrow this, and
they are not loaded.

This defends against an attacker between the guest and the git
host on the first connection, before any project code exists in
the guest. It does not defend against the guest itself. The
project's script runs with `sudo`, and it runs again on every
`bombyx provision`, so after the first the guest owns
`/etc/hosts`, the resolver, the certificate store, and the fetched
`known_hosts` file. A guest that has run untrusted code with
`sudo` cannot be defended from inside itself; that is why the VM
is disposable, and `bombyx destroy` and the egress rules are what
narrow it. The verification also stops at the clone: `bootstrap.sh`
writes the host keys into the clone rather than exporting them, so
it constrains bombyx's own `git` commands and leaves the project's
alone. `bootstrap.sh` and `hostkeys.rs` hold the mechanism.

### A revoked credential persists

Taking `deploy_key` out of the config makes `bootstrap.sh` delete
the key on the next `bombyx provision`. But `bombyx up` saves the
`fresh-install` snapshot *after* provisioning, so the snapshot's
disk still holds the key, and `bombyx reset` restores it --
measured on a real VM. `env_file` behaves the same way. So a
revoked key returns on every reset for the life of the VM.
`bombyx destroy` removes it for certain, because it takes the disk
and the snapshot with it; `bombyx snapshot` helps only if you
re-snapshot after a provision that has already deleted the key.
Whichever route, revoke the key at the git host as well -- that is
the only step that does not depend on a guest doing what it was
told. `docs/developer/redteam-log.md` records why this is open,
and GitHub issue #57 carries the decision it waits on.

### Open problems

**Sizing.** bombyx cannot read a project's CPU and memory needs
from its repository, because it cannot read the repository until
the machine those numbers size has booted. The sizing therefore
lives in configuration, or the boot has to happen in two phases.
`per-host-resource-profiles` in `docs/todo.md` carries it.

**Egress.** The guest has to reach the git host to clone it, so a
host firewall (`docs/vm-host-firewall.md`) has to allow that
egress deliberately. Getting it wrong fails at clone time rather
than at boot. `host-network-isolation` in `docs/todo.md` carries
it.
