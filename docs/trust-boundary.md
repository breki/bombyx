# Where project code is allowed to exist

This records a decision about the one thing bombyx exists to
control: which machines are allowed to hold the source code of
the project an agent works on. The decision is written down
because the reasoning is easy to lose and expensive to rebuild,
and because several planned pieces of work only make sense once
you know which way it went.

> **Both statements are now reached as far as bombyx is
> concerned. We have not confirmed either against a remote VM
> host.**
>
> Statement one is a property of a *machine*, so no change to
> bombyx can establish it: a workstation someone develops the
> project on holds the code whatever bombyx does. What this
> work reached is that bombyx neither requires a checkout nor
> puts a project file anywhere outside the guest.
>
> "The boundary" states the target. "Where project code lives
> today" states the current behaviour, which was read from
> `crates/bombyx/src/plan.rs` rather than recalled.
>
> What landed: bombyx generates the Vagrantfile and writes it on
> the VM host, the guest clones the project itself, the push is
> gone, and every setting now comes out of the operator's own
> `config.toml` with `--project` naming the project. So neither
> the workstation nor the VM host opens a file in the project's
> repository, and the workstation needs no checkout.

## The boundary

There are two statements here, and the second is stronger.

**The guest is the only machine that holds the project's source
code.** Neither the workstation nor the VM host keeps a copy, a
clone, or a cache of it.

**Neither machine reads any file from the project's
repository.** Not the source, and not a config file or a
`vagrant/` directory either. A repository that bombyx has never
opened cannot decide what runs on the machines outside the VM.

What the workstation may still hold is a repository URL, a
commit, and host configuration -- kept in its own configuration,
not read out of the repo. That is metadata about the project
rather than the project, and it tells an attacker where the code
came from without handing them the code or anything derived from
running it.

Both statements are now reached as far as bombyx is concerned,
which the note at the top of this document qualifies. Outside
the guest's own disk image, the VM host holds no project file. The
workstation reads one file, `config.toml` in the operator's own
config directory, and opens nothing in the project's directory
-- so it needs no checkout, and `--project` is what tells
bombyx which project a command is about. The work that got here
is `project-config-off-repo`.

Two qualifications go with that, and this document owns both.
The first is the guest's disk image, below. The second is that
"opens nothing in the project's directory" is a rule about
files and not about everything a repository can reach: bombyx
reads the file `--config <path>` names, whatever it is, and
`BOMBYX_CONFIG_HOME` only has to be anchored, so a
per-directory environment tool (`direnv`, `mise`, a CI job)
redirects the loader from inside a clone. `docs/usage.md` under
**What is checked, and what is not** says what the value checks
then protect, and `docs/architecture.md` under **What config
values are checked** says why they are a boundary rather than a
typo check.

## Where project code lives today

The guest clones its own copy. bombyx puts a project file on
neither of the other two machines and reads one from neither, so
the only copy either of them can hold is one somebody else put
there -- a checkout the operator makes for their own reasons.

That last claim needs one qualification, and the threat model
below is why. The guest's virtual disk is a file on the VM
host's filesystem, and the clone lives inside it, so root on
the VM host can read the project by mounting that image. What
changed is that bombyx no longer puts a project file into the
host's own filesystem, where any process could read it without
touching the guest. The stronger property -- that a compromised
VM host learns nothing about the project -- was never on offer,
because the host runs the hypervisor.

`bombyx up` runs five commands, from the expected argv in
`crates/bombyx/src/plan.rs`, and every one of them is an `ssh`:

```
ssh vmhost "mkdir -p ~/vms/<project>"
ssh vmhost "cat > ~/vms/<project>/Vagrantfile <<'BOMBYX_EOF' ..."
ssh vmhost "cat > ~/vms/<project>/bootstrap.sh <<'BOMBYX_EOF' ..."
ssh vmhost "cd ~/vms/<project> && vagrant up"
ssh vmhost "cd ~/vms/<project> && vagrant snapshot save ..."
```

The fifth is conditional in the script rather than in the plan:
it saves the `fresh-install` snapshot only when the machine has
none, so the reset cycle works without anyone asking for it.

A configured `deploy_key` adds one more command, and it runs
first:

```
ssh vmhost "p=~/'.secrets/k'; if [ ! -f \"$p\" ] ...; fi"
```

It tests that the key file is on the VM host and readable, and
exits non-zero when it is not. It reads one file's presence and
never its contents, so it moves no project code anywhere.

