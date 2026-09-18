# TODO

Project work queue.

- `/todo <text>` captures a new item with a generated slug.
- `/todo` (no arguments) lists pending slugs.
- `/implement <slug>` plans and implements a pending item.
- `/implement` (no arguments) lists pending items and asks
  which to act on.

While an item is being implemented it gets a working planning
doc at `docs/issues/<slug>.md` (problem, plan, decisions). It is
removed when the item lands, with any durable decision promoted
into the reference docs first -- see the `/implement` skill.

Each item is a headed entry: an `### <slug>` heading, a
`**Summary:**` line, optional `**Depends on:**` naming another
live slug, then any prose body. `cargo xtask records-check`
validates the ids and cross-references, so add and remove entries
through `cargo xtask todo` rather than hand-editing those fields.
A body may be edited directly, as long as the heading, Summary
and Depends-on lines are left intact and `records-check` still
passes.

### wire-vm-host

**Summary:** the VM host is on WiFi; VLAN tagging needs it wired

Prerequisite for the agent VLAN.

### packer-box

**Summary:** bake a base box so `scratch` boots fast enough to use.

### agent-vlan

**Summary:** isolate VMs on a VLAN with an egress allowlist
**Depends on:** wire-vm-host

Enforced at the router.

### host-network-isolation

**Summary:** confirm the nftables rules survive a reboot

`apply` and `persist` have run on the VM host, and the in-VM checks pass: the
guest keeps its outbound internet access, the router is rejected by the
forward chain, and the VM host's own addresses are dropped by the input
chain.
`nft -c` accepts the generated ruleset on nftables 1.0.9, and a fresh
host-into-guest connection still works, so the input drop does not break
bombyx.
What is left is the reboot. Persistence is the one part that cannot be
confirmed any other way and it fails silently, so run
`sudo agent-vm-firewall status` after a restart and only then drop the
*(unverified)* marker from the heading in docs/vm-host-setup.md.
Three things stay unexercised whatever the reboot says. The IPv6 rule,
because the guest has no IPv6 route at all. The WSL host, which nobody has
re-checked since the probe was corrected. And the pinned DHCP and DNS
accepts: issue #92 records that the guest resolves through public resolvers
baked into the box image, so the `dns: ok` line answered through those and
never asked the gateway. Deleting those accepts would leave every check we
ran still passing. This is a host-level stopgap for
agent-vlan, not a replacement: enforcement sits on the machine being
protected.

### suspend-resume-commands

**Summary:** save and restore VM RAM state mid-task

Add `bombyx suspend` / `bombyx resume` subcommands wrapping `vagrant suspend`
/ `vagrant resume`, so a VM's RAM state can be saved and restored mid-task.
Context: `Action::Down` maps to `vagrant halt`, which is a graceful
power-off -- the disk survives but running processes, tmux sessions and
listening ports do not. There is currently no way to pick up mid-task after
stopping a VM.

### minimal-vagrantfile

**Summary:** keep the generated file identical across hosts

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

### provision-lifecycle-hooks

**Summary:** named hooks replace one bash provision script

Provisioning is currently one bash script run by Vagrant at VM creation.
Replace it with named lifecycle hooks a project declares in a small manifest:
prepare, dependencies, agent, cleanup. Vagrant then only creates the VM and
runs the bootstrap; bombyx runs the hooks inside the guest. Simple projects
implement one hook, complex ones several, and bombyx stays generic. It also
decouples the hooks from Vagrant, so the backend can change later without
touching them.

### per-host-resource-profiles

**Summary:** detect host capacity, merge project minimums

The same project run on a workstation and on a laptop should not get the same
VM. Let the project declare its needs (minimum memory, minimum CPUs) and let
each host contribute what it can provide. Detect RAM, CPU cores and hypervisor
when a host is first used, apply a default policy such as half of RAM, and
allow a per-host override file. Named profiles are the other half: a profile
maps to a large allocation on the workstation and a smaller one on the laptop.

### status-endpoint

**Summary:** read-only per-host VM status over the network

There is no overview of what is running across machines. Each bombyx
installation could expose a small read-only endpoint reporting its VMs, their
projects, state and resource usage. Read-only keeps the autonomy of the
current design: no central registry, no service to keep alive, no single point
of failure. Report two distinct roles per VM, since they differ in this setup:
the controller, meaning the instance that launched it, and the executor,
meaning the host it actually runs on. Bind it to the private network only.

### status-all-aggregator

