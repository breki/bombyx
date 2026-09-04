---
name: architect
description: Project overview and architecture guide for bombyx -- crate layout, responsibilities, quality gates, and how to add a feature.
---

# bombyx Architecture

## Project Identity

| Field | Value |
|-------|-------|
| Language | Rust (edition 2024, stable toolchain) |
| Shape | CLI only -- no services, no frontend |
| License | MIT |
| Version | `crates/bombyx/Cargo.toml` (single source) |
| Versioning | SemVer 2.0.0 |
| Platforms | Windows (workstation), Linux |
| Template | derived from `breki/rustbase` |

## What bombyx Does

Drives isolated AI-agent VMs on a remote libvirt host
over SSH. The control plane is deliberately thin: bombyx
generates a Vagrantfile and a bootstrap script, writes them
onto the VM host and runs `vagrant` there, streaming output
back.

The key architectural constraint: **the VM host holds no
project code.** Both files are generated from `bombyx.toml`
and rewritten on every boot, so the host cannot drift, and
the guest clones the project itself once it is running. See
`docs/trust-boundary.md`.

## Repository Layout

```
bombyx/
  .cargo/
    config.toml         # cargo xtask alias
  .claude/
    hooks/              # Claude Code hook scripts
    commands/           # slash commands
    skills/             # domain knowledge skills
    settings.json       # hook configuration
  crates/
    bombyx/
      src/
        lib.rs          # crate root, re-exports
        plan.rs         # which commands run, in what order
        config.rs       # bombyx.toml parsing (submodules)
        remote.rs       # SSH command building (submodules)
        vagrantfile.rs  # renders the two generated files
        doctor.rs       # preflight checks (submodules)
        update.rs       # self-update (submodules)
        name.rs term.rs tool.rs
        bin/bombyx/
          main.rs       # CLI entry point (thin)
      tests/
        integration_test.rs
  xtask/
    src/
      main.rs           # build automation
  scripts/              # bash wrappers
  docs/
    developer/
      DIARY.md          # development diary
      redteam-log.md    # security review findings
      artisan-log.md    # quality review findings
```

## Module Responsibilities

### `config` -- project configuration

Parses `bombyx.toml`: `project`, `remote_root`, `[vm]` and
`[source]` -- and *refuses* `host`, which belongs to the
developer rather than the repo. `host` is resolved
separately from `--host`, `BOMBYX_HOST` or the
per-developer `config.toml`, in that order
(`HostSources`). Typed errors via `thiserror`.
Computes remote paths (`remote_project_dir`,
`remote_scratch_dir`).

### `remote` -- command construction

Pure functions returning a `RemoteCommand { program, args, dir }`.
**Nothing here spawns a process.** That separation is
what makes quoting, path joining and command composition
unit-testable with no VM host in the loop.

Includes `shell_quote` for POSIX single-quote escaping --
every value interpolated into a remote script goes
through it.

### `bin/bombyx/main.rs` -- entry point

Clap parsing, then `plan()` maps a subcommand to a
sequence of `Command`s, which are either printed
(`--dry-run`) or executed. Kept thin because coverage
excludes `src/bin/`.

### `xtask` -- build automation

Not published. `validate`, `test`, `clippy`, `fmt`,
`coverage`, `dupes`, `audit`, `dep-age*`.

## Quality Gates

| Gate | Threshold |
|------|-----------|
| Clippy | Zero warnings (`-D warnings`) |
| Formatting | `cargo fmt --check` |
| Coverage | 90% overall, 85% per module |
| Duplication | <= 6% (production code) |
| Unsafe code | Forbidden (`#[forbid(unsafe_code)]`) |
| Advisories | RUSTSEC clean |

`cargo xtask validate` runs every gate; the six above are
the ones worth memorising, and `CLAUDE.md` under
**Definition of Done** lists all nine in execution order.

The Claude Code Stop hook runs a **subset**: fmt-check,
clippy, doc and test. It skips coverage and duplication
deliberately, because both are slow -- see
`.claude/hooks/stop-check.sh`. A coverage regression is
caught by `validate`, not by the hook.

## Adding a New Subcommand

1. Write tests first (TDD).
2. Add the command-building function in `remote.rs`
   with unit tests asserting the exact argv.
3. Add the `VmCmd` variant and its `action_of` arm in
   `main.rs` -- the CLI surface, and nothing else.
4. Add the `Action` variant and its `plan()` arm in
   `plan.rs`, with the unit test asserting the ordered
   commands. This is where the logic goes: `main.rs` is
   excluded from coverage.
5. Add an integration test driving the real binary with
   `--dry-run`.
6. Run `cargo xtask validate`.
7. Commit with `/commit`.

Keep logic out of `main.rs`: it is excluded from
coverage, so anything non-trivial there ships untested.
