# Artisan Findings -- Deferred backlog

Quality (Artisan) review findings. Newest first.

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
