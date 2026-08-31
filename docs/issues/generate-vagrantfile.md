# generate-vagrantfile

**Status:** Done
**Captured:** 2026-08-30
**Started:** 2026-08-30
**Completed:** 2026-08-30

## Problem

bombyx ships the project's `vagrant/` directory to the VM host and
runs `vagrant` there, so every project writes and maintains its own
Vagrantfile. `docs/trust-boundary.md` decides that the guest is the
only machine holding project source, and that decision cannot be
carried out while the Vagrantfile comes from the project: Vagrant
needs the file before the VM exists.

bombyx therefore generates the Vagrantfile itself, from a description
of the machine held in `bombyx.toml`, and the file it generates
provisions the guest by cloning the project repository inside the
guest and running a script from it.

## Context

### What happens today

`plan::push_then` builds three steps for `up`, `provision` and
`scratch`: create the remote directory, push the local `vagrant_dir`
into it as a tar over scp, then run `vagrant` in that directory.
`Action::pushes` marks which actions need the archive.

`Config` holds four fields -- `host`, `project`, `vagrant_dir`,
`remote_root`. None of them describes a virtual machine, so there is
nothing today from which a Vagrantfile could be rendered.

`doctor::vagrantfile_finding` fails when `<vagrant_dir>/Vagrantfile`
is absent. Under this change an absent file is the normal case, so
that probe states the opposite of what it should.

`remote::quote::shell_quote` already exists and is the tool for the
argument-quoting half of this work.

### Two files, not one, and why that matters

The first draft of this plan put the provisioning script inline in
the Vagrantfile, which meant config values crossed three nested
contexts: Ruby, a shell inside a Ruby heredoc, and a shell on the VM
host. That is the failure class `CLAUDE.md` names specifically, and
it was the largest risk in the change.

Vagrant's shell provisioner takes `path:` rather than `inline:`, so
the script can be a separate file, and it takes `env:` for values the
script needs. bombyx therefore writes **two** files to the remote
directory:

- `Vagrantfile` -- rendered per project. Ruby.
- `bootstrap.sh` -- **identical for every project**, so it is a
  constant compiled into the binary with `include_str!` and shipped
  verbatim. bombyx interpolates nothing into it.

One nesting level disappears. `repo`, `ref` and `script` reach the
guest as environment variables set by Vagrant, so they never become
shell text that bombyx composes. The only escaping bombyx still owns
is Ruby string literals.

Validation stays anyway. A newline or a quote in `box` or `repo`
would break the Ruby file whatever else is true, and a field that
cannot contain such a character should say so where it lives.

## Decisions

- **2026-08-30 -- bombyx always generates the Vagrantfile.** A
  project's own Vagrantfile is ignored. Operator decision, taken over
  the alternatives of generating only when absent, or opting in by
  config. This is a breaking change and the entry is filed
  `**BREAKING:**`.
- **2026-08-30 -- Both providers ship; Hyper-V is marked
  unverified.** libvirt is the only provider that has ever booted a
  machine under bombyx, so the Hyper-V template is written from the
  provider's documented options and labelled as never run.
- **2026-08-30 -- The files are written to the VM host over SSH**
  rather than staged locally and pushed, or written into the repo.
- **2026-08-30 -- The provisioning script is a separate file, not
  inline Ruby.** Operator observation that a Vagrantfile can
  reference non-Ruby files, which is correct and better than the
  first draft: `path:` plus `env:` makes the script identical for
  every project, so it ships verbatim and nothing is interpolated
  into shell text. Removes one of three nested quoting contexts,
  which was the largest risk in the change.
- **2026-08-30 -- All four `[vm]` fields are required**: `provider`,
  `box`, `cpus`, `memory`. A project must state all four, so its
  VM size is visible in its own repo. Consequence accepted: every existing
  `bombyx.toml` stops validating until it gains the section.
- **2026-08-30 -- The provisioner clones the repository inside the
  guest** and runs a script from the clone. This closes the
  guest-clone half of `remote-clone-project-source` as well. Assumed,
  for consistency with the `[vm]` answer and flagged here rather than
  asked separately: the three `[source]` fields are required too.

## Open questions

None blocking. Two things stay unresolved and are recorded in
`docs/trust-boundary.md` rather than here: a private repository needs
a credential inside the guest, and the guest needs egress to the git
host.

## Plan

1. **`crates/bombyx/src/config.rs`** -- add `[vm]` and `[source]`
   to `ProjectFile` and `Config`. `provider` is an enum
   (`libvirt`, `hyperv`) so an unknown value fails at parse rather
   than at boot. Validation lives in `Config::validate`, beside the
   fields: reject control characters, newlines and quote characters
   in `box`, `repo`, `ref` and `script`; reject `cpus` or `memory`
   of zero. The per-project override file gains the same fields.

