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

## Done
