# doctor-preflight

**Status:** Done
**Captured:** 2026-08-09
**Started:** 2026-08-10
**Completed:** 2026-08-10

## Problem

`bombyx up` can fail late and confusingly. The push creates a
remote directory and ships a tarball before it ever runs
`vagrant`, so a host that is missing something reports it
half-way through, after changing state. The worst case is
`bash: vagrant: command not found`, which is bombyx-specific
and which **nothing else can report**: vagrant cannot tell you
it is invisible to a non-interactive shell, because it is not
running.

`bombyx doctor` should check bombyx's own preconditions up
front, report every one as a pass/fail line without stopping at
the first failure, and change nothing.

## Context

### What the first real run taught

The probe list in the captured item was a guess. Running the
diagnosis by hand on frosti (see
[first-real-run](../developer/DIARY.md)) changed it in two
ways worth recording:

- **`vagrant` on the non-interactive `PATH` is the probe that
  matters.** It was the actual failure, it is invisible to
  every other tool, and the symptom is misleading: `vagrant`
  works when you `ssh` in and type it, because that is a login
  shell with a fuller `PATH`.
- **A probe can pass while proving nothing.** My first libvirt
  check was `virsh list --all`, which for a non-root user
  silently connects to `qemu:///session` -- a per-user instance
  that is always reachable. It would have passed with no group
  membership at all. Any probe added here must be checked for
  the same flaw: does a green result actually mean what it
  claims?

That second point is the reason this item was deliberately
sequenced *after* the first real run.

### Where it lands

| File | Change |
|------|--------|
This is where the work was expected to land, written before it
did. Two rows moved during review and are corrected here rather
than left to mislead a reader who greps for them: the probe
builders ended up in their own submodule, and `tool.rs` was not
foreseen at all.

| File | Change |
|------|--------|
| `crates/bombyx/src/doctor.rs` | new: report model, classify, render |
| `crates/bombyx/src/remote/probe.rs` | new: the probe-command builders |
| `crates/bombyx/src/tool.rs` | new: `PATH` lookup, never the cwd |
| `crates/bombyx/src/plan.rs` | `Action::Doctor` |
| `crates/bombyx/src/bin/bombyx/main.rs` | `Cmd::Doctor`, the probe runner |
| `crates/bombyx/src/lib.rs` | `pub mod doctor`, `pub mod tool` |

### The executor does not fit

`execute` (`main.rs`) uses `.status()`, so it inherits stdio
and captures nothing, and it returns at the first non-zero
exit. Doctor needs the opposite of both: `.output()` per probe,
and every probe run regardless of earlier failures. So doctor
gets its own small runner rather than reusing `execute`, and
`main.rs` branches to it before the normal
`plan` -> `execute` path.

Keeping the runner thin matters because `src/bin/` is excluded
from the coverage gate. The branchy part -- classifying a probe
result, cascading skips, rendering the report, deciding the
exit code -- lives in `doctor.rs` and is unit-tested.

### Constraints

- **Read-only.** A diagnostic that changes state is not a
  diagnostic. This rules out probing `remote_root` with
  `mkdir -p`, even though `up` does exactly that.
- Config validity is already covered: a bad `remote_root` now
  fails at `Config::load`, so `doctor` cannot even start with
  one. That is the right layer and needs no probe.
- Scope stays as captured: bombyx's own preconditions, not host
  provisioning. No `/dev/kvm`, RAM, disk or libvirt daemon
  checks.
- No remediation text. Naming the fix needs a per-distro
  database that rots -- Ubuntu 24.04 dropped `vagrant` from its
  archive, others did not. Report the fact precisely and let
  the operator decide. `docs/vm-host-setup.md` holds the
  remedies.

## Open questions

Resolved -- see Decisions.

## Plan

The plan as written, kept for the record. Three of the names in
it did not survive: the builders moved to `remote/probe.rs` and
dropped their `probe_` prefix (`probe_reachable` ->
`probe::reachable`), `probe_login_shell` became
`probe::posix_shell` when it was changed to make the shell *run*
a POSIX construct, and `probe_root_writable` became
`probe::dir_writable` when it was pointed at the directory `up`
actually writes into. The Outcome section below is the accurate
description of what shipped.