2. **`crates/bombyx/src/vagrantfile.rs`** (new) -- a pure
   `render(&Config) -> String` for the Vagrantfile, plus
   `BOOTSTRAP`, the static script held as `include_str!` of
   `templates/bootstrap.sh`. One function per provider for the
   provider block, one shared skeleton. Ruby string literals are
   escaped. No I/O, so the whole module is reachable by unit tests.

3. **`crates/bombyx/templates/bootstrap.sh`** (new) -- the script
   itself, kept as a real file rather than a string literal in Rust
   so it is syntax-highlighted, lintable by `shellcheck`, and
   diffable. It clones `$BOMBYX_REPO` at `$BOMBYX_REF` and runs
   `$BOMBYX_SCRIPT` from the clone, with `set -euo pipefail` and a
   `:?` guard on each variable so a missing one fails at once
   instead of cloning into an empty path.

4. **`crates/bombyx/src/remote.rs`** -- add `write_file(cfg, dir,
   name, contents)`, returning a `RemoteCommand` that writes the
   contents through a quoted heredoc so the host shell performs no
   expansion.

5. **`crates/bombyx/src/plan.rs`** -- `push_then` gains two write
   steps between the push and the `vagrant` invocation, one per
   file, so both land after the archive is unpacked and win over
   anything the project shipped.

6. **`crates/bombyx/src/doctor/local.rs`** -- `vagrantfile_finding`
   changes meaning. An absent project Vagrantfile is no longer a
   failure. A present one is reported, because it is now ignored and
   silently ignoring it is what would confuse an operator. `Outcome`
   has no warning variant, so this is a `Pass` carrying the detail.

7. **`README.md`** -- document the two new sections, and revisit the
   Model section's rule 1, which the previous commit marked as being
   replaced. Part of it now genuinely is.

8. **`CHANGELOG.md`** -- `**BREAKING:**` entries for the ignored
   Vagrantfile and the newly required config, via
   `cargo xtask changelog add`.

## Test strategy

Unit tests for the library logic, which is where nearly all of it
is. This is a CLI-only project: no browser, no Vitest, no Playwright.

- **`vagrantfile.rs`** -- rendering per provider; every required
  field appearing in the output; a `box` containing a double quote
  and a `repo` containing a single quote and a semicolon, asserting
  the dangerous character cannot escape its context. These are the
  tests that matter most.
- **`config.rs`** -- the whole family of rejected inputs, enumerated
  before the guard is written: control characters, a newline, a
  quote, an embedded `$(...)`, zero `cpus`, zero `memory`, an
  unknown provider, and each field missing.
- **`plan.rs`** -- the write step appears in `up`, `provision` and
  `scratch`, in the right position relative to push and vagrant, and
  is absent from `down`, `status`, `destroy` and `discard`.
- **`doctor/local.rs`** -- both branches of the changed probe.
- **Integration** -- `--dry-run` against the real binary, asserting
  the emitted argv.

TDD: this is behaviour change throughout, including the new module,
whose `render` carries branching and escaping rather than being a
data declaration. Failing test first in every case.

**Definition of Done item 3 applies and is the part a green suite
cannot cover.** This changes the commands bombyx emits *and* what
runs inside the guest. A dry run proves the argv and proves nothing
about whether Vagrant parses the generated file or the clone
succeeds. A real `up` against a real VM host is required before this
is done, and the outcome is reported plainly either way.

## Progress log

- **2026-08-30** -- Config, renderer, remote write, plan wiring,
  doctor probe, README and CHANGELOG all landed. `cargo xtask
  validate` passes all nine gates.

## Outcome

`[vm]` and `[source]` are required tables in `bombyx.toml`, with
seven fields and no defaults. `crates/bombyx/src/vagrantfile.rs`
renders the Vagrantfile; `crates/bombyx/templates/bootstrap.sh`
is the script, identical for every project and shipped verbatim
by `include_str!`. `remote::write_file` sends each through a
quoted heredoc, and `plan::push_then` places both between the
unpack and the `vagrant` call, so they win over anything the
project shipped.

Design changes made during the work, both from operator review:

- The provisioner uses `path:` and `env:` rather than an inline
  Ruby heredoc. That removed one of three nested quoting
  contexts -- config values now reach the guest as environment
  variables Vagrant sets, never as shell text bombyx composes.
