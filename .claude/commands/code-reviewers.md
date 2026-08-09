# Code reviewers (gating rules)

Two adversarial reviewers guard every code commit. Their
personas now live as first-class subagents in
`.claude/agents/`, so their read-only nature is a harness
guarantee (they have no `Edit`/`Write` tools), not just an
instruction:

- **`red-team`** (`.claude/agents/red-team.md`) -- security &
  correctness. Tools: `Read, Grep, Glob, Bash`. It runs
  `git diff --cached` and `git log` itself, so it needs the
  shell.
- **`artisan`** (`.claude/agents/artisan.md`) -- code quality &
  craftsmanship beyond clippy. Tools: `Read, Grep, Glob` only --
  no shell, so it is read-only by construction. Pass the diff to
  it in the spawn prompt (capture `git diff --cached` once in
  the calling skill and hand it over).

This file is the shared **gating** doc for both `/commit`
(step 3) and `/implement` (Phase 3 pre-launch); it defines
*which* reviewers run, *when*, and *how* to spawn them. The
review criteria themselves live in the agent files above.

## When to run

Run **both** reviewers whenever the diff contains code changes:
Rust (`.rs`, `.toml`), frontend (`.svelte`, `.js`, `.ts`,
`.css`), config (`playwright.config.ts`, `vite.config.js`,
`vitest.config.js`, ...), or deployment / infrastructure files
(`.service`, `Dockerfile`, `docker-compose.yml`, `.conf`,
`.nginx`, `.env.example`, ...). Never skip them, even for
"straightforward" changes. The only exception is a commit with
no code at all (docs-only markdown / `.md` files).

## How to spawn

Spawn both in a **single parallel message** -- one `Agent` call
per reviewer -- so they run concurrently:

- `subagent_type: red-team`
- `subagent_type: artisan`

Give each spawn:

1. A one-line description of what the change does.
2. For `artisan`, the captured `git diff --cached` output (it
   has no shell). `red-team` runs the diff itself.
3. The instruction to report each finding with the six labeled
   bullet fields: **ID**, **Source**, **Category**,
   **Description**, **Impact / Why it matters**, **Suggested
   fix**.

**Diff handoff.** Never write the diff to `/tmp` (on Windows +
Git Bash that resolves outside the workspace and is invisible to
the user). `red-team` reads it via `git diff --cached`;
`artisan` receives it inline. If a file is genuinely needed, use
a git-ignored path under `target/`.

Each agent's final message *is* its report (a plain-text finding
list, or "No issues found."), consumed by the calling skill --
not shown to the user directly.
