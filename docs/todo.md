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
  host. `provider-configured-not-selected` closed the other half of this: the
  renderer's provider block is now the provider vagrant is actually told to
  use. What stays open here is `doctor-checks-hyperv-support`, and the Hyper-V
  block itself, which has never booted anything.

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

- **config-tests-own-file** -- config.rs and registry.rs tests into their own
  files
  `mod tests` is most of config.rs -- by a wide margin -- and that is what makes
  the file unreadable in one pass.
  No exact figure here: it moves every commit and a stale one costs a reader a
  check. CLAUDE.md records
  that reading it whole overflowed a session. Move it with `#[cfg(test)] #[path
  = "config/tests.rs"] mod tests;`. Raised by artisan during the /review2 on #23
  and kept out of that change as unrelated scope.
  Carry the module-scope fixtures out with the tests: TABLE_FIELDS,
  required_tables, test_entry, test_entry_with and test_registry all sit above
  the pub use block, because two sibling test modules share them, so a reader
  scanning the top of config.rs for its public surface hits test scaffolding
  first. Raised by red-team during the /review2 on #26.
  config/registry.rs wants the same move and should land with it: it is around
  900 lines, roughly 440 of them mod tests, and after #18 it is the file that
  owns the whole config format, so it is the first file a new reader opens.
  `#[cfg(test)] #[path = "registry/tests.rs"] mod tests;` moves no code. Raised
  by artisan as AQ-9 during the /review2 on #18.

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

- **split-project-out-of-registry** -- registry.rs holds three types
  registry.rs is 930 lines and defines three types: Project (public, public
  fields, the serde shape of one project's table), RegistryFile (private serde
  shape) and Registry (public, private fields, the read/lookup API). Two
  invariants live in one file, so a reader auditing either one reads past the
  other, and the module doc now has to explain three different places a field is
  checked. Split Project and its to_config/validate into
  config/registry/project.rs, leaving Registry, RegistryFile and parse in
  registry.rs; the tests split along the same seam. Found by artisan (AQ-7)
  reviewing the RemoteRoot/HostName branch for #17, which extended the file
  rather than causing its size. Deferred there because re-cutting a 930-line
  config module inside that branch is the churn #17 itself warned about.