1. `doctor.rs`: `Outcome` (`Pass`/`Fail`/`Skip`), `Finding`
   (scope, name, outcome), `Report`. Pure functions:
   `classify` (exit status + stdout -> `Outcome`),
   `Report::ok`, `Report::render`, and the skip cascade.
2. `remote.rs`: probe builders --
   `probe_reachable` (`ssh <host> true`),
   `probe_command` (`command -v <tool>`),
   `probe_login_shell`, `probe_root_writable`.
3. `plan.rs`: `Action::Doctor`, `pushes()` false.
4. `main.rs`: `Cmd::Doctor`; a `run_probes` loop using
   `.output()`; local probes (`tar`, `Vagrantfile`) done in
   Rust rather than shelled out.
5. **Skip cascade:** run `probe_reachable` first. If SSH fails,
   mark the remaining remote probes `Skip` rather than running
   them -- five probes each waiting on a dead host is slow and
   tells the operator nothing new.
6. Exit 0 when every probe passes, 1 otherwise. `--dry-run`
   prints the probe commands instead of running them, keeping
   the flag meaningful across all subcommands.

## Test strategy

Unit tests for the pure logic, integration tests for the CLI
surface, and a real run against frosti. Cheapest level that
proves each thing:

- **`doctor.rs`** (the bulk): `classify` maps a zero exit to
  `Pass` and carries the first stdout line as the detail; a
  non-zero exit to `Fail`; empty stdout to a `Pass` with no
  detail. `Report::ok` is false when any finding fails and
  **true when findings are only skipped or empty** -- the
  edge case worth pinning, since a skip must not silently
  count as a pass. `render` groups by scope and aligns.
- **Skip cascade**: an unreachable host yields one `Fail` for
  SSH and `Skip` for every other remote probe, with local
  probes still evaluated.
- **`remote.rs`**: each probe builder quotes correctly and
  keeps a leading `~` expandable, same as the existing
  builders.
- **Integration**: `bombyx doctor --dry-run` lists the probe
  commands; `bombyx doctor` against an unreachable host exits
  non-zero and names the SSH failure.
- **Real host**: required. The whole point is behaviour against
  a real remote shell, and `--dry-run` proves only the argv. I
  will run it against frosti in the healthy case, and then
  against a deliberately broken one (a `host` alias that does
  not resolve) to see the cascade.

## Decisions

- **2026-08-10 -- Probe the Vagrant provider plugin, and
  nothing else from the provisioning set.** This revisits the
  scope decision recorded when the item was captured, which
  kept all provisioning checks out on the grounds that vagrant
  reports its own problems. The first real run supplied
  counter-evidence: with the plugin missing, everything bombyx
  needs was present, so `up` created the remote directory and
  shipped a tarball *before* failing. The operator's question
  is "will `up` work", not "whose layer is at fault", and this
  is the one provisioning gap that costs state before it
  surfaces. Rejected as a warning-only probe, which would have
  added a third severity to the report and the exit code for
  little gain. Still excluded: `/dev/kvm`, VT-x, RAM, disk,
  libvirtd, storage pools -- those cost nothing before failing
  and are `docs/vm-host-setup.md`'s job.

  The line this draws: a probe belongs here when a green
  result is something bombyx depends on *and* a red result
  would otherwise be discovered after bombyx has changed
  state. That is a narrower rule than "bombyx's own
  preconditions" and it is the one actually being applied.

## Progress log

- **2026-08-10** -- `doctor.rs` with the pure report model;
  probe builders in `remote.rs`; `host_probes` and
  `Action::Doctor` in `plan.rs`; `doctor_run` in `main.rs`.
- **2026-08-10** -- Verified against frosti in the healthy case
  and against an unresolvable host, which exercised the skip
  cascade.

## Outcome

`cargo xtask validate` green: 99.3% coverage, 1.7% duplication,
clippy clean. Eleven checks: seven on the host and four local.

### The design principle was violated three times, then fixed

The module doc states the rule the first host setup taught:
**distrust a green result**. Writing the feature, I broke it
three times, and the reviewers caught all three. Each probe now
carries a *verdict* rather than a value.

- **`login shell` passed on the state it existed to catch.** It
  ran `echo "$SHELL"` and any zero exit was a pass, so a `fish`
  or `csh` login shell reported `ok` -- and an unset `SHELL`
  reported `ok` with no detail at all, a pass that verified
  nothing. It now makes the shell *run* a POSIX construct and
  print a token, which is checked in `posix_shell_verdict`. That
  also tests the shell that actually interprets bombyx's
  scripts, rather than an environment variable a wrapper can
  set.
