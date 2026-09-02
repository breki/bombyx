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
        config.rs       # bombyx.toml parsing
        remote.rs       # SSH command building
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
separately from `--host`, `BOMBYX_HOST`,
`bombyx.local.toml` or the per-developer `config.toml`, in
that order (`HostSources`). Typed errors via `thiserror`.
Computes remote paths (`remote_project_dir`,
`remote_scratch_dir`).

### `remote` -- command construction

Pure functions returning a `Command { program, args }`.
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

All gates enforced by `cargo xtask validate` and the
Claude Code Stop hook.

## Adding a New Subcommand

1. Write tests first (TDD).
2. Add the command-building function in `remote.rs`
   with unit tests asserting the exact argv.
3. Add the `Cmd` variant and its `plan()` arm in
   `main.rs`.
4. Add an integration test driving the real binary with
   `--dry-run`.
5. Run `cargo xtask validate`.
6. Commit with `/commit`.

Keep logic out of `main.rs`: it is excluded from
coverage, so anything non-trivial there ships untested.
