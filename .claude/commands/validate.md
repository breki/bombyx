---
description: Run all quality checks with stepwise progress
allowed-tools: Bash(cargo xtask:*)
---

Run the full validation pipeline with concise output.

## Usage

`/validate` -- run all 8 gates

`/validate --check` -- same, but check formatting read-only
instead of auto-fixing it in place. Use in CI, or before
staging part of a working tree, where an in-place rewrite
would sweep unrelated drift into the commit.

## Output

```
[1/8] Dep-age....... OK (1 changed: 1 aged, 0 allow-listed, 0.5s)
[2/8] Fmt........... OK (0.3s)
[3/8] Duplication... OK (<= 6%, 0.1s)
[4/8] Clippy........ OK (0.7s)
[5/8] Doc........... OK (1.5s)
[6/8] Test (xtask only) OK (0.3s)
[7/8] Coverage...... OK (99.4% >= 90%, 6.6s)
[8/8] Audit......... OK (cargo: 0 vuln, 0 warn, 2.1s)
Validate OK (12.1s)
```

The order is deliberate; `CLAUDE.md`'s "Definition of Done"
explains it. In short: the dependency-cooldown gate runs first
because it is free on an unchanged lockfile and fails before
anything compiles a too-new crate, then the cheap static gates,
then the expensive dynamic ones.

A failing step prints the single subcommand that re-runs just
that gate, so iterate with that rather than the whole pipeline.

## Implementation

```
cargo xtask validate
```
