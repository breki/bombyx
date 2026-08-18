# Artisan Findings -- Deferred backlog

Quality (Artisan) review findings. Newest first.

## aq-2026-08-18-validate-step-numbers-are-literals

**Category:** API design

`xtask/src/validate.rs` carries nine hardcoded step indices plus a
`TOTAL_STEPS` const that has to agree with them by hand, and nothing
checks the two. Inserting the Deny step meant editing six call
sites; missing one would print `[5/9]` twice or `[9/10]` last, and
no test would fail.

Fix: build a table of `(name, cmd, closure)`, take `total` from
`len()`, and enumerate it -- the numbering becomes derived data and
`TOTAL_STEPS` disappears. Deferred because it touches every step in
a file whose gate was being changed for other reasons.

## aq-2026-08-18-xtask-modules-over-public

**Category:** API design

`xtask/src/licenses.rs` exposes `DEFAULT_OUT`, `Attribution`,
`is_license_file`, `collect`, `attributions_from` and `render` as
`pub`, and `deny.rs` exposes `DenyResult` and `classify`, although
`main.rs` calls only the two entry points. `audit.rs` -- the module
both were modelled on -- keeps its internals private and exposes
only what `validate` needs.

`pub` on an xtask internal is not a compile error, so it becomes the
pattern for the next module, and it drags every item into the
rustdoc pass and the "all public items documented" obligation.
`#[cfg(test)] mod tests` reaches private items through `use
super::*` regardless.

Fix: make everything private except `deny()` and `licenses()`. While
there, `DenyResult { ok: bool, detail: String }` would read better
as an enum naming its three outcomes (passed, failed, tool missing),
which is the distinction its own tests care about.

## aq-2026-08-18-self-update-composition-untested

**Category:** Testability / abstraction boundaries

`self_update` in `crates/bombyx/src/bin/bombyx/main.rs` is ~120
lines of composition in the binary: it renders every `Decision`
variant's operator-facing text, chooses the temp-directory layout,
derives both URLs, builds three commands, and encodes the ordering
rule that `SHA256SUMS` is fetched *before* the archive. `update.rs`
and `update/asset.rs` are thoroughly tested; this composition is
not, and `src/bin/` is outside the coverage gate. The one function
holding the ordering invariant that verification depends on has no
test.

Suggested shape: a library `plan_update(latest, triple,
install_dir, work) -> UpdatePlan` returning the archive name, both
paths, and the three commands, unit-tested for the URLs, the
extract target, and that the sums fetch comes first. `self_update`
then shrinks to parse-capture-match-run. Rendering `Decision` to
text could move too.

Deferred because the correctness findings from the same review
(the `?`-in-loop parser bug, the Windows `tar` drive-letter
failure, the wrong install directory) were fixed instead, and
landing a refactor in the same commit would have buried them.
Note the module doc still claims the binary is "thin by design:
parse arguments, hand off to the library", which this function
contradicts.

## aq-2026-08-18-update-rs-mixes-pure-and-io

**Category:** Module structure

`crates/bombyx/src/update.rs` is past 900 lines and holds four
concerns: version/tag parsing and the update decision (pure), argv
builders, environment probing for the install directory, and
filesystem manipulation of the installed binary (`move_aside`,
`restore`, `place`, `sweep_aside`). The last group renames and
deletes files, which is the only I/O in the crate's
command-building layer.

Suggested split by *effect* rather than by topic:
`update/version.rs` for the pure decision half, `update/swap.rs`
for the filesystem half, leaving `update.rs` as the facade.
`update/asset.rs` is a good boundary already and should stay.

The module doc header was corrected in this commit to say which
half touches the filesystem, so the misleading claim is gone; the
split itself is still worth doing. Second review to raise this
file's size (see `aq-2026-08-17-remote-rs-holds-three-concerns`
for the sibling).

## aq-2026-08-18-execute-returns-an-exit-code

**Category:** API design

`execute` in `main.rs` returns `Result<ExitCode>` -- a
process-exit type -- as its domain answer, so every caller that
wants "did it work" re-derives it. `ran_ok` does
`execute(...)? == ExitCode::SUCCESS`, leaning on `ExitCode:
PartialEq`, which is not what that opaque type is for.
`std::slice::from_ref(cmd)` also appears three times to feed a
single command into a slice API.

Suggested: give `execute` a return type that says what happened
(`struct Ran { ok: bool, code: ExitCode }`, or
`Result<Result<(), ExitCode>>`) and convert at the `main`
boundary; add an `execute_one` wrapper so `from_ref` appears once.

