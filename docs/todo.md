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

- **phantom-deploy-command** -- cargo xtask deploy is documented but missing
  CLAUDE.md and .claude/commands/release.md and implement.md all describe cargo
  xtask deploy as the thing that gates shipping (it refuses unless HEAD is on a
  matching annotated tag), but no deploy subcommand exists in xtask -- it went
  with the template's deploy subsystem during the CLI-only prune. .gitignore
  still carries .deploy for the same reason. Either implement it or strip the
  references; leaving canon describing a gate that cannot run is worse than
  either.

- **doctor-preflight** -- bombyx doctor: preflight the host before a push
  Add a doctor subcommand that verifies bombyx's own preconditions and reports
  each as a pass/fail line, without stopping at the first failure. Probes: ssh
  <host> true (alias resolves, key auth works non-interactively); command -v
  vagrant over NON-interactive ssh, which is the one nothing else can report --
  vagrant cannot tell you it is invisible to a non-login shell, and today that
  surfaces mid-push as a bare command not found after a directory and tarball
  have already been shipped; command -v tar on the host; tar present locally;
  remote_root exists or is creatable; and a purely local check that vagrant_dir
  exists and holds a Vagrantfile, which catches a typo'd bombyx.toml instantly.
  Scope OUT host provisioning: VT-x, /dev/kvm, RAM, disk, libvirtd and the
  vagrant-libvirt plugin are the VM host's business, not a thin SSH wrapper's.
  Scope OUT remediation text: printing apt install hints needs a per-distro
  database that rots (Ubuntu 24.04 dropped vagrant from its archive, others did
  not) -- report the fact precisely and let the operator decide. Design note:
  this does not fit the existing executor. Every current action runs commands,
  stops at the first failure and propagates the exit code, so execute() only
  inspects status; doctor needs .output() per probe and must run all of them.
  Suggested shape, matching the existing pure-construction split: remote.rs
  builds the probe commands, a new doctor.rs owns a pure probe-results-to-report
  function (the branchy part, so the tested part), main.rs runs and prints.
  Roughly 150-200 lines with tests. Sequencing: implement AFTER first-real-run
  -- driving the sequence by hand once more will likely surface probes not yet
  thought of, and building the doctor first risks shipping an incomplete one.

- **discard-leaves-dir** -- discard destroys the VM but leaves its directory
  bombyx discard runs vagrant destroy -f, which removes the domain and the
  .vagrant machine folder, but leaves the scratch directory itself on the host.
  After discard pr-1234, ~/vms/scratch/vmtest/pr-1234/ still holds the pushed
  Vagrantfile and a .vagrant skeleton. Verified live on frosti during
  first-real-run. This matters because README.md says of scratch VMs: 'Nothing
  survives, which is the point: malware that persists to survive credential
  rotation has nothing to persist to.' That claim is now inaccurate. The
  leftover is not itself a security hole -- the VM disk is gone and the
  directory only holds a Vagrantfile bombyx pushed there -- but directories
  accumulate one per discarded VM, and the README overstates what discard
  guarantees. Decide between two fixes: have discard remove the directory after
  a successful destroy (rm -rf on a path built from a validated ScratchName, so
  the traversal guard is load-bearing), or soften the README claim to match what
  the code does. Prefer the first; the point of scratch is that it leaves
  nothing.

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

- **destroy-project-vm** -- no way to destroy a persistent project VM
  bombyx can create a persistent project VM with up but cannot remove one. The
  ephemeral lifecycle is symmetric (scratch creates, discard destroys); the
  persistent one is not: up creates, down only halts, and reset restores a
  snapshot. Removing a project VM today means abandoning bombyx and running
  vagrant destroy -f over ssh by hand, which is what happened when tearing down
  the first-real-run test VM. That contradicts the README's premise that you
  stay on your workstation. Implementation is small and fits the existing shape:
  an Action::Destroy mapping to vagrant_in(cfg, and cfg.remote_project_dir(),
  and [destroy, -f]) -- the same construction discard already uses for scratch.
  Design decisions, both deliberate: (1) It must be harder to type than discard,
  because the two differ in consequence -- a scratch VM is disposable by
  definition, a project VM holds warm caches and installed tooling, which is the
  whole reason the persistent lifecycle exists. Proposal: require the project
  name as a confirmation argument, so bombyx destroy phren proceeds only when
  phren matches project in bombyx.toml, and a bare bombyx destroy refuses and
  names what to type. That reuses the existing validation, adds no interactive
  prompt (bombyx has none anywhere today, and --dry-run is its review
  mechanism), and mirrors the type-the-name guard GitHub uses. (2) Whether
  destroy also removes the host directory is the same question as
  discard-leaves-dir and must be answered identically for both. Do the two items
  together.

## Done

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

