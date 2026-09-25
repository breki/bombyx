# Where project code is allowed to exist

This document records one decision: which machines may hold the
source code of the project an agent works on. The reasoning is
written down because it is easy to lose and expensive to rebuild,
and because several planned pieces of work depend on it.

> **Neither boundary statement below is confirmed against a remote
> VM host.** The first is also a property of a *machine* rather
> than of bombyx: a workstation someone develops on holds the code
> whatever bombyx does. What this work reached is that bombyx
> neither requires a checkout nor puts a project file anywhere
> outside the guest.

## The boundary

Two statements, the second stronger than the first:

- **Only the guest holds the source.** Neither the workstation
  nor the VM host keeps a copy, a clone, or a cache.
- **Neither other machine reads any repository file.** Not the
  source, not a config file, not a `vagrant/` directory. A
  repository bombyx never opens cannot decide what runs outside
  the VM.

The workstation may still hold a repository URL, a commit, and
host configuration. These live in its own configuration, not in
the repository -- metadata about the project rather than the
project. They tell an attacker where the code came from without
handing over the code.

Two qualifications go with the boundary, and this document owns
both:

| Qualification | What it means |
|---|---|
| The guest's disk image | Covered under **Where project code lives today** -- the clone lives inside a file on the VM host. |
| "Reads no file" is about files | `--config <path>` reads whatever it names, and `BOMBYX_CONFIG_HOME` need only be anchored, so a per-directory tool (`direnv`, `mise`, a CI job) can redirect the loader from inside a clone. |

The config values are checked against an allowlist as the config
loads -- an anchored `remote_root`, a single-segment scratch name,
an `ssh` `host` with no leading dash -- and those checks are a
boundary, not a typo check, because the values can come from a
repository. The `config` modules hold the exact rules.

## Where project code lives today

The guest clones its own copy. bombyx puts a project file on
neither of the other two machines and reads one from neither, so
the only copy either can hold is one the operator put there
themselves. This was read from `crates/bombyx/src/plan.rs`, not
recalled.

One qualification comes from the threat model:

- The guest's virtual disk is a file on the VM host, and the
  clone lives inside it, so **root on the VM host can read the
  project by mounting that image**.
- What bombyx controls is that it writes no project file into the
  host's *own* filesystem, where any process could read it without
  touching the guest.
- A compromised VM host still learns the project, because it runs
  the hypervisor. That stronger property was never on offer.

bombyx writes three files onto the VM host, all generated from
`[vm]` and `[source]` rather than taken from the repository:

| File | Generated from | Notes |
|---|---|---|
| Vagrantfile | `[vm]`, `[source]`, `[env]` | Carries every `[env]` value; stays on the host for the life of the VM; written owner-only (default umask would be world-readable); disables Vagrant's default `/vagrant` share. |
| Account script | nothing; the same for every project | Runs inside the guest as root, first: creates the `guest_user` account, gives it passwordless `sudo`, moves the staged credentials into its home, and hands over to the bootstrap script. Reads nothing from the repository. |
| Bootstrap script | nothing; the same for every project | Runs inside the guest as the `guest_user` account, to clone the project and run its hooks. Its per-project inputs arrive through the Vagrantfile's `env:`. |

All three travel on the commands' standard input, not in their
arguments. This is a trust decision: every account on a Unix
machine can list the full command line of every running process,
so an argument would be readable by every other account while the
write runs.

## Why the guest cannot hold everything

- Vagrant reads the Vagrantfile to create the VM, so the file
  must exist **before** the VM does.
- The project's files therefore cannot first appear inside the
  guest -- something outside it has to hold them.
- The constraint is narrower than it looks: it applies to the
  Vagrantfile, not the project. Anything Vagrant does not need in
  order to boot can arrive later, from inside the guest.

## Two ways to satisfy the constraint

| Option | Who holds code | Change size |
|---|---|---|
| VM host joins the trusted computing base | Host holds a clone; Vagrant reads the Vagrantfile from it; workstation holds nothing | Smaller: project keeps its `vagrant/` directory, only the file source moves |
| **Nothing outside the guest holds project code** (chosen) | bombyx generates the Vagrantfile; the guest clones the project once running | Larger, but removes the exposure rather than moving it |

