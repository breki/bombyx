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
  bug. README.md and docs/tutorial.md both now say the snapshot has to be
  taken by hand first, so what is left is the command itself. Options: add a
  bombyx
  command that takes the snapshot (snapshot save fresh-install) so the reset
  cycle is self-contained, or document that taking it is a manual step after the
  first successful up. The first is more useful: the snapshot should be taken at
  a known-good point, which is exactly after up completes.

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
  Context: `Action::Down` maps to `vagrant halt`, which is a graceful
  power-off -- the disk survives but running processes, tmux sessions and
  listening ports do not. There is currently no way to pick up mid-task after
  stopping a VM.

- **minimal-vagrantfile** -- keep the generated file identical across hosts
  The first half of this landed with `generate-vagrantfile`: `vagrantfile.rs`
  renders infrastructure only -- box, provider block with cpus and memory, the
  disabled synced folder, and one shell provisioner pointing at bootstrap.sh.
  Nothing project-specific reaches it.

  What is left is the parity claim. The renderer emits one provider block
  chosen by `[vm] provider`, and only the libvirt spelling has ever been run.
  Whether the Hyper-V block boots anything, and whether the two need to
  diverge further than they do, is unanswered until somebody has a Windows VM
  host. Related: `provider-configured-not-selected`.

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

- **newtype-remaining-config-fields** -- types for the five checked fields
  Five config values carry validation rules and are still bare `String` or
  `u32`: `host`, `project`, `remote_root`, `box` and `ref`, plus
  `cpus`/`memory` whose only rule is a floor. `RepoUrl`, `ScriptPath` and
  `ScratchName` show the shape. `Config`, `Vm` and `Source` all have public
  fields, so every one of those five can be set by hand with no check running.
  The checks for `project` and `remote_root` live in `Config::validate`, and
  those for `box`, `ref`, `cpus` and `memory` in `vm::validate` and
  `source::validate`, all of which only the loading path calls. `host` is the
  exception, and its rule moved to `config::registry::parse` -- see below.
  `remote_root` is the one to do first: it reaches `rm -rf`, it has six rules,
  and `config::root` already holds all of them in one function, so the
  constructor wraps something that exists. Not for the generate-vagrantfile
  PR: the config modules have been re-cut in four commits over two days and
  the review flagged the churn.

  `host` belongs beside `remote_root` at the front of the queue. Its check has
  been placed twice in two weeks and argued about three times: `b534bb1` left
  the value to the ranking and wrote down why, `59dd110` put a check in
  `Project::validate` after a review round on #25, and the registry-config-load
  review moved it again into `config::registry::parse`. Each move reversed the
  reasoning written down for the one before it, which is the sign of a missing
  type rather than a missing decision -- a `HostName` cannot be placed in the
  wrong function.

- **self-update-resolves-tar-late** -- two downloads before it notices no tar
  Found by the red-team review of 92c2e74 (RT-7), verified by reading the call
  sites rather than by running it. bombyx resolves every program a plan needs up
  front, so a missing binary stops the run before anything changes state. That
  covers VM plans, which are a single program throughout. It does not cover
  self-update: ran_ok calls execute with one command at a time, and self_update
  calls ran_ok three separate times. So tar is resolved at the extraction step,
  after curl has already fetched the checksums and the archive. On a machine
  with git and curl but no tar, bombyx self-update does two network round trips
  and then fails. The comment above the resolution loop now says plainly that
  the loop does not cover self-update, so the code and the prose agree; closing
  the gap means resolving git, curl and tar before the first fetch. Worth fixing
  in the same pass: git is the first program self-update runs, and it was
  missing from the lists that name its tools.

