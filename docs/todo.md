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

- `wire-frosti` -- frosti is on WiFi, and VLAN tagging needs it wired.
  Prerequisite for the agent VLAN.
- `packer-box` -- bake a base box so `scratch` boots fast enough to use.
- `agent-vlan` -- isolate VMs on a VLAN with an egress allowlist.
  Enforced at the router.

- **reset-needs-snapshot** -- reset depends on a snapshot nothing creates
  bombyx reset runs vagrant snapshot restore fresh-install, but no bombyx
  command ever creates that snapshot. On a freshly booted VM the command fails
  with 'The snapshot name fresh-install was not found for the virtual machine
  default' and exit 1. The failure is clean and well surfaced -- verified live
  on frosti during first-real-run -- so this is a workflow gap rather than a
  bug. README.md documents reset as 'restore the fresh-install snapshot' without
  saying the operator must create it by hand first, which makes reset look like
  a working command on a new project when it cannot be. Options: add a bombyx
  command that takes the snapshot (snapshot save fresh-install) so the reset
  cycle is self-contained, or document that taking it is a manual step after the
  first successful up. The first is more useful: the snapshot should be taken at
  a known-good point, which is exactly after up completes.

- **done-links-may-dangle** -- todo done can write a dangling issue link
  cargo xtask todo done always renders the Done entry as
  [**slug**](issues/slug.md), whether or not that file exists. Items completed
  through /implement have a doc so the link resolves; anything closed outside
  that flow gets a dead link. Seen twice: first-real-run was hand-written into
  Done without a link precisely to dodge this, and todo-tooling-format-mismatch
  got an issue doc partly so the generated link would resolve. Options: omit the
  link when docs/issues/<slug>.md is absent, which needs an existence check
  passed in from the caller since move_to_done is otherwise pure over the
  markdown; or keep it unconditional and have /implement guarantee the doc
  exists before finalising. The second matches current intent but is enforced
  nowhere.

- **host-network-isolation** -- apply and verify the nftables rules on frosti
  docs/vm-host-setup.md now documents an nftables ruleset that keeps agent VMs
  off the home LAN, the tailnet, Docker and the VM host's own services, while
  leaving outbound internet working. It is marked unverified: the rules were
  derived from frosti's actual network layout (virbr1 on 192.168.121.0/24, host
  on 192.168.1.10 via wlp4s0, plus tailscale0 and docker0) but have not been
  applied, because sudo on frosti needs a password and cannot run from a bombyx
  session. Run `agent-vm-firewall apply`, then the in-VM verification snippet
  including the IPv6 check, then `persist` -- and reboot and run `status`, since
  persistence is the one part that cannot be confirmed any other way and fails
  silently. Only then drop the unverified marker from the heading. Watch for two
  things the review flagged as untested in both directions: whether the input
  drop breaks anything the guest starts against the host, and whether `nft -c`
  accepts the generated ruleset on this nft version. This is a host-level
  stopgap for agent-vlan, not a replacement: enforcement sits on the machine
  being protected.

- **suspend-resume-commands** -- save and restore VM RAM state mid-task
  Add `bombyx suspend` / `bombyx resume` subcommands wrapping `vagrant suspend`
  / `vagrant resume`, so a VM's RAM state can be saved and restored mid-task.
  Context: `down` maps to `vagrant halt` (plan.rs:90), which is a graceful
  power-off -- the disk survives but running processes, tmux sessions and
  listening ports do not. There is currently no way to pick up mid-task after
  stopping a VM.

## Done

- [**phantom-deploy-command**](issues/phantom-deploy-command.md)
  -- stripped the references; bombyx has no deploy step
  (2026-08-12)

- [**provision-command**](issues/provision-command.md)
  -- re-run provisioning on a running VM
  (2026-08-10)

- [**doctor-preflight**](issues/doctor-preflight.md)
  -- bombyx doctor: read-only preflight checks
  (2026-08-10)

- [**discard-leaves-dir**](issues/discard-leaves-dir.md)
  -- discard now removes the scratch directory too
  (2026-08-10)

- [**destroy-project-vm**](issues/destroy-project-vm.md)
  -- destroy the project VM and remove its directory
  (2026-08-10)

- [**todo-tooling-format-mismatch**](issues/todo-tooling-format-mismatch.md)
  -- todo list and done now read backticked entries too
  (2026-08-10)

- **first-real-run** -- drove bombyx against a real libvirt host
  (frosti, Ubuntu 24.04, Vagrant 2.4.9, vagrant-libvirt 0.12.2).
  Full sequence exercised: `up`, `status`, `shell`, `down`,
  `scratch`, `discard`, `reset`, plus a second `up` for
  idempotency and a live traversal rejection. Confirmed the
  tilde fix, the tar-push (no nesting, `.vagrant` preserved,
  archive cleaned up) and project-scoped scratch dirs against
  reality. Turned up `discard-leaves-dir` and
  `reset-needs-snapshot`. (2026-08-10)
- [**drop-frontend-tooling**](issues/drop-frontend-tooling.md)
  -- Delete leftover frontend tooling from the CLI-only prune
  (2026-08-09)

