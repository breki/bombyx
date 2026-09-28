# Fresh Reader Findings -- Deferred backlog

Comprehension review findings, from the reviewer that reads the
changed files cold. Newest first. A finding that was fixed leaves
no entry -- the comment it produced is the record.

---

### fr-2026-09-28-destroy-help-leans-on-undefined-terms

**Category:** Comprehension

The `destroy` subcommand's clap help in
`crates/bombyx/src/bin/bombyx/main.rs` says it "discards the warm
caches the persistent lifecycle exists to keep". Both terms are
defined only in `docs/usage.md`, which a `bombyx destroy --help`
reader never sees. Plain words would serve: it discards the VM's
disk and everything installed on it, which is what makes a later
`up` fast. Logged rather than fixed in the #116 review, because
editing clap help changes the program's output and the
comprehension stage does not touch it.

### fr-2026-09-28-flatten-comment-narrates-history

**Category:** Comprehension

The comment above `Cmd::Vm` in `crates/bombyx/src/bin/bombyx/main.rs`
says the `--help` listing "does change: `self-update` now heads it
instead of sitting between `destroy` and `scratch`". "Now" compares
against a listing a newcomer never saw. State the current fact:
`self-update` and `list` come first because a flattened enum adds
its subcommands where it is flattened. Deferred from the #116
review, which did not touch those lines.

### fr-2026-09-28-gem-versions-point-at-a-check-that-omits-them

**Category:** Comprehension

`docs/vm-host-setup.md`, in "A fog warning that bombyx filters",
says the gem pair was "seen with `vagrant-libvirt 0.12.2` and
`fog-libvirt 0.15.0` at the August 2026 check above". The check
near the top of the page names Ubuntu 24.04.4 and Vagrant 2.4.9
and no plugin or gem versions, so a reader who goes back to
confirm finds nothing. Record the versions next to that check,
or state them here without pointing "above". Deferred from the
#55 review: the sentence predates that change.

### fr-2026-09-28-whether-vagrant-reprovisions-has-five-homes

**Category:** Comprehension

Five places state whether `vagrant up` provisions a machine that
already exists, in five versions: `Action::Provision` and
`Action::Up` in `plan.rs` say it never does, the clap help for
`Up` in `main.rs` says "is not provisioned again", `up_run` says
"usually", and `listing::refreshes_secrets_after_up` names the
exception -- vagrant provisions an existing machine whose
provision marker is missing. The last is the measured behaviour
(RT-3 on PR #126). State the rule once there, with the exception,
and point the other four at it or soften them to "normally".
Escalated rather than applied: it is a consolidation of five
copies, and one of them is clap help, which the prose stage may
not edit. Found as FR-2 on PR #126.

### fr-2026-09-25-the-shell-ignores-an-env-home

**Category:** Behaviour

When a project's `[env]` table sets `HOME`, `bootstrap.sh` clones
under that value, but `bombyx shell` resolves `$HOME` as the
agent's passwd home after `sudo -H`, so it opens outside the
clone after `cd` prints an error. The comments in
`remote::shell_into_vm` and `bootstrap.sh`, and the tutorial, now
say so. bombyx reads the `[env]` table itself, so it could pass
that `HOME` to the shell entry instead.

Deferred on 2026-09-25: a behaviour change found in the
prose-only stage of PR #124's review, which fixes only what a
person reads. Found as FR-4.

---

### fr-2026-09-24-shell-help-names-no-path

**Category:** help text

`bombyx --help` describes `shell` as "Open a shell inside the project
VM, in the project clone" (`VmCmd::Shell` in
`crates/bombyx/src/bin/bombyx/main.rs`; `Action::Shell` in `plan.rs`
has the same words). An operator reading only `--help` has not been
told what "the project clone" is or where it lives. Name the path, for
example "starting in the guest's `~/<project>`, where bombyx cloned the
repository". Raised on PR #120. `/review`'s stage 3 does not edit clap
help, because `bombyx --help` prints it, so it was deferred.

### fr-2026-09-24-clone-fallback-narrates-history

**Category:** history in prose

The comment above `readonly CLONE_DIR` in
`crates/bombyx/templates/bootstrap.sh` explains the `project`
fallback through "an older bombyx always cloned into a fixed
`$HOME/project`". The same history is told at `bootstrap.sh`'s header
("a directory an older bombyx wrote") and in the `PROJECT_ENV` doc in
`vagrantfile.rs` ("still clones where it always did"). A reader cannot
tell whether the fallback is a live requirement or dead compatibility
code. State the current reason: `BOMBYX_PROJECT` is unset when someone
runs `vagrant provision` by hand against a Vagrantfile that does not
set it, and the fallback keeps `set -u` from aborting. If that route is
unsupported, say so instead. Raised on PR #120. The text predates that
PR, and fixing it means merging three copies, so it was deferred.

### fr-2026-09-23-firewall-doc-narrates-history

**Category:** history in prose

`docs/vm-host-firewall.md` tells two incidents where it should state
rules: "That has happened here: a host ran a predecessor ruleset for
weeks whose DNS accept was not pinned", and "Earlier versions of this
section tried to give you one sentence...". `docs/todo.md` under
`host-network-isolation` says "since the probe was corrected". A
reader cannot tell whether "predecessor ruleset" means an older script
they might still have loaded. Keep the rule and drop the incident: for
example, `status` does not compare against `show`, so a table from an
older script passes; re-run `apply` after changing the script. Raised
on PR #114; outside that PR's change, so deferred.
