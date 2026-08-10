# Artisan Findings -- Deferred backlog

Quality (Artisan) review findings. Newest first.

## aq-2026-08-10-doctor-module-size

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
