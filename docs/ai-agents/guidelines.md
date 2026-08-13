# AI Agent Tools and Scripts Guidelines

Guidance for writing scripts and tools used by AI agents
(such as Claude Code). Following these guidelines speeds
up development and minimizes token costs.

## Core Principles

1. **Minimize output** -- every character costs tokens
2. **Be predictable** -- consistent formats reduce
   parsing errors
3. **Fail fast and clearly** -- actionable errors save
   retry cycles
4. **No interactivity** -- agents cannot respond to
   prompts

## Output Guidelines

### Keep Output Concise

```bash
# Bad -- verbose output
echo "Starting the build process..."
echo "Compiling source files..."
echo "Build completed successfully!"

# Good -- minimal output
echo "Build OK"
```

### Reserve Verbose Output for Failures

Only emit detailed information when something goes
wrong:

```
# Success (1 line)
Test OK: 104 passed

# Failure (actionable details only)
FAILED: 2 of 104 tests failed

  inventory::tests::rejects_duplicate FAILED
    assertion `left == right` failed
    at crates/bombyx/src/lib.rs:45
```

### Stepwise Progress for Multi-Step Operations

Use the `[N/Total]` format with dot padding and timing:

```
[1/5] Fmt........... OK (0.2s)
[2/5] Clippy........ OK (3.4s)
[3/5] Test.......... OK (5.1s)
[4/5] Coverage...... OK (95.5% >= 90%, 18.4s)
[5/5] Duplication... OK (<= 6%, 5.1s)
Validate OK (32.2s)
```

## Exit Codes

Always use meaningful exit codes:

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General failure |
| 2 | Hook: block action with error message |

## Script Design

### No Interactive Prompts

Never use `read` or confirmation prompts:

```bash
# Bad -- blocks agent
read -p "Continue? (y/n)" confirm

# Good -- use flags with defaults
if [ -z "$FORCE" ] && [ -f "$target" ]; then
    echo "FAILED: Target exists. Use FORCE=1"
    exit 1
fi
```

### Idempotent Operations

Scripts should be safe to run multiple times:

```bash
# Bad -- fails if already stopped
kill $PID

# Good -- handles already-stopped case
kill $PID 2>/dev/null || true
```

### Fail Early

Check preconditions at the start:

```bash
[ -f "Cargo.toml" ] || {
    echo "FAILED: Not in project root"; exit 1
}
```

## Available Commands

| Command | Description |
|---------|-------------|
| `/check` | Type-check all targets, running none |
| `/test` | Run tests with minimal output |
| `/validate` | Full quality pipeline |
| `/commit` | Git commit with project conventions |
| `/todo` | Process pending TODO items |

## Hooks

Claude Code hooks allow automatic actions at specific
points in the workflow.

### Hook Types

| Type | When | Use Case |
|------|------|----------|
| `PreToolUse` | Before tool | Validate, fail fast |
| `PostToolUse` | After tool | Summarize, log |
| `Stop` | Before stop | Quality gates |

### Hook Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Allow (success) |
| 2 | Block with error message |

### Existing Hooks

- **Stop hook** (`.claude/hooks/stop-check.sh`): runs
  `cargo xtask validate` when Rust files have been
  modified, blocking stop if checks fail.

## Checklist for New Scripts

- [ ] Minimal output on success (ideally one line)
- [ ] Detailed output on failure
- [ ] Meaningful exit code
- [ ] No interactive prompts
- [ ] Idempotent (safe to re-run)
- [ ] Fails fast on missing preconditions
- [ ] Works from project root directory
