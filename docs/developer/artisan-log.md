# Artisan Findings -- Deferred backlog

Quality (Artisan) review findings. Newest first.

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

