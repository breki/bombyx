---
description: Fast compilation check (no tests)
allowed-tools: Bash(cargo xtask:*)
---

Fast compilation check without running tests.

## Usage

`/check` -- check if code compiles

## Output

**Success:** `Check OK`
**Failure:** shows compilation errors (first 10)

## When to use

After making code changes, before running tests --
catches syntax/type errors in under a second.

## Implementation

```
cargo xtask check
```
