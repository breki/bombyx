# disarm-on-the-ssh-route

**Status:** Done
**Issue:** https://github.com/breki/bombyx/issues/48
**Captured:** unknown (filed during review round 3 on issue #45)
**Started:** 2026-09-06
**Completed:** 2026-09-06

## Problem

bombyx clears five vagrant environment variables before it runs
a script, but only on the local route. The argument for leaving
the `ssh` route alone was that sshd builds the far side's
environment and bombyx's own does not cross the connection.
That is true and it does not close the hole: the threat is an
exported variable, and the VM host has its own sources for one.

- `pam_env` applies `/etc/environment` to a non-interactive
  `ssh host "cmd"`.
- `zsh` sources `~/.zshenv` on every invocation, `zsh -c`
  included, so an export there reaches the command sshd runs.
- A `bash` export placed above the usual non-interactive return
  guard in `~/.bashrc` survives.

So `VAGRANT_CWD` or `VAGRANT_VAGRANTFILE` set on the VM host
makes `bombyx destroy` test one project's directory and destroy
the machine defined in another, and
`VAGRANT_DEFAULT_PROVIDER=hyperv` there gets a refused
`vagrant destroy`, which strands the directory because
`execute` stops at the first failing step.

Found by red-team, read from the code and from sshd's
documented PAM and shell startup. Not measured -- the VM host
was not reachable from that session.

## Context

**This section records the tree as it stood when the issue was
filed.** The names below are how to find things; the line
numbers they used to carry are gone, because every one of them
moved while the work was done.

- `remote::DISARM_VAGRANT_REDIRECTS` holds the `unset` prefix
  and its long doc comment.
- `remote::transport` is the one wrapper every VM command goes
  through. Its `Local` arm added the prefix; its two `Ssh` arms
  did not.
- `the_ssh_route_disarms_nothing` recorded the decision this
  change reverses. It is gone, replaced by
  `every_route_disarms_the_vagrant_redirects`.
- `the_local_route_runs_the_same_script_through_sh` strips the
  prefix from the local script and compares the rest to the
  `ssh` script character for character. With the prefix on both
  routes the strip has to move.
- `remote::probe::probe` builds the doctor probes. Its `Local`
  arm delegates to `transport`; its `Ssh` arm builds its own
  command with five connection options.
- `docs/architecture.md` describes the disarm as a local
  branch only.
- `docs/tutorial.md` and `docs/usage.md` print dry-run
  transcripts that the new prefix changes.
- Three unit tests assert against an `ssh` script's exact head:
  `vagrant_runs_in_the_project_dir` (`starts_with("cd ...")`),
  `ensure_dir_keeps_the_tilde_expandable` and
  `ensure_dir_quotes_an_absolute_dir` (both `assert_eq!` on
  the whole script).

## Open questions

- Does the doctor's `ssh` probe arm need the prefix too?
  Answered under **Decisions**.

## Plan

1. Move the prefix out of the `Local` arm of `transport` and
   apply it to the script once, before the match, so all three
   arms carry it.
2. Rewrite the `DISARM_VAGRANT_REDIRECTS` doc comment: cut the
   paragraph arguing the `ssh` route is exempt, and state the
   two environment sources plainly instead.
3. Replace `the_ssh_route_disarms_nothing` with a test asserting
   the `ssh` route disarms all five, and widen the local test's
   name and body to cover both routes. One test over both
   routes, not two.
4. Fix the three tests asserting an `ssh` script's exact head,
   by stripping the prefix first.
5. Update `docs/architecture.md`, `docs/tutorial.md` and
   `docs/usage.md`.
6. Add a `### Changed` bullet to `CHANGELOG.md`.

## Test strategy

Rust unit tests in `remote.rs`, which is where the wrapper and
every existing route test live. The integration suite needs no
new test: its assertions on `ssh` lines are `contains` and
`ends_with`, so the prefix does not disturb them, and running
the real binary proves nothing here that the unit tests do not.

The change alters the string bombyx sends to the VM host, so
Definition of Done item 3 applies: it needs a real run against
a real VM host, not only a dry run. Whether that happened is
recorded under **Outcome**.

## Decisions

- **2026-09-06 -- the doctor's `ssh` probe arm stays as it is.**
  The probes run `true`, `command -v vagrant`, a writability
  check and `vagrant plugin list`. None of the five variables
  decides what any of those answers, so the prefix would add
  noise to the report's own commands and protect nothing.
  **Reversed the same day**, see the next bullet.

- **2026-09-06 -- the probes carry the prefix too.** `artisan`
  found the decision above had left the crate inconsistent
  rather than deliberately narrow. `probe`'s local arm
  delegates to `transport` and so gained the prefix, while its
  `ssh` arm builds its own command and did not, so one
  `bombyx doctor` sent a different script on each route -- and
  `DISARM_VAGRANT_REDIRECTS`'s own new doc said "on either
  route", which was then false. The argument that settles it:
  `doctor` reports the environment bombyx's own commands run
  in, so a probe reading an environment bombyx clears answers
  about a state no other command sees. Both arms of `probe`
  now build from the prefixed script.

- **2026-09-06 -- the provider goes in front of every vagrant
  call, not only the boot.** Clearing
  `VAGRANT_DEFAULT_PROVIDER` on the `ssh` route undoes the
  workaround `docs/vm-host-wsl2.md` tells operators to apply:
  `VAGRANT_DEFAULT_PROVIDER=libvirt` in `/etc/environment`, so
  vagrant never probes the Hyper-V provider, which shells out
  to a PowerShell a hardened WSL distribution does not have.
  Without that value a `bombyx status` run before the first
  `up` would refuse on such a host. Not `doctor`: its one
  vagrant call is `vagrant plugin list`, measured here to
  ignore the variable completely -- it printed the same plugin
  list under `hyperv` and under a provider name that does not
  exist. Two other routes
  were offered -- ship as is and document the loss, or leave
  the two provider variables out of the `unset` -- and the
  operator chose to write the configured provider back. The
  operator's exported value never wins, and vagrant is named a
  provider on every verb. This reverses the rule
  `remote::creates_a_machine` held; that function is gone.

- **2026-09-06 -- the teardown verb names no provider.**
  `red-team` found that writing it on every call re-introduced
  a defect commit `777fa0e` had removed the day before, after
  measuring it on this machine. Re-measured here in a scratch
  directory holding a Vagrantfile and no machine:
  `VAGRANT_DEFAULT_PROVIDER=hyperv vagrant destroy -f` is
  refused and exits 1, while the same command with no such
  variable reports "Domain is not created" and exits 0.
  `execute` stops at the first failing step, so the refused
  version leaves the `rm -rf` behind it unrun. Two facts make
  the exemption safe: a refusal can only happen when no machine
  exists, and a machine that exists carries its own recorded
  provider. `remote::is_teardown` holds the rule, and
  `the_teardown_verb_names_no_provider` states it across the
  actions.

  The option of keeping it everywhere and widening the teardown
  guard to skip vagrant when no machine exists was offered and
  not taken: it is new logic beyond what this issue asked for.

- **2026-09-07 -- the WSL2 teardown gap is carried, not
  closed.** Round 2 of `red-team` showed the exemption above
  moves the stranding rather than removing it: on a WSL2 host
  the teardown now names no provider and the `unset` cleared
  the one `/etc/environment` supplied, so `bombyx destroy` is
  expected to be refused and leave the directory. Two other
  routes were offered -- letting the host's own
  `VAGRANT_DEFAULT_PROVIDER` through on the teardown alone, or
  naming the provider everywhere and calling vagrant only when
  a machine is recorded. The operator chose to keep the code
  and write the gap down, on the grounds that the libvirt case
  is measured while the WSL2 case is inferred, and that this
  rule has already moved four times in two days on facts
  measured after the previous move landed.
  `docs/vm-host-wsl2.md` names the command that settles it.

- **2026-09-06 -- `doctor` is probably not the WSL2 risk, and
  the claim is marked as unproven.** The decision above was
  argued partly from a `bombyx doctor` run on a WSL2 host.
  `vagrant plugin list` prints the same list under
  `VAGRANT_DEFAULT_PROVIDER=hyperv`, under a name no provider
  has, and with the variable absent. That says vagrant does not
  *use* the value; it does not prove vagrant never consults
  providers to pick a default, because this host has a working
  libvirt and a probe that ran would have succeeded. The four
  prose sites now say so and name the experiment that would
  settle it. Round 2 of `red-team` raised this; the first
  version of the claim was broader than the measurement.

## Progress log

- **2026-09-06** -- `every_route_disarms_the_vagrant_redirects`
  written and seen to fail on the `ssh` route, then the prefix
  moved out of the `Local` arm of `transport`.
- **2026-09-06** -- a test that every project vagrant call
  names the provider, written and seen to fail, then
  `creates_a_machine` removed and the provider written on every
  call. Review later narrowed both the test and the rule to
  exempt the teardown; the test is now
  `every_other_project_vagrant_call_names_the_provider`.
- **2026-09-06** -- about two dozen tests read a script's exact
  head, so `remote::tests` and `plan::tests` each grew a helper
  that strips the prefix. `remote::tests::raw_script` is what
  the disarm test itself uses.
- **2026-09-06** -- all ten gates pass.
- **2026-09-06** -- review stage 1 (`artisan`): six findings,
  six fixed, including the probe inconsistency above.
- **2026-09-06** -- review stage 2 (`red-team`) round 1: seven
  findings. One behaviour defect, the teardown provider, fixed
  with its test first and then run end to end against a real
  vagrant: a `hyperv` project's `up` is refused and leaves the
  directory, and `bombyx destroy` then exits 0 and removes it.

## Outcome

`crates/bombyx/src/remote.rs` now prefixes
`DISARM_VAGRANT_REDIRECTS` to the script once, before the match
in `transport`, so all three arms carry it. `vagrant_command`
writes `VAGRANT_DEFAULT_PROVIDER` from the project's config in
front of every vagrant call, and `creates_a_machine` is gone.

Prose updated: `docs/architecture.md` (the disarm is no longer
a route difference, and the provider paragraph), `llms.txt`
(the provider no longer rides on `up` alone),
`docs/vm-host-wsl2.md` (bombyx discards the `/etc/environment`
value and writes its own back), and the dry-run transcripts in
`docs/tutorial.md` and `docs/usage.md`, regenerated from the
real binary rather than hand-edited.

**Verification.** All ten gates pass. The emitted scripts were
run through a real `sh` against a stub `vagrant`, with
`VAGRANT_CWD=/somewhere/else` and
`VAGRANT_DEFAULT_PROVIDER=hyperv` exported: the stub reported
the bombyx directory, an empty `VAGRANT_CWD` and
`VAGRANT_DEFAULT_PROVIDER=libvirt`. The heredoc write behind
the same prefix was run and the file landed with `$(...)`
unexpanded.

**Definition of Done item 3, on the local route.** This
workstation is `frosti`, which is the host the operator's own
registry names, so bombyx takes the local route against it and
a real vagrant 2.4.9 with `vagrant-libvirt` 0.12.2 is
installed. `bombyx --project vmtest doctor` reports six rows,
all `ok` or `skip`. `bombyx --project vmtest status` reports
the real `vmtest` domain as running. Run again with
`VAGRANT_CWD=/tmp` and `VAGRANT_DEFAULT_PROVIDER=hyperv`
exported it returns the same correct answer, while a bare
`vagrant status` in the same directory with the same
`VAGRANT_CWD` exits 1 with "A Vagrant environment or target
machine is required". So the guard was seen to work, and the
counterfactual was seen to fail.

**Not verified: the WSL2 host.** Two claims rest on inference
rather than measurement, both about a host bombyx has never
run against. `bombyx destroy` is expected to be refused there,
because the teardown names no provider. And `bombyx doctor` is
expected to be fine, because `vagrant plugin list` appears not
to consult providers. `docs/vm-host-wsl2.md` names the command
for each.

**Not verified: the `ssh` route.** It was not exercised against
a remote VM host: the only host in the registry is this machine,
and `ssh frosti` fails host key verification here. Both routes
emit one script, pinned by
`the_local_route_runs_the_same_script_through_sh`, so what
remains unproven is only that a remote sshd's shell accepts the
prefixed string -- it is the same POSIX shell. No WSL2 host was
available to confirm that the written-back provider fixes the
case that decision was made for.