- `--dry-run` abbreviates the two payloads to one line each,
  naming the heredoc and the number of lines dropped. Without
  it, `up` printed about seventy lines in which a payload line
  could not be told from the next command. `plan`'s doc comment
  says where the dry run stops being literal, and
  `remote::abbreviated` does the work in the library rather than
  in `src/bin/`, which the coverage gate cannot see.

Verified here, not inferred:

- `vagrant validate` accepts the generated Vagrantfile. Vagrant
  2.4.9 with vagrant-libvirt 0.12.2 -- the versions
  `README.md` records for frosti -- is installed on this
  workstation. Kept as an `#[ignore]`-tagged test in
  `tests/integration_test.rs`; run it with `cargo xtask test
  --ignored`. This proves the file parses and its config
  validates. It does **not** prove a VM boots or the clone
  succeeds.
- `shellcheck` reports `bootstrap.sh` clean, and `bash -n`
  parses it.
- `bombyx --dry-run up` against a realistic config emits seven
  commands, one line each.

**Definition of Done item 3 is not met.** Nothing has been run
against frosti. A dry run proves the argv and local validation
proves the syntax; neither proves the VM host accepts the
heredoc write, that the guest can reach the git host, or that
the clone and the project's script run.

Follow-ups:

- **A typo in `vagrant_dir` is no longer caught.** The doctor
  probe used to fail on a missing Vagrantfile, which detected it
  cheaply. An absent Vagrantfile is now ordinary, so that signal
  is gone and nothing replaced it.
- **The Hyper-V template has never booted a machine.** It is
  written from the provider's documented options and marked as
  such in `Provider`.
- **`remote-clone-project-source` is only half closed.** The
  guest clones the project, but the workstation still needs a
  checkout, because `bombyx.toml` is read from the working
  directory and `vagrant_dir` is still pushed.
- **A private repository still needs a credential in the
  guest.** Recorded in `docs/trust-boundary.md` as accepted and
  unsolved.
- **The abbreviated dry-run line leaves its opening quote
  unclosed**, since the command is truncated. It reads as
  truncation rather than as a pasteable command, which is
  arguably right, but it is a deliberate choice rather than an
  oversight.

## Review round

Both reviewers ran against the staged diff and returned 25
findings. Two were cross-confirmed, and three changed the
design rather than the wording.

**The change did not achieve its own purpose.** The generated
Vagrantfile left Vagrant's default `/vagrant` share on, so the
guest mounted the workstation's pushed copy of the project --
the copy `docs/trust-boundary.md` exists to keep out of it. On
a host with the documented nftables rules that mount also
hangs. `docs/tutorial.md` had taught disabling it, in a
Vagrantfile bombyx now overwrites. The generated file disables
it, pinned by a test.

**A shipped feature had gone inert.** `BOMBYX_VM_HOST` and
`BOMBYX_VM_HOSTNAME` reach `vagrant` on the host, and the
project's own Vagrantfile used to hand them to the guest.
Overwriting that file broke the hand-over while `README.md`
and `docs/tutorial.md` still taught writing it. The generated
file forwards both.

**The guards stopped at the Ruby literal.** `check_renderable`
protected the generated file and nothing else, while the three
`[source]` fields also reach `git` argv and a path that is made
executable and run as root in the guest. `ref =
"--upload-pack=..."` was read as an option, `repo = "ext::sh -c
..."` ran a command instead of cloning, and `script` had no
path check at all -- while its siblings `vagrant_dir` and
`remote_root` had carried those rules for weeks. This is the
"guard one field, check its siblings" rule, missed again.

**The heredoc invariant was enforced in the wrong place.**
`write_file` is public and relied on a guard in another module
plus a test over one fixture. It now lengthens the delimiter
until no payload line equals it, which cannot fail and needs
nothing from the caller.

Cross-confirmed by both reviewers: `out.contains("4")` matched
the box name `generic/ubuntu2204`, so the only test covering
`cpus` could not fail; and `requires_both_new_sections` built
its input by truncating at a header, so the `[vm]` case deleted
`[source]` too and neither section was tested on its own.

Also applied: the bootstrap script resets `origin` before
fetching (changing `source.repo` and re-provisioning silently
kept the old repository), checks for `git`, and chowns the
clone to the `vagrant` user so the agent can write to it; the
failure path prints the abbreviated command, which the dry run
already did; `charset` got back the doc comment my insertion
had landed inside; `config/vm.rs` and `remote/write.rs` split
out; `Provider` gained `Display`; the fixture TOML went from
five copies to two; and `doctor` regained a failing branch.

Deferred to backlog rather than fixed: nothing. The refactors
were applied at the operator's direction.