bombyx takes the second option. The first only relocates the
exposure: if code inside the VM may be hostile, any machine
holding a copy can be attacked for it, and under the first option
that machine is the VM host.

A firewall on the host narrows this but does not close it:

- `docs/vm-host-firewall.md` describes an nftables ruleset that
  drops new connections on the guest bridge, so a guest cannot
  open an SSH session to the host.
- **The rules are not loaded.** That work is
  `host-network-isolation`, its section is marked *(unverified)*,
  and applying it needs a host password -- so on a host set up as
  documented today the path is open.
- **Packets are not the only way in.** The host runs the
  hypervisor the guest executes on, so a hypervisor escape reaches
  it without crossing the bridge.

This is not hypothetical. Docker Sandboxes isolates coding agents
the same way -- a small VM with the project shared in -- and kept
a host-guest file share and a guest-to-host socket relay. Both
were escaped from inside the sandbox by planted symlinks in
September 2026 (CVE-2026-77179, CVE-2026-79994), giving in-sandbox
code read and write on host files. bombyx runs no such share.

The resulting `bombyx up` sequence -- bombyx generates the
Vagrantfile, Vagrant boots a clean VM, a bombyx bootstrap
provisioner runs, it clones the project inside the guest, and the
guest runs the project's own hooks -- is in `docs/architecture.md`.

## What this costs

The boundary reads tighter than it is. The items below are the
accepted exposures; the code holds how each is implemented.

### The guest needs a credential

Cloning a private repository requires a credential, and it has to
live inside the machine whose contents are assumed untrustworthy.
Scoping it -- read-only, one repository, short-lived -- limits
what a stolen copy reaches but does not prevent the theft. This
qualifies the phrase "no credentials" in `README.md`.

Three secrets can reach the guest:

| Secret | What it is | Named on |
|---|---|---|
| `deploy_key` | A private key | The VM host |
| `env_file` | A file of secrets | The workstation |
| `repo_token` | A git token built from a variable inside `env_file` | (derived) |

Each takes a route that keeps it off both machines' command lines
and out of every generated file, and the VM host holds its copy
only for the length of the `vagrant` run.

`up` and `shell` also rewrite the `env_file` copy and the
`repo_token` credential inside a guest that already exists, so a
rotated token reaches it without a provision. That route stores
nothing on the VM host at all. Each file travels on a pipe: from
bombyx to `ssh`, through `vagrant ssh --no-tty` on the VM host,
and into a `cat` that the agent's account runs in the guest. On the
VM host the file exists only in the memory of the processes
passing it along. Nothing in the path asks for a terminal, because
a terminal's line discipline can echo input back into the output.
We have not checked whether Vagrant's own debug log
(`VAGRANT_LOG=debug`, set on the VM host) records what passes
through; the staging route has the same unknown for its upload.

The outcome is the same
for all three and is the cost: **the agent needs the value to
work, so code in the guest can read it.** The design changes how
many machines hold a copy on the way, not whether one arrives.

The choice of token decides the blast radius:

| Token | Reaches |
|---|---|
| Bitbucket repository access token | One repository (an SSH access key there is read-only, so a token is the only push credential) |
| Atlassian API token | Every repository the account can see, plus Jira and Confluence |

Which token is in the VM is the operator's choice.

The presence check for `deploy_key` runs in bombyx, not in the
Vagrantfile: `vagrant destroy` loads the Vagrantfile too, so a
Vagrantfile that tested the key and raised would leave a directory
no bombyx command could tear down. bombyx knows which verb it is
running; the Vagrantfile does not.

### The agent can read the credential

- The workstation never holds the deploy key: the path travels in
  the Vagrantfile, and `vagrant` on the host reads it, so the key
  exists on the VM host and in the guest and nowhere else.
- Inside the guest the key is owned by the agent's own account,
  `guest_user`, and that is deliberate: work leaves the VM by being
  pushed, and pushing needs the key. A key the agent could not read
  would be a key it could not push with.