Partially defused in this commit: the second, differently-spelled
copy of the predicate inside `replace_binary` is gone, because the
move-aside dance moved into the library as `update::place`, which
returns a `Result` rather than an `ExitCode`. One spelling is left.

## aq-2026-08-18-selfupdate-arm-is-unreachable

**Category:** Type safety

`action_of` carries a `Cmd::SelfUpdate => bail!("internal error:
...")` arm that cannot be reached, guarded only by a `matches!` in
`run` some four hundred lines away. The invariant is maintained by
a comment rather than by the types, and the next subcommand that
also bypasses the config (`completions`, a `version --check`) adds
a second such arm.

Suggested: `enum Cmd { SelfUpdate, Vm(VmCmd) }` with
`#[command(flatten)]`, so the CLI surface is unchanged and
`action_of(&VmCmd, ...)` becomes total, deleting both the arm and
the `matches!` sentinel.

## aq-2026-08-17-remote-rs-holds-three-concerns

**Category:** Module structure

`crates/bombyx/src/remote.rs` is now past 800 lines and holds
three unrelated things: `RemoteCommand` plus its `Display`,
`PushArchive`, the shell-quoting primitives (`is_plain`,
`display_arg`, `shell_quote`, `quote_remote_path`) and every
command builder. The quoting primitives are pure functions with
their own dense test block and no dependency on `Config`, so they
read as a separate unit a reader has to scroll past.

Suggested split: `remote/quote.rs` for the quoting primitives and
their tests, `remote/command.rs` for `RemoteCommand` and
`PushArchive`, leaving `remote.rs` as the builders plus the
VM-host identity constants. `shell_quote` and `quote_remote_path`
are already `pub`, so re-exporting them from `remote` keeps the
public paths unchanged.

Deferred rather than done: it is a pure move touching every test
module in the file, and it would have buried the behaviour change
it was raised against (the VM-host identity prefix) in rename
noise. The file grew by roughly 60 production lines in that
commit, so this is not urgent -- but it is the second review to
mention the file's size.

## aq-2026-08-13-config-parse-is-test-only

**Category:** API design / test-only public surface

`Config::parse(source: &str, path: &Path, host: &str)` is public
and has no production caller. Its only users are `for_tests` and
two integration tests; `main.rs` goes through `Config::load`.

Two things follow. The signature takes two adjacent `&str` with
different meanings, so `parse(host, path, source)` compiles and
fails at run time with a confusing charset error. And it hardcodes
`&HostSources::default()` when refusing a `host` key, so the same
repo mistake produces a less helpful message than `load` gives --
the per-developer `config.toml` is not named.

The suggestion was either a `Host` newtype carrying the charset
check (which would also stop the argument swap), or demoting
`parse` to a test-only constructor and having the integration
tests use `Config::load` against a temp fixture like the rest of
the suite.

Deferred because the commit that raised it already changed this
module's public surface twice over (`load` gained a `HostSources`
parameter and a `HostOrigin` return), and a newtype for `host`
touches every construction site. Worth doing as its own change,
where the diff is about the type and nothing else.

## aq-2026-08-11-config-module-size

*(Flagged again on 2026-08-13, larger: the host-resolution work
added `ProjectFile`, `UserFile`, `HostSources`, `HostOrigin`,
three constants, five free functions and ~450 lines of tests, and
the file is past 1900 lines. The suggested split is now
`config/host.rs` for everything host-resolution -- the constants,
`HostSources`, `HostOrigin`, `UserFile`, `user_config_dir`,
`config_dir_from`, `is_anchored_dir`, `resolve_host`,
`host_places`, `host_problem` and their tests -- leaving
`config.rs` as the project-file parser and orchestrator. Deferred
for the same reason as before: that commit was fixing two real
defects in this file, and moving the code around the fixes makes
both harder to review.)*

**Category:** Module size / cohesion

`crates/bombyx/src/config.rs` passed 900 lines with the overlay
work, and now holds three public types (`ConfigError`, `Config`,
`Overlay`), the charset and path validators, the overlay
discovery and reading helpers, and roughly half the file in
tests.

The suggestion was a `config/overlay.rs` submodule holding
`Overlay`, `local_config_path`, `read_optional` and their tests,
re-exported from `config`, matching the existing `doctor/` and
`remote/` split.

Deferred because the same commit was closing a live security
hole in this file (`vagrant_dir` accepting an absolute path, so
`up` archived it), and moving code around a fix makes both
harder to review. The validation rules are the part worth
protecting from churn.

