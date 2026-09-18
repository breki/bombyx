# Artisan Findings -- Deferred backlog

Quality (Artisan) review findings. Newest first.

---

### aq-2026-09-14-error-names-downgrade-a-checked-value

**Category:** Type Safety

`ConfigError::ProjectNotFound.name` and
`ConfigError::RegistryNotFound.name` are `String`, and both
construction sites now hold a `ProjectName` and unwrap it:
`config/registry.rs`'s `project` and `config.rs`'s
`load_project`. Removing `ConfigError::Invalid` took away the
one variant that carried an unchecked name, so these two are
the last places a checked value is turned back into a string.
A reader of the error type cannot tell the name has passed
`check_segment`, and that is exactly what makes the
`[projects.<name>]` advice in the message safe to follow.

Both fields would become `ProjectName`, with
`.name.as_str()` passed to `heading` in the `#[error]`
attribute.

Deferred by the operator on 2026-09-14: a public error surface,
on a branch already carrying several breaking changes. Found as
AQ-5.

---

### aq-2026-09-14-named-has-no-single-home

**Category:** API Design (test fixtures)

`named(&str) -> ProjectName` is defined twice with the same
body, in `config.rs` and in `config/registry.rs`, and the
integration suite spells it inline four more times as
`bombyx::name::ProjectName::parse("myproject").unwrap()`. Six
spellings of one fixture.

One `#[cfg(test)] pub(crate) fn named` in `crate::name`, beside
the type whose rule it asserts, would serve both unit-test
copies; `integration_test.rs` wants a local helper next to
`load_cfg`.

Deferred by the operator on 2026-09-14: a consolidation of
three or more copies, which `/review` under **Fixing what a
stage finds** forbids applying in the round that finds it.
Found as AQ-8.

---

### aq-2026-09-04-blocks-rebuilt-per-check

**Category:** Efficiency

`collect` in `xtask/src/canon.rs` calls `reference_targets`
over every canon file and then `unresolved_xrefs` over every
canon file, and each call rebuilds the paragraph blocks from
scratch, so each file is copied into `String`s twice.
`reference_targets` also walks the content twice on its own,
once for headings and once for blocks. Building the blocks
once per file in `collect` and passing `&[Block]` to both
would end that. `Block` is private while both checks are
`pub`, so the two would have to change visibility together.

Deferred: 23 small markdown files, so the cost is invisible
today.