- **provider-configured-not-selected** -- vagrant picks the provider, not bombyx
  Found by the red-team review of 58099a8 (RT-2), verified by reading
  vagrantfile.rs and plan.rs. The generated Vagrantfile emits
  `config.vm.provider :<name> do |v|`, which configures a provider if vagrant
  chooses it. It does not choose it. No plan passes `--provider`, and bombyx
  sets no VAGRANT_DEFAULT_PROVIDER, so vagrant picks whatever the host makes
  available. On a Linux VM host with only vagrant-libvirt installed, a project
  with provider = "hyperv" boots a libvirt machine. The `:hyperv` settings block
  never applies, so the cpus and memory in the config are ignored and the VM
  comes up at vagrant defaults. Nothing reports the mismatch. Until 58099a8 the
  libvirt plugin probe caught this by accident: a hyperv project got a red
  doctor row, for the wrong reason. That probe is now conditional and a hyperv
  project gets a skip row instead, so the accident is gone and the defect is
  visible. The fix is to pass the provider to vagrant rather than only
  describing it -- `vagrant up --provider <name>`, or VAGRANT_DEFAULT_PROVIDER
  in the command bombyx already builds. Then `doctor` should check that the host
  can supply the provider the project asks for, which is the honest version of
  the probe that was removed.

  Two halves, and only one is blocked. Passing the provider to vagrant is
  verifiable on the libvirt host we already use: `vagrant up --provider hyperv`
  on a Linux host should refuse instead of substituting *(unverified -- run it
  once on frosti and record the output here)*, which is the point -- a loud
  failure rather than a wrong VM. Only making Hyper-V actually work needs a
  Windows VM host, and Provider::Hyperv is documented as never exercised.

- **validate-resume-from-step** -- let validate resume at the gate that failed
  `cargo xtask validate` prints `-> iterate with: cargo xtask <cmd>` on a
  failure, and CLAUDE.md records that ignoring that hint four times in one
  sitting is what prompted the rule to re-run the step rather than the pipeline.
  It happened again while canon-check was being built: three full ten-gate runs,
  roughly 15s each, where fmt, clippy and a test had each failed alone.
  Re-running one gate is a different command from re-running the pipeline, which
  is the friction. A `--from <step>` flag would make the resume as cheap to type
  as the restart, and the step names already exist in `validate.rs`'s step list.
  Raised by the workflow retrospective, 2026-09-03.

- **destroy-confirmation-shape** -- what destroy's positional becomes
  Step 7 of 7; GitHub issue #27. Depends on project-selection-flag. One design
  question, undecided.

- **config-tests-own-file** -- config.rs tests into config/tests.rs
  `mod tests` is most of config.rs -- by a wide margin -- and that is what makes
  the file unreadable in one pass.
  No exact figure here: it moves every commit and a stale one costs a reader a
  check. CLAUDE.md records
  that reading it whole overflowed a session. Move it with `#[cfg(test)] #[path
  = "config/tests.rs"] mod tests;`. Raised by artisan during the /review2 on #23
  and kept out of that change as unrelated scope.

- **config-home-env-provenance** -- say when the environment picked the config
  A repo-set BOMBYX_CONFIG_HOME redirects bombyx to another config.toml, and the
  winning origin is then HostOrigin::UserFile, which main.rs deliberately stays
  silent about -- so bombyx runs against a host the operator never configured
  and prints nothing. Demonstrated live during the /review2 on #23: an anchored
  value such as /tmp/pwn passes is_anchored_dir, and a per-directory environment
  tool (direnv reading an .envrc in a clone, mise, a CI job) can set it.
  This was captured as two halves and one of them has landed. #18 passes the
  registry path into HostOrigin::describe from main.rs, so a notice that prints
  names the file bombyx read rather than a bare config.toml, and the Display
  impl that rendered the bare name is deleted. What remains is the printing
  half: print the provenance line for UserFile too whenever CONFIG_DIR_ENV
  supplied the directory, which needs a failing test first.
  Raised as red-team finding RT-1; the prose claiming otherwise was corrected in
  that change, the code was not. The second half was added during the /review2
  on #25, where a doc comment cited this item as the work that gives the notice
  the path and the item did not yet say so; it landed in #18, which the /review2
  there found still described as pending.

- **add-issue-flag-unused** -- todo add --issue has no caller
  The flag renders a pending entry as a link to issues/<slug>.md, derived rather
  than given, so it can write a dead link the way todo done used to. Nothing
  calls it: no file under .claude/ mentions it, and /todo documents only --slug,
  --summary and --body. Guarding it the way done --doc is now guarded would be
  wrong, because at capture time the spec may legitimately not exist yet. So the
  question is whether the flag should take a path like done --doc does, or be
  deleted. Found while fixing #7, and kept out of it: that issue is about done,
  and this flag has no caller to endanger.