- **`project dir` green-lit a state guaranteed to fail.**
  Verified before the fix: with a *file* named `~/vms`, doctor
  printed `all checks passed` and `up` then died on its first
  command with `mkdir: cannot create directory: Not a
  directory`. It also reported `not found` for four distinct
  states including "exists but unwritable", sending the operator
  to the wrong remedy. Each failure now names itself, and the
  probe checks the directory `up` actually writes into rather
  than `remote_root`.
- **`libvirt provider` trusted the wrong exit code.** The
  pipeline kept only `grep`'s status, discarding vagrant's own
  failure, and the pattern was an unanchored substring so
  `vagrant-libvirt-qemu` would have passed. Vagrant's status is
  now propagated and the match anchored.

The same review also found the report itself was untrustworthy:
probe details are text from the VM host printed straight to the
terminal, so a host emitting cursor escapes could repaint a
`FAIL` line as `ok`. Control characters are now replaced before
rendering.

### Other fixes from the review

- `ConnectTimeout=10` and `LogLevel=ERROR` on every probe.
  `BatchMode` bounds interaction, not duration, so a blackholed
  host previously stalled for the OS timeout with no output;
  measured at 0.25s now. `LogLevel` stops an sshd `Banner` being
  reported as the failure reason, and `fail_reason` takes the
  *last* stderr line for the same reason.
- The skip cascade moved into `doctor::run_probes`, a pure
  function over a runner closure, so it is tested. It was in
  `main.rs`, which the coverage gate excludes, while a doc
  comment claimed keeping the list in the library made it
  testable. It did not.
- A gating flag on `HostProbe` replaces `if *name == "ssh"`, so
  renaming a report column cannot silently disable the cascade.
- A spawn failure becomes a `Finding` instead of discarding the
  whole report -- previously `ssh` missing from `PATH`, the
  likeliest local fault, produced no diagnosis at all.
- `plan`'s `Action::Doctor` arm was unreachable, since
  `doctor_run` had its own dry-run printer; the "must be
  read-only" test was testing dead code. `--dry-run` now goes
  through `plan` for every action and the probe list has one
  owner.
- New `tool` module resolving `tar`/`ssh`/`scp` against `PATH`
  explicitly, never the working directory. On Windows the OS
  search includes the cwd, and `doctor` is the command the docs
  say to run *first in a fresh clone* -- so a repo shipping
  `tar.exe` was workstation code execution. Applied to `execute`
  too, closing the same hole in the push path.

That last change altered which `tar` bombyx runs on this
machine, from Windows bsdtar to Git's GNU tar, so a real `up`
was re-verified end to end afterwards.

### Second review round

Twenty-five findings, and the pattern across them is worth
recording: **most were instances of two classes, not twenty-five
independent bugs.**

- **Hand-rolling something with more edge cases than it looks
  like it has.** Four findings (executability bits, `PATHEXT`
  ordering, quoted `PATH` entries, backtracking past an
  unusable candidate) were all the same mistake in the
  hand-written `PATH` search. Fixed by deleting the search and
  taking the `which` crate; `tool.rs` now keeps only the one
  decision that is bombyx's own -- never search the working
  directory -- as a subtraction from `PATH` before the lookup.
  A fifth finding of the same shape was mine to make twice: the
  first `tail`-based provider message was another hand-rolled
  shell construct, and it broke on the exact host state it
  reported on (see below).
- **A guard that fails on one spelling of its input.**
  `probe_dir_writable`'s walk tested `-e`, which is false for a
  dangling symlink, so it stepped *past* one to a writable
  parent and passed. Verified on frosti: the probe now fails
  with `exists but is not a directory` where `mkdir -p` fails
  with `File exists`; before the fix the same host state
  reported `ok`. This is the third time this feature has shipped
  a probe that passed on the state it existed to catch.

Other fixes:

- `spawn_probe` had a bare-name fallback: when `tool::resolve`
  found nothing it spawned the unresolved name, which is the OS
  search `tool` exists to avoid -- in the command the docs say to
  run first in a fresh clone. Removed.
