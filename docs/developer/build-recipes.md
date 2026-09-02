# Build and toolchain recipes

Three recipes for configuring the build, each needed rarely and
none of them a rule you follow on every commit. `CLAUDE.md`
carries the always-on standards; these are here so they do not
sit in every session's context.

- **Scoped `unsafe` in `xtask`** -- when build tooling needs an
  OS API and the workspace forbids `unsafe_code`.
- **Coverage exceptions for hardware-bound code** -- when an I/O
  path cannot run under `cargo llvm-cov` and the 90% gate has to
  stay honest anyway.
- **Edition-2024 migration** -- the mechanical fixes a project
  inheriting an older snapshot hits once.

## Workspace lints and xtask overrides

The workspace forbids `unsafe_code` via
`[workspace.lints.rust]` so production crates inherit
the policy by default. If a derived project needs OS-
specific code in `xtask/` (for example, calling Win32
APIs for process management on Windows -- the canonical
case being `OpenProcess` / `TerminateProcess` /
`CreateToolhelp32Snapshot` for stale-server cleanup),
the recipe is to redefine the lints block locally for
`xtask` only rather than weakening the workspace policy:

```toml
# xtask/Cargo.toml
[lints.rust]
warnings = "deny"
unsafe_code = "allow"   # xtask is build tooling, scoped exception

[lints.clippy]
# inherit the workspace clippy block by re-declaring
# or by overriding selectively
```

Production crates keep `[lints] workspace = true` and
remain `unsafe`-forbidden. Document the scoped
exception with a comment near the use site so reviewers
can verify the unsafe block is genuinely necessary.


## Coverage exceptions for hardware-bound code

The 90% coverage gate (see **Definition of Done** in
`CLAUDE.md`) assumes
every code path can run under `cargo llvm-cov` in CI.
Real projects routinely have I/O paths that can't:
audio playback, network calls against external
services, native API calls (Win32, CoreAudio, ALSA),
GPIO on embedded targets. The recipe for keeping the
gate honest without weakening it:

1. **Extract the hardware-bound code into a sibling
   submodule.** Given `foo.rs` that contains both
   business logic and an I/O call, split into `foo.rs`
   (the orchestrator) and `foo/bar.rs` (the I/O leaf).
   The leaf module should be as small as possible --
   ideally just the unmockable call plus its
   immediate error mapping.
2. **Exclude the leaf submodule via manifest config.**
   Add its path (a regex fragment) to
   `[workspace.metadata.coverage] ignore` in the **root
   `Cargo.toml`** -- no need to fork `xtask`:

   ```toml
   [workspace.metadata.coverage]
   # Each entry is a regex fragment merged into the coverage
   # --ignore-filename-regex baseline. Use single-quoted TOML
   # literal strings so backslashes reach the regex verbatim
   # (no doubling).
   ignore = ['src[/\\]audio[/\\]playback\.rs']
   ```

   `cargo xtask coverage` merges these with its built-in
   baseline (`src/main.rs`, `src/bin/`); the leaf module is
   exempted from the gate, the orchestrator is not. An absent
   section leaves the baseline unchanged, and a
   missing/unreadable manifest degrades to the baseline rather
   than failing. A pattern that would match *every* file
   (empty, `.`, `.*`, `.+`) is rejected -- it would silently
   neuter the gate. Only the `[workspace.metadata.coverage]` +
   line-leading `ignore = [...]` shape is read; the dotted-key
   (`coverage.ignore = ...`) and inline-table spellings are
   not.
3. **Add a `*_TEST_*` env-var escape hatch in the
   excluded module.** For example, `RUSTBASE_TEST_AUDIO`
   short-circuits the real native call and returns a
   fixed `Ok`/`Err` shape. This keeps the parent
   module's post-call success and error branches
   testable -- they're the parts that actually carry
   business logic, and they remain inside the 90% gate.

What this gets you: the orchestrator is fully covered
(including both branches of its `match
play_audio_native() { Ok => ..., Err => ... }`), the
leaf is honestly acknowledged as untested in CI, and
there's no `#[cfg(test)]` test-only branch leaking into
production code paths.

When NOT to use this recipe: if the I/O can be faked
with a trait + dependency injection at the call site
without contortions, do that instead. The submodule-
plus-ignore-regex pattern is for cases where the
indirection itself would obscure the code more than it
reveals.


## Edition-2024 migration notes

The template ships on Rust edition 2024. Projects
inheriting from an older snapshot of the template (or
upgrading from edition 2021) routinely hit a small set
of mechanical fixes that `cargo fix --edition` either
applies automatically or flags:

- **Unsafe extern blocks**: `extern "C" { fn foo(); }`
  must become `unsafe extern "C" { fn foo(); }`. Each
  declaration inside is still individually `unsafe fn`.
- **Match ergonomics tightening**: bare `ref` patterns
  inside a binding that already implies a reference
  must be dropped. `match x { Some(ref y) => ... }`
  becomes `match x { Some(y) => ... }` when the outer
  match already produces a reference.
- **`gen` is reserved**: any identifier called `gen`
  (variables, function names, struct fields) needs the
  raw-identifier form `r#gen` or a rename.
- **Nested `if let` -> let chains**: clippy's autofix
  collapses `if x { if y { ... } }` into
  `if x && y { ... }` once `let`-chains are stable.
  This is a clippy fix rather than an edition fix, but
  it lands at the same time and is worth running in the
  same pass.

Run `cargo fix --edition --workspace` followed by
`cargo xtask validate` and expect a small follow-up
pass for the items above.