The two files bombyx writes are generated by bombyx itself, from
`[vm]` and `[source]` in the configuration. Neither comes out of
the project's repository, so the VM host holds no project file
except the guest's own disk image, and the guest clones the
repository once it is running.

Three machines held copies bombyx put there, and two of those
are gone. Vagrant mounts the Vagrantfile's directory at
`/vagrant` unless told otherwise, which put the VM host's copy
inside the guest; the generated Vagrantfile disables that
share. The VM host's own copy arrived in a `tar` archive that
no program there read once the share was disabled, and that
push is gone.

The workstation was the last one, and its copy is gone too.
Removing the push stopped bombyx *sending* anything from a
checkout; moving every setting into the operator's own
`config.toml` stopped it *reading* one. `bombyx --project
myproject up` runs the same commands from any directory at all,
so the operator need not be standing in a clone.

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
not prevent the theft. This remains an accepted exposure rather
than a solved problem, and it qualifies the phrase "no
credentials" in `README.md`.

The mechanism is `deploy_key` in `[source]`, which names a
private key file **on the VM host**. Before
`bombyx up`, `provision` or `scratch` creates anything at all,
bombyx checks on the VM host that the file is there and stops
with a message naming the expanded path when it is not. The
generated Vagrantfile then uploads the key with a `file`
provisioner that Vagrant runs before the bootstrap script, and
`bootstrap.sh` tightens it to `0600` where the provisioner
uploaded it, exports `GIT_SSH_COMMAND` so `git` uses that key
and no other, and records the same command on the clone so the
agent can push with it.

Where that check runs is worth a sentence, because the obvious
place is wrong. The Vagrantfile could test the file itself and
`raise`, which needs no extra round trip. It cannot: `vagrant
destroy` loads that file too, so a raising Vagrantfile leaves a
directory that no bombyx command can tear down -- teardown
stops at the failing destroy and never reaches the removal
behind it. bombyx knows which verb it is running, and the
Vagrantfile does not, so the check belongs to bombyx and the
upload in the Vagrantfile stays conditional.

### Four properties of that credential

Each is a deliberate limit rather than an oversight, and all
four belong to the cost above rather than being costs of their
own.

**The workstation never holds the key.** bombyx does not open the
file and does not send it: the path travels in the
Vagrantfile, and `vagrant` on the VM host is what reads it. So
the credential exists on the VM host and inside the guest, and
nowhere else. A design that took the path on the workstation
instead was rejected for exactly this reason -- it would have
put the secret in a third place.

**The agent's own user can read the key, and that is
deliberate.** Vagrant's `file` provisioner uploads as the box's
SSH user, `vagrant` on the boxes bombyx assumes, which is the
user the agent works as, so the key arrives owned by it -- and
that, rather than anything bombyx does, is why the agent can
read it. `bootstrap.sh` only tightens the mode to `0600` and
leaves the file where it is. A box setting a different
`config.ssh.username` would break the assumption, and
`vagrantfile.rs` records it on the constant naming the path.

Putting it out of the agent's reach was tried and does not
survive contact with the job. Work leaves this VM by being
pushed -- a commit made in the guest sits on no branch after the
next provision -- and pushing needs this key. A root-owned key
means an agent that cannot push at all. So `bootstrap.sh` also
records the key on the clone as `core.sshCommand`, which is the
opposite of hiding it.

What the `0600` buys is narrower than it looks: no *other*
account in the guest can read the key. Two cases make it worth
doing, and the box's umask is not one of them -- `scp` sends
the source file's own mode, and a umask only clears bits, so
the uploaded key is never looser than the key on the VM host.
What it does catch is a loosely-permissioned key on the VM
host, delivered as it is, and a file already sitting at that
path, whose mode `scp` does not touch -- so a world-readable
leftover stays that way until bombyx tightens it.

Against the agent itself the mode buys nothing, and nothing
can: an agent that can push is an agent that holds the
credential.

Nor does running as the agent rather than as root put root out
of reach. On the boxes bombyx assumes, that user has
passwordless `sudo`. What the hand-over changes is which step
has to ask for root, not what is reachable from inside the
guest.

**`StrictHostKeyChecking=accept-new` trades a first-contact
check for an unattended clone.** The guest has no `known_hosts`
entry for the git host the first time it runs, and the strict
default would stop at a prompt nobody is there to answer. So
the guest accepts the git host's key on first sight and refuses
a change afterwards.

What an attacker on that first connection gets is worth being
precise about, because the obvious answer is wrong. They do
**not** get the private key: SSH public-key authentication
signs the session identifier, which is bound to the server's
host key, so the signature cannot be replayed against the real
git host and the handshake discloses only the public half.

