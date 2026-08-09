# TODO

Project work queue.

- `/todo <text>` captures a new item with a generated slug.
- `/todo` (no arguments) lists pending slugs.
- `/implement <slug>` plans and implements a pending item.
- `/implement` (no arguments) lists pending items and asks
  which to act on.

Each implemented item gets a planning doc at
`docs/issues/<slug>.md` that captures the problem statement,
plan, decisions, and outcome.

## Pending

- `wire-frosti` -- frosti is on WiFi; VLAN tagging needs
  it wired. Prerequisite for the agent VLAN.
- `first-real-run` -- drive bombyx against a real
  libvirt host; --dry-run only proves the argv.
- `packer-box` -- bake a base box so `scratch` is fast
  enough to actually get used.
- `agent-vlan` -- move VMs onto an isolated VLAN with a
  router-enforced egress allowlist.

- **todo-summary-truncation** -- todo done truncates a wrapped summary
  cargo xtask todo add wraps a long summary across several lines in the Pending
  bullet, but todo done only carries the first line into the Done entry, so the
  text is cut mid-sentence. Seen on drop-frontend-tooling, whose Done entry
  reads 'Delete leftover frontend tooling from the'. Fix in xtask with a test:
  rejoin the wrapped continuation lines when reading a pending bullet. The
  --summary override cannot repair an entry after the fact, since done refuses a
  slug that is no longer pending.

- **phantom-deploy-command** -- cargo xtask deploy is documented but does not
  exist
  CLAUDE.md and .claude/commands/release.md and implement.md all describe cargo
  xtask deploy as the thing that gates shipping (it refuses unless HEAD is on a
  matching annotated tag), but no deploy subcommand exists in xtask -- it went
  with the template's deploy subsystem during the CLI-only prune. .gitignore
  still carries .deploy for the same reason. Either implement it or strip the
  references; leaving canon describing a gate that cannot run is worse than
  either.

## Done

- [**drop-frontend-tooling**](issues/drop-frontend-tooling.md)
  -- Delete leftover frontend tooling from the
  (2026-08-09)

