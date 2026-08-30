# trust-boundary-doc

**Status:** Done
**Captured:** 2026-08-30
**Started:** 2026-08-30
**Completed:** 2026-08-30

## Problem

bombyx exists so that project code never runs on the
workstation. That code is the attack surface: an agent is
compromised through what you ask it to work on. Nothing in the
repo says where project code is allowed to exist. You have to
read the code to find the boundary. Today the code puts project
files on two machines that are not the VM.

The operator has decided the boundary: **the guest is the only
place the project's source code exists.** The VM host must not
have to clone it either. This document records that decision,
the argument for it, and what it costs -- so that the next
reader does not have to re-derive the choice, and so that the
items built on it have something to cite.

## Context

### What happens today

`bombyx up` archives the project's `vagrant/` directory on the
workstation and unpacks it on the VM host. From the expected
argv in `crates/bombyx/src/plan.rs:351`:

```
tar -czf .bombyx-push-<n>.tar.gz -C <vagrant_dir> .
scp .bombyx-push-<n>.tar.gz vmhost:.bombyx-push-<n>.tar.gz
ssh vmhost "cd ~/vms/<project> && tar -xzf ~/.bombyx-push-<n>.tar.gz"
```

So the workstation holds a checkout, and the VM host holds an
extracted copy of part of it. The guest holds a third copy by
way of Vagrant's synced folder. Three machines, and the one the
design is meant to protect is the first of them.

### What the README currently claims

`README.md:28` ("Why") states the defence as "a VM with its own
kernel, no host filesystem, and no credentials". The last of
those three is the one this decision qualifies -- see
Decisions.

`README.md:41` ("Model") states two rules that shape the design.
Rule 1 reads: "The repo is the source of truth. Each project
keeps its `vagrant/` directory in its own repo. `bombyx up`
pushes it to the host before booting, so the host holds a cache
that cannot silently drift." The first sentence survives this
decision. The second and third describe the mechanism being
replaced, as does the `pushes vagrant/` arrow in the diagram
above them.

This matters for scope: the document being written here
describes a target, and the README describes what bombyx does
today. Writing the target into the README's Model section would
leave the README describing two architectures at once.

### The argument

Vagrant needs the Vagrantfile before the VM exists, so the
project's files cannot first appear inside the guest. Something
outside the guest has to hold them. There are exactly two ways
to satisfy that constraint:

1. The VM host joins the trusted computing base and holds a
   clone. Smaller change, and it keeps `vagrant/` in the project
   repo where it is today.
2. Nothing outside the guest holds project code at all. Then the
   Vagrantfile cannot come from the project, so bombyx has to
   generate it from its own templates, and the guest clones the
   project itself after boot.

Option 2 is the decision. It costs more work, and it matches the
premise in `README.md:28`. If the code inside the VM may be
hostile, then a machine holding a copy of that code is exposed
to it as well. The VM host would hold that copy, and the guest
can reach the host over SSH.

The chat that produced this backlog leaned toward option 1 on
the grounds that it was the smaller change. That is true and not
sufficient.

### What option 2 makes the sequence

Generating the Vagrantfile is no longer optional. The whole
sequence depends on it:

1. bombyx generates a minimal Vagrantfile from its own
   per-provider template. No project input needed.
2. Vagrant boots a clean VM from a base box.
3. A generic bootstrap provisioner, shipped by bombyx, runs in
   the guest.
4. The bootstrap clones the project inside the guest.
5. The guest runs the project's own lifecycle hooks.

The workstation and the VM host hold a repository URL, a commit
and host configuration. Metadata, not code.

### Effect on the captured backlog

- `remote-clone-project-source` was **wrong as captured**. Its
  body had the VM host cloning the repo and using its `vagrant`
  directory, which is option 1. Corrected in `docs/todo.md`;
  see Plan step 5.
- `generate-vagrantfile` moves ahead of `minimal-vagrantfile`
  and stops being optional. `minimal-vagrantfile` becomes a
  description of what the generator emits.
- `provision-lifecycle-hooks` is how step 5 is specified.
- `per-host-resource-profiles` inherits the open question below.
- `host-network-isolation` interacts: the guest cannot clone
  anything unless the egress ruleset permits reaching the git
  host. The pending nftables work has to allow it deliberately
  rather than by accident.

## Open questions

Question 1 (the git credential), 3 (where the document lives)
and 4 (how much of the README is corrected) are answered under
Decisions. One remains.

1. **Sizing the VM before it boots.** If a project declares its
   memory and CPU needs in its own repository, bombyx cannot
   read them before the VM exists. This is the same ordering
   problem the Vagrantfile has. Either the sizing lives
   in host-side or workstation-side bombyx configuration, or
   the boot is two-phase. Not resolved here, but the document
   should name it rather than leave it to be rediscovered.

## Plan

Docs-only. No code, no behaviour change.

1. Write `docs/trust-boundary.md`, following the house style in
   `docs/vm-host-setup.md`: full sentences, headings that state
   their content, the mechanism explained and not just the
   symptom, and the reason the decision went the way it did.
   Sections: what the boundary is; where project code lives
   today and why that is not it; the constraint Vagrant imposes;
   the two options and why the second was chosen; what it costs,
   including the credential and sizing questions; and what is
   not yet built.

