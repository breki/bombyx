#!/usr/bin/env bash
# Stop hook: runs a fast-path subset of validate when
# Rust files have been modified -- fmt-check + clippy
# + tests.
#
# Coverage and duplication are intentionally skipped:
# coverage alone adds ~15s per invocation on a small
# codebase, and the Stop hook fires often enough during
# interactive work that the cost compounds. Full
# `cargo xtask validate` still runs from /commit and
# is available manually for explicit pre-flight checks.
#
# fmt-check is included (~0.2s) because /commit only
# runs full validate for version-bumping commits;
# chore / docs / refactor / test commits skip validate
# entirely, so a fmt drift can otherwise slip through
# both interactive gates and land in CI as a fmt-check
# failure (see commit 9d7b3ff for a worked example).
#
# Exit codes:
#   0 -- all checks passed (or nothing to check)
#   2 -- a check failed; stderr carries the failure output
#       so Claude can fix the issues before stopping

set -euo pipefail

# --- Guard against infinite loops -------------------------
# If Claude is already fixing issues from a previous hook
# run, skip re-checking to avoid fix-fail cycles.
input="$(cat)"
if echo "$input" | grep -q '"stop_hook_active"'; then
  exit 0
fi

# --- Detect modified Rust files ---------------------------
changed_rs=$(
  {
    git diff --name-only --diff-filter=ACMR HEAD -- '*.rs' 2>/dev/null
    git diff --name-only --diff-filter=ACMR -- '*.rs' 2>/dev/null
    git ls-files --others --exclude-standard -- '*.rs' 2>/dev/null
  } | sort -u
)

if [ -z "$changed_rs" ]; then
  exit 0
fi

# --- Run checks ------------------------------------------
# Stages run sequentially. A `cmd1 && cmd2 && cmd3`
# chain captured into a single `$output` would, on
# multi-stage failures, only contain the first failing
# stage's output AND drop the label of which stage
# failed -- so when Claude fixed clippy the test
# failure would resurface looking "new" on the next
# hook run, defeating the `stop_hook_active` guard
# above.
run_stage() {
  local label="$1"
  shift
  local stage_output
  if ! stage_output=$("$@" 2>&1); then
    printf '=== %s failed ===\n%s\n' "$label" "$stage_output" >&2
    return 1
  fi
  return 0
}

run_stage "cargo fmt --check" cargo fmt --all -- --check \
  && run_stage "cargo xtask clippy" cargo xtask clippy \
  && run_stage "cargo xtask test" cargo xtask test \
  || exit 2