- **done-drops-the-body-silently** -- say that todo done discards the body
  move_to_done splices the whole pending block away and re-emits only the slug,
  summary and date, so an entry's analysis body is dropped without a word. That
  is intended -- every entry under ## Done is two or three lines -- but nothing
  says so, and 'block spliced, content not carried' is exactly the shape of the
  summary-loss bug the /review2 on #7 found. Completing done-links-may-dangle
  and todo-done-link on 2026-09-04 dropped a dozen lines of analysis each; the
  content survives in git history and in docs/developer/template-feedback.md.
  Either say it in move_to_done's doc comment and the Done clap help, or carry
  the body across. Found while using the fixed tool for the first time.

- **readme-vagrantfile-pointer** -- README promises what Part 3 deletes
  Found by /review2 (fresh-reader FR-11) while working local-host-execution
  (#38); pre-existing and out of that issue's scope. README.md points at
  docs/tutorial.md for "a sample project with a Vagrantfile and a provisioning
  script", but tutorial.md Part 3 is headed "The Vagrantfile: bombyx writes it"
  and ends by telling the reader a committed one is read by nothing and should
  be deleted. The README pointer describes what Part 3 produced before bombyx
  generated the file. Fix is in README.md: list what Part 3 actually produces, a
  provisioning script and a project table.

- **comments-narrating-the-past** -- Eight comments tell history, not reasons
  Found by /review2 (fresh-reader FR-12) while working local-host-execution
  (#38); all pre-existing and out of that issue's scope. CLAUDE.md under Code
  comments forbids narrating the past outright. The eight: remote/probe.rs
  "which is exactly how an earlier version of this probe passed on a fish
  shell"; remote.rs "and when it built its own string it silently ran vagrant
  with neither variable set"; remote.rs "It was the sibling left out when Tty
  was introduced"; doctor/probes.rs "Hardcoding the reason meant renaming the
  gating probe left the report explaining the skip in terms of a column that no
  longer existed"; integration_test.rs "Both names bombyx once used are tried";
  a remote.rs test "so it once assembled its own string and ran vagrant with
  neither variable set"; architecture.md "Both of these were code comments
  once"; architecture.md's "It took three review rounds to find all three
  copies" paragraph. The reviewer notes each already contains its own
  present-tense form, so the rewrite is mechanical rather than a judgement call
  about what the comment is for. The reviewer's own example: "the skip reason is
  read from the gate, so renaming a report column cannot leave the report naming
  a column that does not exist".

- **vm-disk-size-unset** -- no disk key, so the guest gets the box's own size
  Found by registry-run-against-frosti (#37), driving the CLI against frosti.
  The generated Vagrantfile's provider block carries cpus and memory only, and
  no disk setting appears anywhere in the template
  (crates/bombyx/src/vagrantfile.rs, render). There is no disk key in
  config.toml, so the guest inherits the box's own partitioning. That is a wide
  range in practice. cloud-image/debian-13 gave the guest a 9.7 GB root;
  generic/ubuntu2204 gave a 128 GiB disk whose root logical volume is 63 GB,
  with another 63 GB unallocated in the volume group. kozmotic's own
  hand-written Vagrantfile carries DISK_GB = 30 with the comment that the box
  default of about 10 GB is too small for a Rust target directory plus two
  cargo-installed tools and a coverage run, so a project that needs a size has
  no way to ask bombyx for one. Options: add an optional disk key under the vm
  table that the Vagrantfile writes as the provider's disk setting, or state in
  config.toml.sample that the box's own disk is what you get and that choosing
  the box is how you choose the size.

- **box-must-carry-git** -- the git requirement surfaces after the boot
  Found by registry-run-against-frosti (#37). bootstrap.sh refuses when git is
  absent, with a clear message naming the fix: install it in the box, or choose
  one with git. The refusal itself is right. What costs time is when it arrives.
  It runs inside the guest as the first provisioner, so the operator has already
  downloaded the box, created the domain and waited for the boot before learning
  the box cannot work. The two boxes already on frosti when the run started were
  cloud-image/debian-13 and cloud-image/ubuntu-24.04. Only the first was booted,
  and it had no git; the second was not tested, so treat "cloud images tend not
  to ship git" as the expectation it is rather than as a measurement. Neither
  config.toml.sample nor doctor mentions the requirement, and the sample's own
  box value, generic/ubuntu2204, does carry git, so a reader following the
  sample never meets the problem and never learns the rule. Options: say it in
  config.toml.sample next to the box key, which costs nothing and is honest
  about being unenforced; or have doctor check the box, which it cannot do
  without booting something and so probably does not belong there.

- **scratch-domain-name-collides** -- one libvirt domain for two scratches
  Found by registry-run-against-frosti (#37). config.toml.sample claims that
  scratch VMs land in remote_root/scratch/project/name, so the same scratch name
  in two projects cannot collide. The directories indeed cannot. The libvirt
  domain names can. vagrant-libvirt builds the domain name as the basename of
  the directory holding the Vagrantfile, an underscore, and the Vagrant machine
  name -- which bombyx never sets, so it is Vagrant's default, `default`. Three
  domains on frosti follow that rule: ~/vms/jutro gave jutro_default,
  ~/vms/vmtest gave vmtest_default, and
  ~/vms/scratch/vmtest/probe gave probe_default. The project name is nowhere in
  the last one, so a probe scratch in a second project would ask libvirt for
  probe_default as well. The collision itself was not booted, so treat the
  mechanism as evidenced by three domains and not as demonstrated. The claim in
  the sample is what needs settling either way: either the domain name gains the
  project, or the sample stops promising more than the directory layout
  delivers.

- **tutorial-box-lacks-git** -- two passages still assume the Debian box
  Found while working box-must-carry-git, and verified by booting it on frosti
  on 2026-09-05: debian/bookworm64, which docs/tutorial.md used to tell the
  reader to use, has no git, so bootstrap.sh refuses and the first up exits 1
  after the download and the boot. The table in Part 3 now names
  generic/ubuntu2204, whose guest booted and provisioned to completion the same
  day, and the paragraphs under it explain the failure and warn readers off the
  Debian box. What is left is the two passages written around that box, which
  still mention it: Part 3's provision.sh runs chsh because the Debian box gives
  its user /bin/sh, and When something goes wrong explains the arrow-key
  behaviour the same box causes. Both are currently handled by telling the
  reader they will not apply, which is an explanation where a rewrite belongs.
  Doing this properly means running the tutorial end to end, which has never
  happened -- its header already marks the Part 3 and Part 4 transcripts
  unverified.

## Done

- [**tutorial-local-route-now-booted**](issues/registry-run-against-frosti.md)
  -- a guest has booted on the local route
  (2026-09-05)

- [**registry-run-against-frosti**](issues/registry-run-against-frosti.md)
  -- drive the new CLI against a real VM host
  (2026-09-05)

- **local-host-execution** -- run vagrant directly when this machine is the host
  (2026-09-05)

- [**project-selection-flag**](issues/project-config-off-repo.md)
  -- `--project` names the project explicitly
  (2026-09-05)

- [**registry-config-load**](issues/project-config-off-repo.md)
  -- load a Config from the registry by name
  (2026-09-05)

- [**registry-project-host**](issues/project-config-off-repo.md)
  -- an optional host key per project
  (2026-09-04)

- [**registry-projects-table**](issues/project-config-off-repo.md)
  -- config.toml gains a projects table
  (2026-09-04)

- **todo-done-link** -- todo done writes a link that need not resolve
  (2026-09-04)

- **done-links-may-dangle** -- todo done can write a dangling issue link
  (2026-09-04)

- [**overlay-drop-host-source**](issues/project-config-off-repo.md)
  -- delete bombyx.local.toml entirely
  (2026-09-04)

- **canon-xref-wrapped-bold** -- canon-check reads paragraphs
  (2026-09-04)

- [**overlay-drop-project-overrides**](issues/project-config-off-repo.md)
  -- the overlay carries a host and nothing else
  (2026-09-04)

- [**remote-clone-project-source**](issues/project-config-off-repo.md)
  -- dropped the push; no program read the archive
  (2026-09-02)

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