2. Mark the target state plainly as not implemented. The
   document is a decision record, and stating the target in the
   present tense would be a false claim about what bombyx does
   -- the failure mode `CLAUDE.md` calls out under
   "Say what you have not verified".

3. In `README.md`, keep rule 1 and the diagram describing
   today's mechanism, and mark them inline as current and
   superseded, naming the new document. The README stays
   accurate about what bombyx does while saying where the
   design is going.

4. Qualify the "no credentials" claim in `README.md:28`.
   Without a qualifier, the README and the new document
   contradict each other.

5. `remote-clone-project-source` has been corrected in
   `docs/todo.md` by hand. Its summary and body put the clone
   on the VM host, which is the option this decision rejects.
   Its closing line also claimed that cloning inside the guest
   cannot work; only the Vagrantfile cannot come from the
   guest. `cargo xtask todo` has no edit command, so this was a
   hand edit against the rule in `.claude/commands/todo.md`.
   The gap is logged as
   `tf-2026-08-30-todo-tooling-cannot-revise-a-captured-entry`.

## Test strategy

There is no behaviour to test. The applicable gates:

- `cargo xtask doc` -- both rustdoc passes; only relevant if a
  doc comment gains a link, which is not currently planned.
- `cargo xtask validate` -- run at the end, as the umbrella
  gate. A docs-only diff should not move coverage or
  duplication.
- Markdown wrap at 80 columns, checked with `awk`.
- Every `path:line` reference in the new document read back
  against the file, since a line number is exactly the kind of
  claim that goes out of date, and the document is worth having
  only if it can be trusted.

Definition of Done item 3 does not apply: nothing changes the
commands bombyx emits, so there is no real-host run to do.

## Decisions

- **2026-08-30 -- The guest is the only place the project's
  source code exists.** The VM host must not clone it either.
  Operator decision, taken after both options above were
  described. Consequence accepted: bombyx must generate the
  Vagrantfile, because a project-supplied one cannot be read
  before the VM exists.

- **2026-08-30 -- The credential in the guest is an accepted,
  scoped exposure.** The guest holds a read-only, single-
  repository, short-lived credential in order to clone. Code
  inside the guest can read it. Scoping limits what stealing it
  is worth; it does not prevent the theft, and the document
  says so rather than implying the problem is solved. No
  alternative mechanism (forwarded agent, host-side fetch
  proxy, source baked into the image) is designed, so recording
  an intent would be a claim about something that does not
  exist.

- **2026-08-30 -- The decision gets its own document,**
  `docs/trust-boundary.md`, with a pointer from the README.
  Keeps the README a description of current behaviour and gives
  the dependent backlog items one place to cite.

- **2026-08-30 -- The superseded README rules are marked, not
  rewritten.** Rule 1 and the diagram in `README.md:41` keep
  describing the push mechanism, which is what the code does,
  with an inline note that it is being replaced and where the
  argument lives. Rewriting them now would describe an
  architecture bombyx does not have.

## Progress log

- **2026-08-30** -- `docs/trust-boundary.md` written, README
  amended in two places, `remote-clone-project-source`
  corrected in `docs/todo.md`. Docs-only throughout; no code
  changed.

## Outcome

`docs/trust-boundary.md` is the deliverable. It states the
boundary, describes today's behaviour with the argv read from
`crates/bombyx/src/plan.rs`, explains the Vagrant ordering
constraint, gives both ways to satisfy it and the argument for
the one chosen, and ends with three costs and a list of what
is not built. A blockquote at the top says the document is a
decision rather than a description, so a reader cannot mistake
the target for current behaviour.

`README.md` changed twice. "Why" now says "none of your
credentials" instead of "no credentials", with a short note
that a VM fetching its own private repository needs a
credential of its own. Model rule 1 keeps describing the push
mechanism, with an italic note that its second and third
sentences are being replaced and a link to the new document.
The diagram is untouched, since it illustrates the same
mechanism the rule describes.

`remote-clone-project-source` in `docs/todo.md` had its summary
and body rewritten. Its previous body put the clone on the VM
host, which is the rejected option, and claimed that cloning
inside the guest cannot work. That claim was wrong: only the
Vagrantfile cannot come from the guest.

Verification. Every file and slug named in the new document was
checked to exist -- three files and six todo slugs. Prose wrap
matches `docs/vm-host-setup.md` at 62 columns rather than the
80-column maximum, since `CLAUDE.md` names that file as the
style reference. Definition of Done item 3 does not apply: no
command bombyx emits was changed, so there is no real-host run
to report.

Follow-ups, all already captured:

- `generate-vagrantfile` is now the item everything else waits
  on, and it is not yet reflected in the ordering of
  `docs/todo.md`, which is append-order rather than dependency
  order.
- The sizing question stays open and belongs to
  `per-host-resource-profiles`.
- The egress question belongs to `host-network-isolation`,
  which also needs a sudo password on the VM host and so
  cannot be finished from a bombyx session.
