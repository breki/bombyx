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

- **todo-tooling-format-mismatch** -- todo list and done ignore
  backticked entries, and done truncates wrapped summaries
  Two defects in `cargo xtask todo`, one root cause: the reader and the
  writer disagree about the bullet format.

  1. `todo add` writes `- **slug** -- summary`, but the four original
     hand-written entries use `` - `slug` -- summary ``. The parser only
     recognises the bold form, so `todo list` silently omitted
     `wire-frosti`, `first-real-run`, `packer-box` and `agent-vlan` from
     the moment they were written, and `todo done first-real-run` fails
     with "no pending todo with slug". A listing that quietly drops
     entries is worse than one that errors: it was reported as the
     complete list several times. Accept both spellings when reading,
     and normalise on write.
  2. `todo add` wraps a long summary across lines, but `done` carries
     only the first line into the Done entry, cutting it mid-sentence.
     See the `drop-frontend-tooling` entry under Done, which reads
     "Delete leftover frontend tooling from the". Rejoin continuation
     lines when reading a pending bullet. Note `--summary` cannot repair
     an entry after the fact, because `done` refuses a slug that is no
     longer pending.

  Both need unit tests over fixture markdown covering each bullet
  spelling. Until this is fixed, `docs/todo.md` cannot be trusted as a
  work queue, which makes it the highest-priority item here.

- **phantom-deploy-command** -- cargo xtask deploy is documented but does not
  exist
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

## Done

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