Revisit when the next change touches this module for a reason
other than a fix -- the split is a good one, it just should not
ride along with a security patch.

## aq-2026-08-10-push-expectation-duplication

**Category:** Craftsmanship (test duplication)

`plan.rs`'s `up_makes_the_dir_then_pushes_then_boots` and
`provision_pushes_then_reprovisions` each spell out the same
five-command expected script as a hand-escaped literal block,
differing only in the trailing `vagrant 'up'` versus
`vagrant 'provision'`. The review suggested an
`expected_push(dir, subcommand)` helper, so the one-token
difference is visible at the call site and a change to the push
sequence lands once instead of twice.

Deferred deliberately. The literal blocks are meant to be dumb
pins: they read as the exact shell bombyx emits, which is what
makes them useful as documentation, and two independently
written expectations cannot both drift the same wrong way, which
one shared builder can. The duplication is bounded -- a third
exact-script test would change this judgement -- and
`provision_and_up_take_the_same_shape` now carries the "these
two differ only in their last step" claim that the helper would
have made visible.

Revisit if a third caller of `push_then` gains its own
exact-script test.

## aq-2026-08-10-doctor-module-size

**Status:** Resolved 2026-08-10, in the commit after the one that
deferred it. `doctor.rs` is now a directory module of five
submodules, with the public names re-exported so no caller
changed.

Measured before `#[cfg(test)]`, in lines of code (blanks and
comments excluded): `readonly.rs` 193, `probes.rs` 97,
`report.rs` 79, `doctor.rs` 68, `local.rs` 62, `text.rs` 36.
`readonly.rs` is still the outlier this entry was about, and it
is left whole deliberately: it holds one concern, declares no
types, and most of its bulk is the explanation of why each entry
in the blocklist is there. Splitting it would separate the list
from that explanation.

**Category:** Module size / cohesion

`crates/bombyx/src/doctor.rs` is ~840 lines of production code
plus ~490 of tests, and holds five concerns that barely touch
each other: the data model (`Scope`, `Outcome`, `ProbeResult`,
`VersionAnswer`, `Finding`), the probe list and cascade, the
read-only guard (`MUTATING_COMMANDS` and the small shell
tokenizer), untrusted-text handling (`sanitize`, `clip`), the
local-tool findings, and the renderer. `remote/probe.rs` was
split out on exactly this reasoning during the same review; the
argument applies one level up.

The suggested shape is a directory module: `doctor.rs` keeps the
model, with `doctor/probes.rs`, `doctor/readonly.rs`,
`doctor/text.rs`, `doctor/local.rs` and `doctor/report.rs`, each
carrying its own `mod tests` and each well under 300 lines.
Public names re-exported from `doctor` so no caller changes.

**Deferred deliberately, not rejected.** It is a pure
reorganisation of ~1300 lines with no behavioural effect, and it
arrived in the same round as six verified correctness and
security fixes (the `PATH` resolution hole, the unsearchable-
directory false pass, the read-only guard, the report's
character allowlist, the detail-budget collapse, the dead dry-run
arm). Folding a whole-file move into that commit would bury those
fixes in a diff nobody can review, and a mechanical slip during
the move would put verified behaviour at risk. It should be its
own commit, with no other change in it.

## aq-2026-08-09-collect-changes-generality

**Category:** API design / leftover generality

`collect_changes` in `xtask/src/dep_age/gate.rs` still takes a
`parser: fn(&str) -> Vec<(String, String)>` function pointer
and two `&mut Vec` out-params. That shape earned its keep
while the function was called twice, once for `Cargo.lock`
and once for `frontend/package-lock.json`. After the npm
purge it has a single call site, `parser` has exactly one
possible value (`parse_cargo_lock`), and the out-params exist
only so the caller could accumulate across two invocations.

A function-pointer parameter reads as "there are several
parsers here" and sends the reader looking for the other one.

Suggested shape:
`fn collect_changes(rel: &str, allow: &HashSet<String>) ->
(Vec<(String, DepOutcome)>, Vec<String>)`, with the caller
destructuring the pair.

Deferred deliberately: this is the supply-chain gate, and the
same commit had just changed its failure handling (the silent
read-failure pass) and verified that end-to-end against a
missing `Cargo.lock`. Reshaping it again in the same change
would have traded verified behaviour for style. The dead
generality is inert -- it costs a reader's attention, not
correctness -- so it is safe to carry until someone is next
working in this file.
