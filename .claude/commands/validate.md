---
description: Run all quality checks with stepwise progress
allowed-tools: Bash(cargo xtask:*)
---

Run the full validation pipeline with concise output.

## Usage

`/validate` -- run all 5 checks

## Output

```
[1/5] Fmt........... OK (0.2s)
[2/5] Clippy........ OK (3.4s)
[3/5] Test.......... OK (5.1s)
[4/5] Coverage...... OK (95.5% >= 90%, 18.4s)
[5/5] Duplication... OK (<= 6%, 5.1s)
Validate OK (32.2s)
```

## Implementation

```
cargo xtask validate
```