**Summary:** bombyx status --all queries the known hosts
**Depends on:** status-endpoint

The consumer of the per-host status endpoints. The client initiates: no
background chatter, no instances polling each other. Discovery starts as a
static config file listing the other hosts, which is dull and reliable; keep
the lookup behind an interface so a tailnet or Consul provider can be added
later without changing callers. The CLI is the first consumer, a dashboard is
possible afterwards.

### self-update-resolves-tar-late

**Summary:** two downloads before it notices no tar

self-update resolves each program as it runs, not up front:
`self_update` calls `ran_ok` three times, so `tar` is resolved
only at the extraction step, after `curl` has already fetched the
checksums and the archive. On a machine with git and curl but no
tar, `bombyx self-update` does two network round trips and then
fails. Fix: resolve git, curl and tar before the first fetch, and
add git to the tool lists, where it is currently missing. Found
by red-team (RT-7).

### validate-resume-from-step

**Summary:** let validate resume at the gate that failed

`cargo xtask validate` prints `-> iterate with: cargo xtask
<cmd>` on a failure, but re-running one gate is a different
command from re-running the pipeline -- that is the friction, and
it costs a full multi-gate run each time the hint is skipped. A
`--from <step>` flag would make the resume as cheap to type as
the restart, and the step names already exist in `validate.rs`'s
step list. Raised by the workflow retrospective, 2026-09-03.

### destroy-confirmation-shape

**Summary:** what destroy's positional becomes

Step 7 of 7; GitHub issue #27. Depends on project-selection-flag. One design
question, undecided.

### config-tests-own-file

**Summary:** config.rs and registry.rs tests into their own files

`mod tests` is most of `config.rs` by a wide margin, which makes
the file unreadable in one pass. No line count is given here on
purpose: the file grows every commit, and a stale figure costs
the next reader a check. Move it with `#[cfg(test)]
#[path = "config/tests.rs"] mod tests;`, and carry the
module-scope fixtures out with it -- `TABLE_FIELDS`,
`required_tables`, `test_entry`, `test_entry_with` and
`test_registry` -- since they sit above the `pub use` block and a
reader scanning for the public surface hits test scaffolding
first. `config/registry.rs` wants the same move and should land
with it: roughly 900 lines, ~440 of them `mod tests`, and after
#18 it owns the whole config format, so it is the first file a
new reader opens; `#[cfg(test)] #[path = "registry/tests.rs"]
mod tests;` moves no code. Raised by artisan (AQ-9) and red-team.

### config-home-env-provenance

**Summary:** say when the environment picked the config

An env-set `BOMBYX_CONFIG_HOME` redirects bombyx to another
`config.toml`, giving the winning origin `HostOrigin::UserFile`,
which `main.rs` stays silent about -- so bombyx runs against a
host the operator never configured and prints nothing. An
anchored value such as `/tmp/pwn` passes `is_anchored_dir`, and a
per-directory tool (`direnv` reading an `.envrc` in a clone,
`mise`, a CI job) can set it. The first half landed in #18: the
notice names the file bombyx read rather than a bare
`config.toml`. What remains: print the provenance line for
`UserFile` too whenever `CONFIG_DIR_ENV` supplied the directory,
which needs a failing test first. Red-team RT-1.

### vm-disk-size-unset

**Summary:** no disk key, so the guest gets the box's own size

