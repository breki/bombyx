---
description: Type-check all targets, incl. tests; runs none
allowed-tools: Bash(cargo xtask:*)
---

Type-check the whole workspace without running anything.

## Usage

`/check` -- check if the code compiles

## Output

**Success:** `Check OK`
**Failure:** shows compilation errors (first 10)

## What it covers

Every target, tests included (`cargo check --workspace
--all-targets`). That matters: without the test targets the
command reported `Check OK` while a changed function signature
had broken five call sites in the test files, and the breakage
only surfaced from the slower `/test`.

"Tests are compiled but not run" is the accurate reading -- not
"tests are ignored".

## When to use

After making code changes, before running tests. It is the
cheapest signal that everything still type-checks; on a cold or
pruned `target/` it has to build the dev-dependency tree first,
so it is not instant.

`/clippy` compiles the same target set and adds the pedantic
lints, so reach for this when you want the compile answer alone.

## Implementation

```
cargo xtask check
```
