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

- **remote-clone-project-source** -- drop the push once nothing needs it
  The guest-clone half landed with `generate-vagrantfile`: the bootstrap clones
  `[source]` inside the VM and runs a script from it. What remains is the
  workstation half, and it is larger than the slug suggests.

  The goal is that neither the workstation nor the VM host reads any file from
  the project's repo -- not the source, and not `bombyx.toml` or `vagrant/`
  either. A repo bombyx has never opened cannot decide what runs on the
  machines outside the VM. See `docs/trust-boundary.md`.

  Two pieces, and only the second is this item. Moving the config out of the
  repo is captured separately as `project-config-off-repo`, because it is a
  design question rather than a deletion. What is left here is the push: once
  nothing needs the pushed files, `vagrant_dir`, `Action::pushes` and the
  `tar`/`scp` path all go, and `bombyx up` becomes four commands instead of
  seven.

  Ordering: this comes first, before `project-config-off-repo`. The VM host
  already receives an archive that no program there reads -- the generated
  Vagrantfile disables the `/vagrant` share and names only bombyx's own
  bootstrap script, so the unpacked files sit unread. `vagrant_dir` is the
  only config value naming a location inside the checkout, and the push is
  its only consumer. Removing the push first means the moved config never
  needs a path to a checkout.
  GitHub issue #10; planned in `docs/issues/project-config-off-repo.md`.

- **minimal-vagrantfile** -- strip project logic to boot + bootstrap hook
  Reduce the Vagrantfile to infrastructure only: provider, base box, CPUs,
  memory, and a single generic bootstrap provisioner that calls into bombyx.
  Everything project-specific moves out. A short Vagrantfile is also easier to
  keep identical across Windows and Linux hosts, since provider-specific
  features are what turn it into a nest of conditionals.

- **provision-lifecycle-hooks** -- named hooks replace one bash provision script
  Provisioning is currently one bash script run by Vagrant at VM creation.
  Replace it with named lifecycle hooks a project declares in a small manifest:
  prepare, dependencies, agent, cleanup. Vagrant then only creates the VM and
  runs the bootstrap; bombyx runs the hooks inside the guest. Simple projects
  implement one hook, complex ones several, and bombyx stays generic. It also
  decouples the hooks from Vagrant, so the backend can change later without
  touching them.

- **per-host-resource-profiles** -- detect host capacity, merge project minimums
  The same project run on a workstation and on a laptop should not get the same
  VM. Let the project declare its needs (minimum memory, minimum CPUs) and let
  each host contribute what it can provide. Detect RAM, CPU cores and hypervisor
  when a host is first used, apply a default policy such as half of RAM, and
  allow a per-host override file. Named profiles are the other half: a profile
  maps to a large allocation on the workstation and a smaller one on the laptop.

- **status-endpoint** -- read-only per-host VM status over the network
  There is no overview of what is running across machines. Each bombyx
  installation could expose a small read-only endpoint reporting its VMs, their
  projects, state and resource usage. Read-only keeps the autonomy of the
  current design: no central registry, no service to keep alive, no single point
  of failure. Report two distinct roles per VM, since they differ in this setup:
  the controller, meaning the instance that launched it, and the executor,
  meaning the host it actually runs on. Bind it to the private network only.

- **status-all-aggregator** -- bombyx status --all queries the known hosts
  The consumer of the per-host status endpoints. The client initiates: no
  background chatter, no instances polling each other. Discovery starts as a
  static config file listing the other hosts, which is dull and reliable; keep
  the lookup behind an interface so a tailnet or Consul provider can be added
  later without changing callers. The CLI is the first consumer, a dashboard is
  possible afterwards.

- **project-config-off-repo** -- bombyx.toml must not be read from the repo
  The second half of the boundary in `docs/trust-boundary.md`: neither the
  workstation nor the VM host may read any file from the project's repo.
  `bombyx.toml` is read from the working directory today, so it is the file
  blocking that.

  The decisions live in `docs/issues/project-config-off-repo.md`; this entry
  is a pointer at it. GitHub issue #16. It is chunk 2 of three:
  `remote-clone-project-source` comes first and `project-selection-flag`
  comes after.

  The cost is real and should be stated rather than discovered.
  `bombyx.toml` is committed today, so a teammate who clones gets the machine
  spec for free. Moving it out means every operator writes their own, and two
  of them can disagree about a VM's size without either file showing it. That
  trade is the point -- a repo bombyx has never opened cannot decide what runs
  outside the VM -- but it is a trade. It also inverts the README's Configure
  section, which explains the split as project-file-committed versus
  host-file-private. That framing does not survive.

- **newtype-remaining-config-fields** -- types for the six checked fields
  Six config values carry validation rules and are still bare `String` or `u32`:
  `host`, `project`, `vagrant_dir`, `remote_root`, `box` and `ref`, plus
  `cpus`/`memory` whose only rule is a floor. `RepoUrl`, `ScriptPath` and
  `ScratchName` show the shape. `Config`, `Vm` and `Source` all have public
  fields, so every one of those six can be set by hand with no check running.
  The checks live in `Config::validate`, `vm::validate` and `source::validate`,
  which only the loading path calls. `remote_root` is the one to do first: it
  reaches `rm -rf`, it has six rules, and `config::root` already holds all of
  them in one function, so the constructor wraps something that exists. Not for
  the generate-vagrantfile PR: the config modules have been re-cut in four
  commits over two days and the review flagged the churn.

- **project-selection-flag** -- `--project` names the project explicitly
  Chunk 3 of the project-config-off-repo work; GitHub issue #18. Depends on
  project-config-off-repo. Once a project's settings live in the operator's
  registry, bombyx has to be told which project a command is about. `--project
  <name>` becomes a required global argument for every VM subcommand. Inferring
  it from the working directory git remote was rejected: naming the project
  explicitly means bombyx reads nothing at all from the project directory.

## Done

- [**generate-vagrantfile**](issues/generate-vagrantfile.md)
  -- generate per provider from bombyx templates
  (2026-08-30)

- [**trust-boundary-doc**](issues/trust-boundary-doc.md)
  -- write down that the VM host is trusted
  (2026-08-30)

- [**crlf-staircase-on-windows**](issues/crlf-staircase-on-windows.md)
  -- output staircases on a Windows console
  (2026-08-18)

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