- **provider-change-on-existing-vm** -- a provider edit needs a destroy first
  Found by red-team in round 3 of the review on issue #45. bombyx sets
  VAGRANT_DEFAULT_PROVIDER on every project call but the teardown, which makes
  vagrant refuse rather than substitute -- but only for a machine that does not
  exist yet. Measured on
  frosti: with a machine already created, vagrant reads the provider it recorded
  and ignores the variable, so `VAGRANT_DEFAULT_PROVIDER=hyperv vagrant status`
  on a running libvirt machine exits 0 and reports libvirt. So an operator who
  edits `provider` and re-runs `bombyx up` on an existing project keeps the old
  provider, the new settings block is never applied, and nothing says so. That
  is the same silent mismatch #45 closed, surviving on the re-boot path. `bombyx
  destroy` first is the workaround, and the documents now say so. A real fix
  would compare the provider recorded under the project's `.vagrant/machines/`
  against the configured one and refuse, or report the mismatch in `doctor`.

- **doctor-checks-hyperv-support** -- a skip row is not a check
  Split out of `provider-configured-not-selected` (issue #45) by red-team in
  round 3 of its review. That entry asked doctor to check the host can supply
  the provider the project asks for. For libvirt it already does: the probe
  greps `vagrant plugin list` for `vagrant-libvirt`, so a host missing it gets a
  red row before `up` runs. For hyperv doctor emits `Outcome::Skip("not checked
  for hyperv")`, and a skip is the absence of the check, not the check. The
  concrete gap: on a Linux VM host, `bombyx doctor` for a hyperv project reports
  every row green plus one skip, and `bombyx up` then fails. Writing the probe
  honestly needs a Windows VM host, which nobody has, so this is blocked rather
  than merely unwritten -- the skip row is the honest report until then.

- **project-parsed-at-cli-edge** -- the project name is checked twice
  Found by the artisan review of the #43 branch (AQ-5). The `--project` value
  arrives as a `String` in `main.rs` and stays one into the library, where
  `check_segment` runs on it twice on the ordinary path: once in
  `Config::load_project` before the registry is opened, and again in
  `Registry::project` before the map is consulted. The sibling value already
  does better -- `vm_name` parses the scratch name into a `ScratchName` at the
  CLI edge. Parsing the project name into a `ProjectName` there and taking
  `&ProjectName` in both functions would leave one check. The trade to weigh:
  the message for a bad name moves from `ConfigError::Invalid { field: "project"
  }` to the CLI layer, and the parse has to stay ahead of any file being opened,
  because `ProjectNotFound` advises writing a table heading the parser would
  refuse.

- **split-source-module** -- source.rs holds three unrelated newtypes
  Found by the artisan review of the #43 branch (AQ-6). `config/source.rs`
  crossed 500 lines when `GitRef` was added, and holds three independent
  newtypes -- `RepoUrl`, `ScriptPath` and `GitRef` -- plus the `Source` struct,
  three private rule functions and a test module. Its only cohesion is "the
  `[source]` table", and each further field type adds another block a reader
  scrolls past to reach the one they want. Splitting it into `config/source.rs`
  for the struct and the module doc, with `config/source/repo.rs`,
  `config/source/script.rs` and `config/source/git_ref.rs` beside it, leaves the
  public paths `bombyx::config::{RepoUrl, ScriptPath, GitRef}` unchanged. The
  `checked_str_newtype!` macro took roughly 40 lines back out of the file in the
  meantime, so this is not urgent.

- **doc-link-guard-path-family** -- DocLink accepts a doubled or trailing slash
  DocLink::new refuses blank, rooted, unrenderable, escaping and naming-no-file
  paths, but not the rest of the family CLAUDE.md enumerates under Test-Driven
  Development: doubled slash, trailing slash and an interior . segment. Both
  --doc issues//project-config-off-repo.md and --doc
  ./issues/../issues/project-config-off-repo.md pass every rule today.
  escapes_repo skips empty and . components deliberately, and
  Path::join(..).is_file() normalises the doubled separator, so the existence
  check agrees while the written link keeps the odd spelling. The guard doc
  comment claims it refuses every shape that would not survive the trip to
  another reader, which is one claim wider than the code. Whether a renderer
  resolves docs/issues//plan.md was not verified and should not be assumed; what
  was verified is that the check normalises and the rendered link does not. Fix:
  refuse an empty or . component in escapes_repo, which costs nothing because no
  real target needs one, and add the four rows to the existing test table.
  Raised by red-team in round 3 of the /review2 on #7, at the ceiling.

- **registry-not-found-advice** -- the no-registry message advises too little
  ConfigError::RegistryNotFound in crates/bombyx/src/config/error.rs tells an
  operator with no registry file to create <place> with a [projects."<name>"]
  table and stops there. Its sibling ProjectNotFound lists the .vm and .source
  sub-tables and remote_root as well. The asymmetry is backwards: Project
  requires vm and source with no serde default, so an operator who follows the
  shorter advice literally writes a file bombyx then refuses for a missing
  field, and gets a third failure after that for the host. The variant own doc
  comment claims the opposite, that the message says both. Changing what bombyx
  prints wants a failing test first. Raised by red-team as RT-11 in the /review2
  on registry-config-load (#26).

- **backlog-ids-dangle-in-docs** -- canon-check never reads docs for cited IDs
  cargo xtask canon-check fails on a cited backlog ID that is in no backlog, but
  it reads only .claude/, CLAUDE.md and llms.txt. Files under docs/ cite those
  IDs too and no gate sees them. Deleting 47 entries from the three reviewer
  logs on 2026-09-06 left four dangling citations in
  docs/issues/project-config-off-repo.md, and validate passed the whole time;
  they were found by hand and repaired. A fifth,
  aq-2026-08-10-doctor-module-size in docs/developer/template-feedback.md, has
  been dangling since the 2026-08-18 sweep and nothing reported it either. Fix:
  give the unknown-ids check the same file set the other checks get plus docs/,
  or a second pass over docs/. Watch for the two files that are meant to cite
  closed IDs: an issue record and template-feedback.md both name entries on
  purpose after they are closed, so the check may need to accept a citation that
  says the entry was closed. Found by the artifact walk of the backlog sweep,
  before any reviewer was spawned.

- **comment-claims-have-no-gate** -- no gate checks a claim against the code
  Comments and docs state checkable facts about the code -- how many callers a
  function has, which builders skip a helper, whether a type is public, how many
  files pass a line count -- and no gate compares any of them to the tree. The
  /review2 on the 2026-09-06 backlog sweep found six such claims false in one
  round, and every one had been written that same day while fixing a vaguer
  version of the same sentence. Sharpening prose is what makes it falsifiable,
  so the class grows each time comments are improved. canon-check already reads
  markdown and checks five claim shapes, and backlog-ids-dangle-in-docs asks it
  to widen its file set; a sibling check over doc comments would need to parse
  Rust, which is a bigger job and may not be worth it. Worth deciding what is
  mechanically checkable: a backticked item name in a doc comment that no longer
  exists in the crate is the cheapest candidate, and rustdoc intra-doc links
  already cover part of it. Found during the /review2 on the backlog sweep.

- **more-comments-narrating-the-past** -- a second set of history comments
  comments-narrating-the-past closed on 2026-09-06 having fixed the eight
  comments it named. The /review2 on that sweep then found the class alive
  elsewhere, so the entry covered its own list rather than the tree. The
  survivors fresh-reader named: doctor/readonly.rs lines 130, 375, 391, 401 and
  466 (the substring version this replaced, the earlier substring version,
  vagrant subcommands beyond the four once listed, stopping at the wrapper made
  sudo mkdir read as read-only, taking eight bytes of context panicked);
  remote/probe.rs around 290, the doc on the test helper all, six of whose eight
  lines argue with a deleted list; plan.rs 428, 586 and 806; config.rs 527, 767,
  780 and 1317; remote.rs 828; docs/architecture.md 571;
  xtask/src/licenses/graph.rs 26. Two want judgement rather than a rewrite:
  plan.rs 407-436 argues a case against a reviewer nobody can read and states a
  hypothetical future condition, which CLAUDE.md rules out separately, and the
  invariant worth keeping there is that two tests spell out nearly the same
  script deliberately, because two independently written expectations cannot
  drift the same wrong way. Verify each rewrite against the code as you make it:
  six sharpened comments in the sweep this came from turned out false, which is
  what comment-claims-have-no-gate records.

- **bootstrap-sets-own-path** -- guard without a name list to keep current
  Refusing an `[env]` name is a list that can go stale. `bootstrap.sh` could set
  its own `PATH` and `IFS` instead, but only if it restores the operator's
  values before it execs the project's script. Raised as RT-4(b) on PR #65 and
  rejected there for that reason.

- **bootstrap-harness-runs-the-script** -- run the script, do not match its text
  The tests in `crates/bombyx/src/vagrantfile/bootstrap_tests.rs`
  assert over the text of `bootstrap.sh`, and keeping them working has taken
  three flattening helpers, a comment stripper, a word splitter and two
  allowance lists -- which is the parser `CLAUDE.md` under **Test-Driven
  Development** says such a test ends up being. Four assertions turned out to be
  satisfied by the script's own comments rather than its code, each found one at
  a time: AQ-1, RT-1 and RT-2 on PR #66, over two review runs. The replacement
  is a harness that runs the script: a fake `git` on `PATH`, a temporary `HOME`,
  a fake deploy key, and assertions on what it refuses, what it removes, where
  it clones and what it exits with. That is a contract, and it would have caught
  all four comment-satisfied assertions by construction rather than singly. It
  also covers shapes text matching cannot see at all -- an `mv` on the key, a
  refusal spelled `exit 2`, an unquoted $HOME above the guard -- each of which
  had to be added to a list by hand after a reviewer found it. Unix-only, so it
  needs `#[cfg(unix)]` or an `#[ignore]` on Windows, where CI runs the suite.
  The two cross-file agreement tests stay as they are: their subject is that the
  Rust half and the shell half agree about a name, which is not a behavioural
  question. Seven comment findings on PR #66 were escalated and left unfixed
  because they turn on this decision rather than on seven separate defects:
  FR-1, FR-2, FR-5, FR-7, FR-12, FR-13 and FR-15 in that PR's review record.
  FR-12 is the argument itself.

- **guest-branch-state-differs** -- first up leaves a branch, later ones detach
  The first `up` clones with `git clone --depth 1 --branch <ref>` and leaves the
  guest on a real branch. Every provision after that runs `git checkout --force
  FETCH_HEAD`, which detaches HEAD. So a guest that has never been
  re-provisioned is on a branch and an identical one that has is not, and an
  operator cannot say which state to expect. The detaching itself is deliberate
  and `bootstrap.sh` says so where it happens: a commit made in the guest sits
  on no branch, the next provision moves HEAD away from it, and pushing rather
  than committing is how work survives. That reasoning stands. It is the
  inconsistency between the two paths that is not covered. Found by running
  `bombyx --project jutro-rebuild provision` a second time on 2026-09-07 and
  then looking at the guest's checkout. Still true as of 2026-09-08: the
  `git clone --depth 1 --branch` in bootstrap.sh and the
  `git checkout --force FETCH_HEAD` above it. Cited by command rather than by
  line, because the host-key work moved both.

- **chmod-dirties-the-checkout** -- bombyx modifies a tracked file every run
  `bootstrap.sh` runs `chmod +x "$script_real"` before `exec`ing the project's
  script, so a `vagrant/provision.sh` tracked at mode 100644 becomes 100755 and
  git reports the tree as modified from the moment provisioning finishes. Worked
  around on jutro's side in commit `c71e2d0`, which records 100755 so bombyx's
  chmod changes nothing. The bombyx-side question survives: should bombyx modify
  a tracked file at all, rather than invoking the script through `sh`? Invoking
  through `sh` would drop the shebang, which is a real cost and the reason the
  chmod is there -- so this is a trade rather than an obvious fix. Found by
  running `bombyx --project jutro-rebuild provision` a second time on
  2026-09-07. Still true as of 2026-09-08: the `chmod +x "$script_real"` in
  bootstrap.sh.

- **abort-before-refuse-keeps-the-key** -- an abort leaves the key in the guest
  The three ${VAR:?} checks at the top of bootstrap.sh exit through the shell's
  own expansion rather than through refuse(), so an unset BOMBYX_REPO,
  BOMBYX_REF or BOMBYX_SCRIPT aborts with a message naming bombyx but leaves the
  uploaded deploy key in the guest. Vagrant's file provisioner uploads the key
  before the shell provisioner runs, so the key is already there when those
  lines execute -- which is the one invariant refuse() exists to hold. Found by
  a reviewer while checking a comment that claimed those three names were
  already 'refused'; that comment now states the real mechanism instead.
  Predates the host-key work. Fixing it means either routing the three through
  refuse, which needs refuse() and DEPLOY_KEY declared above them and both
  currently sit below, or accepting the gap and saying so where the checks are.

- **release-bump-ignores-pre-one-zero** -- a removal infers major even at 0.x
  `/release` step 3 infers a major bump from a bullet marked `**BREAKING:**` or
  a non-empty `### Removed`, and `CLAUDE.md` states the same rule under
  **Commits and releases**. Neither reads the current version. SemVer 2.0.0 says
  major version zero is for initial development and anything may change at any
  time, so a breaking change at 0.5.0 is ordinarily 0.6.0, not 1.0.0.
  **Two consecutive releases have now overridden this by hand.** Commit
  `428d9ef` cut v0.5.0 as a minor with 21 breaking bullets, and wrote the
  argument into its own message; the stdin work removed the public
  `RemoteCommand::abbreviated` and the operator called that a minor too. A rule
  nobody has followed twice running is a rule that should change, not a prompt
  people keep answering. Decide whether the inference should read the version
  first and treat a breaking change below 1.0.0 as a minor bump, or whether the
  accept-or-override `AskUserQuestion` -- the last bullet of `/release` step 3,
  not step 4 -- is judged enough on its own. Whichever way, say it in
  `CLAUDE.md` and in `.claude/commands/release.md` both, so the two cannot
  disagree.

- **deploy-key-path-names-vagrant** -- the guest path hard-codes the account
  The deploy key's guest path is hard-coded as
  /home/vagrant/.ssh/bombyx-deploy-key, which assumes the box's SSH account is
  called `vagrant`. Vagrant's default for config.ssh.username is `vagrant`, but
  a box is free to set another, and some do; on such a box the upload fails
  inside Vagrant, a long way from `box` in the config. DEPLOY_KEY_GUEST_PATH in
  crates/bombyx/src/vagrantfile.rs and the matching literal in
  crates/bombyx/templates/bootstrap.sh both carry the assumption. The env_file
  work on branch feat/env-file-delivery shows the way out: Vagrant expands an
  upload's `destination:` by running `printf <destination>` through a shell
  inside the guest as the SSH account (verified in vagrant 2.4.9,
  plugins/provisioners/file/provisioner.rb expand_guest_path and
  plugins/guests/linux/cap/shell_expand_guest_path.rb), so a destination written
  `~/.ssh/bombyx-deploy-key` lands in the real home whatever the account is
  called. bootstrap.sh cannot then use $HOME, because a project's [env] table
  may set it; it reads the passwd entry instead, which is what ENV_FILE already
  does. Found while working issue #78, deliberately left out of that change.

## Done

- **generated-files-world-readable** -- the Vagrantfile lands at mode 664
  (2026-09-13)

- **list-registered-vms** -- list the registered projects and their VM state
  (2026-09-12)

- [**disarm-on-the-ssh-route**](issues/disarm-on-the-ssh-route.md)
  -- the VM host's own environment reaches vagrant
  (2026-09-07)

- **comments-narrating-the-past** -- the eight comments that entry named
  (2026-09-06)

- **newtype-remaining-config-fields** -- types for the five checked fields
  (2026-09-06)

- **provider-configured-not-selected** -- vagrant picks the provider, not bombyx
  (2026-09-05)

- [**reset-needs-snapshot**](issues/registry-run-against-frosti.md)
  -- reset depends on a snapshot nothing creates
  (2026-09-05)

- [**box-must-carry-git**](issues/registry-run-against-frosti.md)
  -- the git requirement surfaces after the boot
  (2026-09-05)

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