Found by the local-route verification run (#37), driving the CLI against the
VM host.
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

### scratch-domain-name-collides

**Summary:** one libvirt domain for two scratches

Found by the local-route verification run (#37). config.toml.sample claims
that
scratch VMs land in remote_root/scratch/project/name, so the same scratch name
in two projects cannot collide. The directories indeed cannot. The libvirt
domain names can. vagrant-libvirt builds the domain name as the basename of
the directory holding the Vagrantfile, an underscore, and the Vagrant machine
name -- which bombyx never sets, so it is Vagrant's default, `default`. Three
domains on the VM host follow that rule: ~/vms/jutro gave jutro_default,
~/vms/vmtest gave vmtest_default, and
~/vms/scratch/vmtest/probe gave probe_default. The project name is nowhere in
the last one, so a probe scratch in a second project would ask libvirt for
probe_default as well. The collision itself was not booted, so treat the
mechanism as evidenced by three domains and not as demonstrated. The claim in
the sample is what needs settling either way: either the domain name gains the
project, or the sample stops promising more than the directory layout
delivers.

### split-project-out-of-registry

**Summary:** registry.rs holds three types

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

### provider-change-on-existing-vm

**Summary:** a provider edit needs a destroy first

Found by red-team in round 3 of the review on issue #45. bombyx sets
VAGRANT_DEFAULT_PROVIDER on every project call but the teardown, which makes
vagrant refuse rather than substitute -- but only for a machine that does not
exist yet. Measured on the VM host: with a machine already created, vagrant
reads the provider it recorded and ignores the variable, so
`VAGRANT_DEFAULT_PROVIDER=hyperv vagrant status`
on a running libvirt machine exits 0 and reports libvirt. So an operator who
edits `provider` and re-runs `bombyx up` on an existing project keeps the old
provider, the new settings block is never applied, and nothing says so. That
is the same silent mismatch #45 closed, surviving on the re-boot path. `bombyx
destroy` first is the workaround, and the documents now say so. A real fix
would compare the provider recorded under the project's `.vagrant/machines/`
against the configured one and refuse, or report the mismatch in `doctor`.

### doctor-checks-hyperv-support

**Summary:** a skip row is not a check

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

### project-parsed-at-cli-edge

**Summary:** the project name is checked twice

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

### split-source-module

**Summary:** source.rs holds three unrelated newtypes

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

### registry-not-found-advice

**Summary:** the no-registry message advises too little

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

### comment-claims-have-no-gate

**Summary:** no gate checks a claim against the code

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

### bootstrap-sets-own-path

**Summary:** guard without a name list to keep current

Refusing an `[env]` name is a list that can go stale. `bootstrap.sh` could set
its own `PATH` and `IFS` instead, but only if it restores the operator's
values before it execs the project's script. Raised as RT-4(b) on PR #65 and
rejected there for that reason.

### bootstrap-harness-runs-the-script

**Summary:** run the script, do not match its text

The tests in `crates/bombyx/src/vagrantfile/bootstrap_tests.rs`
assert over the text of `bootstrap.sh`, and keeping them working
has taken three flattening helpers, a comment stripper, a word
splitter and two allowance lists -- the parser `CLAUDE.md` under
**Test-Driven Development** says such a test becomes. Four
assertions turned out to be satisfied by the script's comments
rather than its code (AQ-1, RT-1, RT-2 on PR #66). Replace them
with a harness that runs the script: a fake `git` on `PATH`, a
temporary `HOME`, a fake deploy key, and assertions on what it
refuses, what it removes, where it clones and what it exits with.
That is a contract, and it also covers shapes text matching
cannot see -- an `mv` on the key, a refusal spelled `exit 2`, an
unquoted `$HOME` above the guard. Unix-only, so it needs
`#[cfg(unix)]` or an `#[ignore]` on Windows, where CI runs the
suite. The two cross-file agreement tests stay: their subject is
that the Rust and shell halves agree about a name, which is not a
behavioural question. Seven comment findings on PR #66 turn on
this decision and are left unfixed until it is made: FR-1, FR-2,
FR-5, FR-7, FR-12, FR-13, FR-15 (FR-12 is the argument itself).

### guest-branch-state-differs

**Summary:** first up leaves a branch, later ones detach

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

### chmod-dirties-the-checkout

**Summary:** bombyx modifies a tracked file every run

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

### abort-before-refuse-keeps-the-key

**Summary:** an abort leaves the key in the guest

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

### release-bump-ignores-pre-one-zero

**Summary:** a removal infers major even at 0.x

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

### deploy-key-path-names-vagrant

**Summary:** the guest path hard-codes the account

The deploy key's guest path is hard-coded as
`/home/vagrant/.ssh/bombyx-deploy-key`, which assumes the box's
SSH account is `vagrant`. That is Vagrant's default for
`config.ssh.username`, but a box may set another, and some do; on
such a box the upload fails inside Vagrant, a long way from `box`
in the config. `DEPLOY_KEY_GUEST_PATH` in
`crates/bombyx/src/vagrantfile.rs` and the matching literal in
`crates/bombyx/templates/bootstrap.sh` both carry the assumption.
The way out: Vagrant expands an upload's `destination:` by
running `printf <destination>` through a shell inside the guest
as the SSH account (verified in vagrant 2.4.9, file provisioner's
`expand_guest_path`), so `~/.ssh/bombyx-deploy-key` lands in the
real home whatever the account is called. `bootstrap.sh` cannot
then use `$HOME`, because a project's `[env]` table may set it;
it reads the passwd entry instead, as `ENV_FILE` already does.
Found while working issue #78, left out of that change.