- `bootstrap.sh` tightens the mode to keep *other* guest accounts
  out; against the agent itself the mode buys nothing, and nothing
  can. Giving the agent an account of its own, rather than
  `vagrant`, does not change this either, because that account has
  passwordless `sudo`.

### Host-key verification

The guest verifies the git host when bombyx knows where it
publishes its keys, and trusts it on first sight otherwise. The
table in `crates/bombyx/src/hostkeys.rs` holds two:

| Host | Published keys at | Behaviour |
|---|---|---|
| `github.com` | `https://api.github.com/meta` | `bootstrap.sh` fetches the keys before cloning and checks strictly |
| `bitbucket.org` | `https://bitbucket.org/site/ssh` | same |
| any other host | -- | `accept-new`: accept the key offered on first connection, refuse a change afterward |

For the known hosts, the trust does not disappear, it moves: it
now rests on the certificate authority behind those HTTPS
endpoints -- a chain bombyx already depends on to download the box
-- rather than on whatever answers on port 22.

- **A failed fetch refuses the run**, rather than falling back to
  the weaker check. Someone able to impersonate the git host on
  port 22 can usually block an HTTPS request too, so a fallback
  would be a check they could switch off at will. A fetch that
  fails, or returns nothing naming the host, stops provisioning
  with a message naming the URL.
- **`accept-new` still protects the private key on the first
  connection.** SSH public-key auth signs the session identifier,
  which is bound to the server's host key, so the signature cannot
  be replayed against the real git host. What an attacker gets is
  the ability to serve a repository of their own -- and
  `bootstrap.sh` runs the script from whatever it clones, as the
  key's owner. So the key does leave, by code execution rather
  than by the handshake. The `host-network-isolation` egress rules
  would narrow this; they are not loaded.

What this defends and does not defend:

- **Defends:** an attacker between the guest and the git host on
  the first connection, before any project code exists in the
  guest.
- **Does not defend:** the guest itself. The project's script runs
  with `sudo`, and again on every `bombyx provision`, so after the
  first run the guest owns `/etc/hosts`, the resolver, the
  certificate store, and the fetched `known_hosts` file. A guest
  that has run untrusted code with `sudo` cannot be defended from
  inside itself -- which is why the VM is disposable, and
  `bombyx destroy` and the egress rules are what narrow it.

The verification also stops at the clone: `bootstrap.sh` writes
the host keys into the clone rather than exporting them, so it
constrains bombyx's own `git` commands and leaves the project's
alone. `bootstrap.sh` and `hostkeys.rs` hold the mechanism.

### A revoked credential persists

- Removing `deploy_key` from the config makes `bootstrap.sh`
  delete the key on the next `bombyx provision`.
- But `bombyx up` saves the `fresh-install` snapshot *after*
  provisioning, so the snapshot's disk still holds the key, and
  `bombyx reset` restores it -- measured on a real VM. `env_file`
  behaves the same way. A revoked key returns on every reset for
  the life of the VM.
- `bombyx destroy` removes it for certain, because it takes the
  disk and the snapshot with it. `bombyx snapshot` helps only if
  you re-snapshot after a provision that has already deleted the
  key.
- Whichever route, **revoke the key at the git host as well** --
  the only step that does not depend on a guest doing what it was
  told.

`docs/developer/redteam-log.md` records why this is open, and
GitHub issue #57 carries the decision it waits on.

### Open problems

| Problem | Why it is open | Tracked by |
|---|---|---|
| **Sizing** | bombyx cannot read a project's CPU and memory needs from its repository, because it cannot read the repository until the machine those numbers size has booted. So sizing lives in configuration, or the boot has to happen in two phases. | `per-host-resource-profiles` in `docs/todo.md` |
| **Egress** | The guest must reach the git host to clone, so a host firewall (`docs/vm-host-firewall.md`) has to allow that egress deliberately. Getting it wrong fails at clone time rather than at boot. | `host-network-isolation` in `docs/todo.md` |