What they get is the ability to impersonate the git host and
serve a repository of their own. `bootstrap.sh` clones what it
is served and runs the script named by `script` from that
clone. That script runs as the agent's own user rather than as
root, so its first command has the agent's authority and no
more -- which narrows what it reaches and not the outcome,
because the key belongs to that very user. So the key does go,
by way of code execution rather than by way of the handshake.

The egress rules under `host-network-isolation` are what would
narrow that, and they are not loaded. Pre-seeding the guest's
`known_hosts` would close the first-contact window itself, and
nothing does that today.

**Removing `deploy_key` from the config removes the key from
the guest's live disk, and not from its snapshot.**
`bootstrap.sh` deletes the key whenever the config names none,
so the credential goes on the next `bombyx provision`. But
`bombyx up` takes the `fresh-install` snapshot
*after* provisioning, so that snapshot's disk holds the key,
and `bombyx reset` restores it -- measured on a real VM rather
than reasoned about. A later `up` does not refresh the
snapshot either, because it only takes one when the machine
has none.

So a revoked key comes back on every reset for the life of that
VM. `bombyx destroy` is what certainly removes it, because it
takes the disk and the snapshot with it.

`bombyx snapshot` also works and the order matters, so it is
worth spelling out. It saves the *live* disk as the new return
point, which only helps once that disk no longer holds the key:
take `deploy_key` out of the config, run `bombyx provision` so
the guest deletes it, then `bombyx snapshot`. A `snapshot`
before the provision captures the key again, and so does one
taken after a `reset` -- the restore puts the key back, as
above. bombyx warns you about neither order.

Whichever route, revoke the key at the git host as well. That
is the only step that does not depend on a guest doing what it
was told. `docs/developer/redteam-log.md` holds why this is
recorded rather than closed.

Two alternatives would change the picture rather than describe
it, and neither exists. A forwarded agent keeps the key off the
guest's disk entirely, at the cost of a loaded `ssh-agent` on
the VM host before every `up` -- which does not survive a host
reboot and cannot run unattended. A fetch proxy on the VM host,
or source baked into a base image, would move the credential
out of the guest altogether.

**bombyx cannot size the VM before the VM exists.** A project
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

## What is built and what is not

Built, as of 2026-09-07. `project-config-off-repo` is six of
its seven steps done; the seventh is under **Not built** below:

- `generate-vagrantfile` -- bombyx renders the Vagrantfile from
  a project's `[vm]` table and writes it, with a bootstrap
  script, onto the VM host. The generated file disables the
  default `/vagrant` share.
- `remote-clone-project-source` -- the bootstrap clones
  `[source]` inside the guest and runs a script from it, and the
  push is gone. `vagrant_dir`, the `tar`/`scp` pair and the
  remote unpack all went with it, so `bombyx up` is five `ssh`
  commands -- six with a `deploy_key` configured -- and the VM
  host holds no project file outside the guest's disk image.
- GitHub issue #50 -- `deploy_key` in `[source]` names a
  private key file on the VM host, and the guest clones a
  private repository with it. Cited by issue number rather than
  by a backlog ID because it never had one: the work came off
  the jutro conversion, not out of `docs/todo.md`. **What this
  costs** above holds the exposure.

- `project-config-off-repo` -- every setting moved into the
  operator's own `config.toml`, one `[projects.<name>]` table
  per project, and `project-selection-flag` added the
  `--project` argument that names one. The committed project
  file is gone, along with the overlay file and the `--host`
  flag, so the workstation opens no file in the project's
  directory and both statements above are reached.

  Note the direction of travel it reversed.
  `generate-vagrantfile` had added `[vm]` and `[source]` **to
  the committed file**, putting more of what bombyx depends on
  into the repository rather than less. Those tables are what
  moved.

Not built. The captured work sits in `docs/todo.md`:

- `destroy-confirmation-shape` -- what `destroy`'s positional
  becomes, now that `--project` already names the project.
- `minimal-vagrantfile` -- what the generator should emit, and
  nothing more.
- `provision-lifecycle-hooks` -- how the guest's setup is
  specified.
- `per-host-resource-profiles` -- carries the sizing question
  above.
- `host-network-isolation` -- carries the egress question
  above, and is what the guest needs in order to reach the git
  host at all.

The credential the guest needs for a private repository has a
mechanism, `deploy_key`, and the exposure it carries is
described under **What this costs** above rather than closed.
GitHub issue #50 is the work that added it.