- `execute` resolved each program inside its loop, so `up` could
  create the remote directory and *then* discover `tar` was
  missing. Resolution is now up front. Verified against frosti
  with a `PATH` holding only `ssh` and `scp`: `bombyx up` exits
  with `tar not found on PATH` and `~/vms/<project>` is not
  created.
- The provider probe reported "vagrant is not installed" and
  "the plugin is not installed" identically, because both
  arrived as a silent non-zero exit and fell back to
  `not found`. Each now names itself. The first attempt composed
  the label and vagrant's own message with a `tail` subshell and
  **failed against a real host** -- a `PATH` broken enough to
  hide `vagrant` hides `tail` too, so the reason came back empty.
  Printing the label *before* vagrant's output instead makes
  vagrant's last line the reason and needs no extra tool.
- `ConnectTimeout` bounds the direct `connect()` only: it is not
  inherited by a `ProxyJump` and does not cover the banner
  exchange, so a host that accepts the connection and then goes
  quiet still hung the diagnostic. Added
  `ServerAliveInterval=5` / `ServerAliveCountMax=3` and
  corrected the doc comment, which had claimed the coverage it
  did not have.
- `sanitize` covered control characters but not bidirectional
  overrides or the invisible formatting characters, so a host
  could still reverse a run of text on the operator's terminal.
  Widened -- and moved to `Report::render`, the one boundary
  every detail crosses. It was previously applied by each
  producer, and the binary's producers did not.
- The archive exclusions had only ever been asserted as
  `--exclude` flags in the argv, which proves bombyx asked, not
  that `tar` complied. There is now an integration test that
  packs a real tree with the resolved `tar` and reads the
  archive back. Checked by hand that the assertion discriminates
  (without the flags the listing contains `./.git/HEAD` and
  `./.vagrant/id`).
- The read-only guarantee was asserted by two hand-maintained
  blocklists, in the unit test and the CLI test, which disagreed
  -- so each proved something weaker than it looked. One
  `doctor::MUTATING_TOKENS` now serves both. It stays a
  blocklist deliberately: the allowlist version needs a shell
  parser, and a subtly wrong parser inspires more confidence
  than the blocklist while catching less.
- `Outcome::Skip` hardcoded `"no ssh"`; it now names the gating
  probe, read from the probe.
- `clip` returned the full detail when the budget was under four
  characters -- the one case where returning it breaks the
  aligned line the caller asked for a budget to build. It now
  degrades.
- `local_tool`'s four-way decision (absent / present /
  present-but-unusable / present-but-quiet) lived in `main.rs`,
  outside the coverage gate. Moved to
  `doctor::local_tool_finding` with a test per case.
- `ProbeResult` replaces `classify`'s two adjacent `&str`
  parameters, which were read differently and would have
  transposed silently.
- `HostProbe::plain(..).gating()` / `.with_verdict(..)` replaces
  four struct literals, so the two probes that are not plain say
  so at the point a reader sees the list.
- The probe builders moved to `remote/probe.rs`. They are the
  only commands in that module forbidden to change the host, and
  the rule is now reviewable in one file.
- `Report::push`/`extend` renamed to `add`/`add_all`: they
  shadowed the `Vec` and `Extend` spellings on a type that is
  not a collection.
- The dry-run comment claimed `plan` renders the whole doctor
  run; it renders the host probes only. `doctor::probe_commands`
  is now the single renderer both paths use, the local checks'
  absence is explained, and `Doctor` sits in the same position
  in `Cmd` and `Action`.

### Third review round

Twenty-two findings, two of them cross-confirmed. The severe one
was in the module written to close a security hole.

