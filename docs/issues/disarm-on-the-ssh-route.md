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
- A `zsh` login shell reads `~/.zshenv` for `zsh -c`.
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

- `crates/bombyx/src/remote.rs:347` holds
  `DISARM_VAGRANT_REDIRECTS`, the `unset` prefix and its long
  doc comment.
- `crates/bombyx/src/remote.rs:364` holds `transport`, the one
  wrapper every VM command goes through. Its `Local` arm adds
  the prefix; its two `Ssh` arms do not.
- `crates/bombyx/src/remote.rs:690`,
  `the_ssh_route_disarms_nothing`, records the decision this
  change reverses.
- `the_local_route_runs_the_same_script_through_sh` strips the
  prefix from the local script and compares the rest to the
  `ssh` script character for character. With the prefix on both
  routes the strip has to move.
- `crates/bombyx/src/remote/probe.rs:62` builds the doctor
  probes. Its `Local` arm delegates to `transport`; its `Ssh`
  arm builds its own command with five connection options.
- `docs/architecture.md:75-87` describes the disarm as a local
  branch only.
- `docs/tutorial.md:667` and `docs/usage.md:304` print dry-run
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
  noise to the report's own commands and protect nothing. If a
  probe later runs a directory-bound vagrant command, it moves
  under this rule with it.

- **2026-09-06 -- the provider goes in front of every vagrant
  call, not only the boot.** Clearing
  `VAGRANT_DEFAULT_PROVIDER` on the `ssh` route undoes the
  workaround `docs/vm-host-wsl2.md` tells operators to apply:
  `VAGRANT_DEFAULT_PROVIDER=libvirt` in `/etc/environment`, so
  vagrant never probes the Hyper-V provider, which shells out
  to a PowerShell a hardened WSL distribution does not have.
  Without that value a `bombyx status` or `doctor` run before
  the first `up` would refuse on such a host. Two other routes
  were offered -- ship as is and document the loss, or leave
  the two provider variables out of the `unset` -- and the
  operator chose to write the configured provider back. The
  operator's exported value never wins, and vagrant is named a
  provider on every verb. This reverses the rule
  `remote::creates_a_machine` held; that function is gone.

## Progress log

- **2026-09-06** -- `every_route_disarms_the_vagrant_redirects`
  written and seen to fail on the `ssh` route, then the prefix
  moved out of the `Local` arm of `transport`.
- **2026-09-06** -- `every_project_vagrant_call_names_the_provider`
  written and seen to fail, then `creates_a_machine` removed
  and the provider written on every call.
- **2026-09-06** -- about two dozen tests read a script's exact
  head, so `remote::tests` and `plan::tests` each grew a helper
  that strips the prefix. `remote::tests::raw_script` is what
  the disarm test itself uses.
- **2026-09-06** -- all ten gates pass.

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

**Not verified.** Definition of Done item 3 has not been met.
`ssh frosti` fails host key verification from this session, so
nothing here ran against a real VM host, and no WSL2 host was
available to confirm that the written-back provider fixes the
case that decision was made for.
