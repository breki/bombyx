# Fresh Reader Findings -- Deferred backlog

Comprehension review findings, from the reviewer that reads the
changed files cold. Newest first. A finding that was fixed leaves
no entry -- the comment it produced is the record.

---

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