- **`tool::resolve` could still return a relative path, and
  execute it.** With no absolute entry on `PATH`,
  `absolute_entries` yields an empty string --- and
  `std::env::split_paths("")` yields one **empty** entry rather
  than none, so `which`'s own empty-list check never fires. On
  Unix `which` deliberately does not drop empty entries (the real
  `which` searches the working directory for them), so the
  candidate became the relative path `tar`, resolved against the
  process working directory and then spawned. Confirmed by
  reading the dependency's `finder.rs`, not inferred. A second
  hole: the separator guard listed `/` and `\` but not `:`, so
  the Windows drive-relative `C:tar` --- named as a
  working-directory spelling in the module's own doc --- passed
  it.

  The guarantee is now enforced three times: a non-bare name is
  refused (`:` included), an unsearchable `PATH` stops the lookup,
  and a non-absolute answer is discarded. `which_in_global`
  replaces `which_in`, which removes the `current_dir()` argument
  entirely. This is the second round in a row where this guard
  covered a *sample* of its input family rather than the family,
  which is what `CLAUDE.md`'s "enumerate the family first" rule
  exists to prevent.
- **`project dir` passed on a directory `mkdir -p` cannot write
  to.** It tested `-w` but not `-x`, and creating an entry needs
  write *and* search permission. Verified on frosti with a
  `drw-------` directory: `-w` passes, and `mkdir -p` fails with
  `Permission denied`. The probe now names the missing search bit
  as its own failure. Also recorded, rather than fixed: `test -w`
  calls `access(2)`, which succeeds for root on almost any path,
  so against a root login this probe is advisory.
- **`doctor` was not as read-only as it claimed.** The README said
  in bold that it changes nothing on the host, and the guard
  behind that claim inspects only the *text* of bombyx's own
  scripts. `vagrant plugin list` creates `~/.vagrant.d` on a host
  where vagrant has never run, and does a version-checkpoint HTTP
  call whose response it caches. Fixed both ways: the claim is
  narrowed everywhere it appears (bombyx creates, deletes and
  modifies nothing; vagrant may initialise its own home
  directory), and `VAGRANT_CHECKPOINT_DISABLE=1` removes the
  network call --- which was also the one place a probe could hang
  past the ssh keepalives, since those bound a dead network and
  not a slow remote command.
- **The read-only guard matched substrings.** `"rm "` and `"> "`
  with trailing spaces are the ordinary spellings and not the only
  ones: `>file`, `1>file`, `>|file`, a tab-separated `rm`,
  `vagrant init` and `vagrant plugin uninstall` all passed. It now
  identifies the command word of each shell segment (skipping
  keywords and `VAR=` assignments) and treats `>` structurally ---
  any `>` that is not the `>&` descriptor duplication. What it
  still cannot see (`xargs rm`, `sh -c '...'`) is written down at
  the definition.
- **`sanitize` was a blocklist that could not be completed.** It
  missed `U+2028`/`U+2029` (not `is_control`), the variation
  selectors, and the whole tag block `U+E0000`-`U+E007F`, which
  renders as nothing at all and is the standard way to hide text
  inside text --- enough to make `vagrant-libvirt-fork` read as
  `vagrant-libvirt`. Inverted to an allowlist: printable ASCII and
  the space, which is finishable and matches what the report is
  everywhere else.
- **A long host name deleted the failure reason.** The detail
  budget is 80 minus a prefix that grows with the host name, which
  `Config` does not bound. At 49 characters the detail was three
  dots; at 52 it was empty, so `FAIL` printed with no diagnosis
  while the report still looked complete and aligned. There is now
  a floor, and a line may run over 80 columns instead.
- **Cross-confirmed: `plan`'s `Action::Doctor` arm was dead.**
  Both reviewers found it. The previous round's "build the probe
  list once" change had made `main` return before `plan` was
  reached, so the arm was unreachable from the binary and its test
  compared `probe_commands` with its own definition. The dry run
  goes back through `plan` for every action, the two doctor paths
  are now mutually exclusive so nothing is built twice, and the
  CLI-level test asserts the binary's printed output against
  `probe_commands` --- the half that constrains the binary.
- **Cross-confirmed: a control character was committed.** The
  `bombyx doctor` sample in `docs/vm-host-setup.md` read
  `C:Program FilesGitSr<0x08>in` --- the backslashes were eaten as
  escapes when I wrote the file and a literal backspace survived
  into git. In the document that is the project's style exemplar,
  in the sample an operator compares their own run against, for
  the feature whose job is removing control characters from
  output. Fixed, and the tree swept for others (none).
- **A CLI test asserted nothing.** `contains("Vagrantfile")`
  matches the name column, which prints for a pass, a failure and
  a skip alike; `contains("FAIL")` was satisfied by the unrelated
  ssh failure. Inverting `vagrantfile_finding` would have left it
  green. It now locates the row by scope *and* name and asserts
  its tag, in both the missing and the present case.
- Quality fixes: `VersionAnswer` replaces a nested
  `Option<Result<_, _>>` whose `None` meant two different things;
  `LOCAL_LABEL` is a constant because it was both printed and
  measured, and measured in bytes while the host beside it was
  measured in characters; four internal-only items became private;
  `Config::for_tests` replaces four copies of the same test
  fixture; the probe-builder test list is derived from
  `host_probes` rather than being the hand-maintained list its own
  comment claimed to prevent; and four incidents narrated in four
  places each now have one home and pointers.

Deferred at the time, and done in the very next commit: splitting
`doctor.rs` into a directory module. It was the right change and a
1300-line reorganisation, which would have buried the fixes above
in a diff nobody could review. Separating them cost one commit and
made both readable. The resulting line counts, and why
`doctor/readonly.rs` was left whole at 193 lines, are recorded in
that file's own module doc rather than in a backlog.

### Fourth review round, scoped to the rewritten guards

Two rounds running had found a bug inside my own fix, so the
fourth pass reviewed only the three guards this round rewrote.
`tool::resolve` and the report's text handling came back clean ---
including a trace through `which`'s internals confirming no code
path reaches the working directory. The read-only guard came back
with six findings, and they are the ones worth recording.

- **The rewrite regressed against the version it replaced.**
  Reading the command word of each segment means stopping at a
  wrapper, and `sudo`, `env`, `command`, `nohup`, `timeout` and
  `xargs` are wrappers. So `sudo mkdir -p "$d"` read as read-only,
  as did `sudo systemctl restart libvirtd` --- and the substring
  blocklist had caught both. The blind spot landed exactly where
  the command list was aimed: `systemctl`, `apt` and `install` are
  in the list precisely because a probe author might reach for
  them, and `sudo` is how they would. Wrappers are now stepped
  past, with their own flags; a shell's `-c` is treated as the
  objection because its payload cannot be read; and the command
  word is unquoted first, since `'rm'` was another free bypass.
- **`>&` is not always a duplication.** I had skipped every `>&`
  on the premise that it duplicates a descriptor. In bash, bare
  `>&word` is a documented synonym for `&>word` and **truncates
  the file** --- duplication only applies when what follows is a
  number or `-`. `vagrant plugin list >&out.txt` passed the guard.
- **A panic in a `pub` function.** The failure message took eight
  *bytes* of context after the `>`, and slicing a `str` at a
  non-boundary aborts: `mutating_token(">αααα")` panicked. Verified
  by running it before fixing it. Nothing in-tree reaches it today,
  because `Config` restricts the charset --- which is exactly the
  kind of two-modules-away reason that stops being true later.
  Eight characters now, which is what "8" was meant to mean.
- **It also misfired.** The scan was lexical over the whole
  script, so `printf 'expected >= 2 vCPUs'` and `echo "$p -> $d"`
  were reported as writing files. No current probe trips it, but a
  guard that objects to ordinary prose is one the next author
  relaxes --- and the obvious relaxation would have widened the
  real hole above. Quoted runs are now skipped, and `>=` and `->`
  excluded.
- `vagrant upload`, `push` and `plugin license` were missing, and
  `vagrant ssh -c '<cmd>'` --- a realistic way to write a probe
  that checks something inside the guest --- was not considered at
  all.
- A `!w.is_empty()` guard that could never fire, and whose
  semantics were inverted relative to the intent.

Fixing the false positive surfaced one more thing worth naming:
adding `command` to the wrapper list broke bombyx's own probes,
because `command -v tar` is a *lookup* and reports where `tar`
is without running it. The tests caught that immediately. The
distinction is now in the code rather than in whether the word
happened to be quoted.

Both of the guard's directions are now pinned in one test table,
so a later relaxation has to break a named case rather than
quietly widen the set.

### Follow-ups

- The five probe builders are near-identical and account for
  most of the 1.7% duplication. A shared private helper would
  collapse them; left alone because each one's doc comment is
  what explains why the probe is shaped as it is, and merging
  them would push that explanation away from the code.
- The plugin-missing branch of the provider probe was verified
  with a stub `vagrant` on the host's `PATH`, not by uninstalling
  the real plugin. The stub reproduces vagrant's
  `No plugins installed.` output exactly, so the branch is
  exercised, but a future vagrant that words it differently would
  not be caught.
- Resolved from the previous round: the `Action::Doctor` /
  `doctor_run` split no longer has two owners --
  `doctor::probe_commands` over `doctor::host_probes` is the one
  renderer, and the binary builds the list once.
