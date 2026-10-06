# Fresh Reader Findings -- Deferred backlog

Comprehension review findings, from the reviewer that reads the
changed files cold. Newest first. A finding that was fixed leaves
no entry -- the comment it produced is the record.

---

### fr-2026-10-03-snapshot-rule-stated-six-times

**Category:** Duplicated rule deferred for its own commit

The rule for `up`'s `fresh-install` snapshot -- a machine `up`
creates, or one whose state it cannot tell -- is written out in
`up_run`'s doc in `main.rs`, `up::up_plan`'s doc, `takes_fresh_snapshot`'s
doc in `listing.rs`, `VmState::is_absent`'s doc, and two comments
in `plan.rs`. The review on `refactor/up-steps-in-library` made
every copy agree on the unknown case; the repair is one statement,
on `takes_fresh_snapshot`, with the others naming it. A
consolidation is its own commit per `/review`.

### fr-2026-10-03-up-run-doc-repeats-up-plan

**Category:** Doc length

`up_run`'s doc in `main.rs` runs to about 48 lines, and most of
it repeats reasoning that now lives with `up::up_plan`,
`up::run_up_steps` and `listing::takes_fresh_snapshot`. Keeping the
summary, the pointer to `up.rs`, what `up_run` itself does and the
dry-run paragraph would cut about 25 lines. It removes rustdoc
links, so the prose stage of that review could not make the cut.

### fr-2026-10-03-provision-advice-does-not-say-whose

**Category:** Program output

Advice such as "run provision for this project" in `bootstrap.sh`
(the `BOMBYX_CLONE_UPDATE` and `BOMBYX_HISTORY` refusals) and
`bootstrap.ps1` sits beside text about "the shell that ran vagrant
on the VM host". An operator who ran `vagrant provision` by hand
may read it as running that again, which does not rewrite the
Vagrantfile. Naming bombyx's step, run from the workstation, would
settle it. The text is program output, so the prose stage of the
review on `fix/advice-names-the-project` could not change it.

### fr-2026-10-03-doctor-firewall-skip-names-no-network

**Category:** Program output

`doctor`'s firewall skip row reads "no network until `up`" without
saying it means vagrant-libvirt's network, on a host whose earlier
rows show SSH working. "libvirt net needs `up`" fits the report's
24-character floor. The text is program output, so the prose stage
of the review on `fix/advice-names-the-project` could not change it.

### fr-2026-10-03-colon-refusal-message-says-file-stream

**Category:** Program output

`WindowsScriptRefusal::Colon`'s `Display` in
`config/source.rs` tells the operator a Windows guest reads a `:`
"as a drive or a file stream". "File stream" is not a term an
operator knows; "an alternate data stream (`setup.ps1:x`)" names
it. The text is program output, so the #175 review's prose stage
could not change it.

### fr-2026-10-03-credential-lists-comment-narrates-history

**Category:** History in a comment

`crates/bombyx/src/vagrantfile/bootstrap_tests.rs` explains that
the credential lists are built from `CREDENTIALS` by recounting
how adding a credential "used to mean editing three separate
lists" and a review that found them lagging. The rule fits in one
sentence: the lists come from `CREDENTIALS`, so a new row reaches
every guard. Raised in the #175 review, outside its diff.

### fr-2026-10-03-unset-home-test-misdescribes-its-sibling

**Category:** False claim in a comment

A comment in `crates/bombyx/src/vagrantfile/bootstrap_tests.rs`
says `every_refusal_clears_every_uploaded_credential` "compares
the offsets of `exit 1` and the removal". That test compares no
offsets: it refuses any `exit` or `return` outside `refuse()` and
checks one `rm -f` line removes every credential. The comment
should say an `unbound variable` abort is not an `exit`, so that
test cannot see it. Raised in the #175 review, outside its diff.

### fr-2026-10-03-firewall-doc-narrates-incidents

**Category:** Comprehension

`docs/vm-host-firewall.md` tells two stories where a rule would
do. Under "Checking that it worked", one paragraph recounts a host
that ran a predecessor ruleset for weeks while reporting green, and
another says earlier versions of the section each gave a wrong
one-sentence reading. `CLAUDE.md` under **Writing** calls "an
earlier version" a defect. Keep the rules -- a table from an older
script passes `status`; read results against the scoped table --
and drop the incidents. Found reviewing PR #173 (#93), in passages
that change did not write, so it was left for its own edit.

### fr-2026-10-02-heading-and-read-error-pointers

**Category:** Comprehension

Two pointers send a reader the wrong way, and each fix touches
code or a rustdoc link, which the prose stage may not. The doc on
`config::registry::heading` says "every message calls this one
function", and `ConfigError::ProjectNotFound`'s doc says "this
message and the two others" without naming them, yet five
`ConfigError` messages (`HookWithoutSecrets` and the four
`WindowsGuest*`) spell `[projects."<name>"...]` by hand. Routing
them through `heading` or narrowing the claim settles it. Also,
`DeployKeyPath::read`'s `# Errors` links to
`EnvFilePath::read`'s list rather than to `WorkstationFileError`,
which now names both fields, and that list's "`Read` when the
file is missing" is wrong for a deploy key, whose missing file is
`DeployKeyError::Missing`. The same list is written out three
times (`read_capped`, `EnvFilePath::read`, `Config::read_staged`).

### fr-2026-10-02-config-test-prose-predates

**Category:** Comprehension

Found reading files the artisan-backlog change touched, on lines
it did not. `config/registry.rs` around line 766 points at a test
`an_illegal_name_cannot_be_built_into_the_argument` in `config.rs`
that does not exist (the table is
`name::tests::a_project_name_is_one_path_segment_or_nothing`), and
says "the old rule ... is now the argument type's". History
phrasing also sits at `registry.rs` around 691 ("is now a checked
type"), `remote.rs` around 25 ("an unchanged path") and
`vagrantfile.rs` around 1846 ("a failure this repo has had
before"). In `config.rs`, `registry_file_in_a_dir`'s doc says it
writes what `registry_with` describes, though callers pass other
text; `registry_with_source_key` lands its key in `[source]` only
because `required_tables` writes that table last, unsaid; and
`load_project_tests::load` calls `load_on`, defined about 600
lines below with no pointer.

### fr-2026-10-01-provision-help-detached-head

**Category:** Comprehension

The `provision` help paragraph on the detached HEAD (#163), which
stage 3 may not edit because `bombyx --help` prints it. It chains a
colon clause, "and so does every checkout after it", and then a
sentence opening with "So", so the fact, the consequence and the
instruction to push run together. It also says "the first `up`
detaches HEAD", and does not say that a provision re-cloning after a
change of `source.repo` does too. Three short sentences would fix it:
the clone is on a detached HEAD after every fresh clone and every
fetching provision; a commit made there is left behind by the next
one; push it. `docs/usage.md` already carries that wording.

### fr-2026-10-01-provision-clone-mode-help-and-messages

**Category:** Comprehension

Printed text about the clone mode (#161, #162) that stage 3 may not
edit, because the program emits it. The `provision --no-fetch` help
says it is refused on a VM with no clone, and not that it is also
refused when the clone belongs to a repository other than
`source.repo`. The `provision` help says the next provision moves
HEAD away, which `--no-fetch` does not do. The `--discard` help says
it overwrites the agent's uncommitted work, but `checkout --force`
leaves an untracked file the fetched commit has no path for. And the
repo-change announcement in `bootstrap.sh` and `bootstrap.ps1` prints
"Uncommitted work in <dir> is lost" even under the default mode, where
the guard has just found none; it should say what that mode loses,
which is ignored files, and keep the full warning for `--discard`.
`docs/usage.md` already carries the corrected wording for the first
two.

### fr-2026-10-01-discard-names-three-things

**Category:** Comprehension

"discard" means three things: the `bombyx discard` subcommand, which
destroys a scratch VM; the new `provision --discard` and
`CloneUpdate::Discard`; and, in older comments and test names in
`bootstrap.sh` and `bootstrap_tests.rs` ("a discard that failed
part-way", `a_discard_that_cannot_finish_says_so`), the `rm -rf` of a
clone whose `source.repo` changed, which also runs under the default
mode. Calling that last step "removing the clone" would leave
"discard" to the subcommand and the flag. Deferred from the #161
review: renaming the older names is churn outside the change, and
`docs/usage.md` now says the flag is not the subcommand.

### fr-2026-09-30-deploy-key-messages-name-the-wrong-thing

**Category:** Comprehension

Printed text about the deploy key that stage 3 may not edit, because
the program emits it. `vault.rs`'s refusal of an empty vault ends
"or name a `deploy_key`", and only `vault.deploy_key` satisfies it,
not the top-level key. `main.rs`'s best-effort `shell` warning says
"not refreshing the secrets" when a missing `deploy_key` file is the
cause, and it skips every refresh. `vagrantfile::assert_staged_matches`
says "an env_file or a vault" where the check is `names_secrets`,
false for a key-only vault. The generated Vagrantfile calls the key
"The credential the guest clones a private repository with", while
"credential" names the git credential file everywhere else.

### fr-2026-09-29-windows-env-refusal-message-reads-twice

**Category:** Comprehension

The message for `ConfigError::WindowsGuestEnv` renders as "... and there
the name is, compared without regard to case as Windows compares it, a
name bombyx refuses on every guest or one its Windows scripts rely on;
set it inside your own script instead". The core "the name is ... a name"
is split by a long clause, and it never names the entry the name matched.
Something like "Windows reads `Path` as `PATH`, which bombyx or its
Windows scripts rely on" would land on the first read. Logged rather than
fixed in the #145 review, because the message is program output and the
comprehension stage changes prose only.

### fr-2026-09-29-account-ps1-255-message-assumes-ssh

**Category:** Comprehension

`account.ps1` prints "the SSH login ... failed, so bootstrap.ps1 did not
run" for exit code 255, but a `bootstrap.ps1` or project script exiting
255 gets the same message. A comment now says so; the message itself
could say "ssh or the remote script exited 255". Logged rather than fixed
in the #145 review, because the message is guest output.

### fr-2026-09-29-box-build-hides-its-build-folder

**Category:** Comprehension

`boxes/windows-server-2025/build.sh` puts qemu's monitor socket, the
only way to look at a stuck build, in a `mktemp` folder whose name it
never prints. `docs/windows-guest-box.md` says to look for the
`build.*` folder in the work folder, which works while one build runs.
Printing the folder when the install starts would name it. Logged
rather than fixed in the #144 review, because printing it changes the
script's output and the comprehension stage changes prose only.

### fr-2026-09-28-destroy-help-leans-on-undefined-terms

**Category:** Comprehension

The `destroy` subcommand's clap help in
`crates/bombyx/src/bin/bombyx/main.rs` says it "discards the warm
caches the persistent lifecycle exists to keep". Both terms are
defined only in `docs/usage.md`, which a `bombyx destroy --help`
reader never sees. Plain words would serve: it discards the VM's
disk and everything installed on it, which is what makes a later
`up` fast. Logged rather than fixed in the #116 review, because
editing clap help changes the program's output and the
comprehension stage does not touch it.

### fr-2026-09-28-flatten-comment-narrates-history

**Category:** Comprehension

The comment above `Cmd::Vm` in `crates/bombyx/src/bin/bombyx/main.rs`
says the `--help` listing "does change: `self-update` now heads it
instead of sitting between `destroy` and `scratch`". "Now" compares
against a listing a newcomer never saw. State the current fact:
`self-update` and `list` come first because a flattened enum adds
its subcommands where it is flattened. Deferred from the #116
review, which did not touch those lines.

### fr-2026-09-28-gem-versions-point-at-a-check-that-omits-them

**Category:** Comprehension

`docs/vm-host-setup.md`, in "A fog warning that bombyx filters",
says the gem pair was "seen with `vagrant-libvirt 0.12.2` and
`fog-libvirt 0.15.0` at the August 2026 check above". The check
near the top of the page names Ubuntu 24.04.4 and Vagrant 2.4.9
and no plugin or gem versions, so a reader who goes back to
confirm finds nothing. Record the versions next to that check,
or state them here without pointing "above". Deferred from the
#55 review: the sentence predates that change.

### fr-2026-09-28-whether-vagrant-reprovisions-has-five-homes

**Category:** Comprehension

Five places state whether `vagrant up` provisions a machine that
already exists, in five versions: `Action::Provision` and
`Action::Up` in `plan.rs` say it never does, the clap help for
`Up` in `main.rs` says "is not provisioned again", `up_run` says
"usually", and `listing::refreshes_secrets_after_up` names the
exception -- vagrant provisions an existing machine whose
provision marker is missing. The last is the measured behaviour
(RT-3 on PR #126). State the rule once there, with the exception,
and point the other four at it or soften them to "normally".
Escalated rather than applied: it is a consolidation of five
copies, and one of them is clap help, which the prose stage may
not edit. Found as FR-2 on PR #126.

### fr-2026-09-25-the-shell-ignores-an-env-home

**Category:** Behaviour

When a project's `[env]` table sets `HOME`, `bootstrap.sh` clones
under that value, but `bombyx shell` resolves `$HOME` as the
agent's passwd home after `sudo -H`, so it opens outside the
clone after `cd` prints an error. The comments in
`remote::shell_into_vm` and `bootstrap.sh`, and the tutorial, now
say so. bombyx reads the `[env]` table itself, so it could pass
that `HOME` to the shell entry instead.

Deferred on 2026-09-25: a behaviour change found in the
prose-only stage of PR #124's review, which fixes only what a
person reads. Found as FR-4.

---

### fr-2026-09-24-shell-help-names-no-path

**Category:** help text

`bombyx --help` describes `shell` as "Open a shell inside the project
VM, in the project clone" (`VmCmd::Shell` in
`crates/bombyx/src/bin/bombyx/main.rs`; `Action::Shell` in `plan.rs`
has the same words). An operator reading only `--help` has not been
told what "the project clone" is or where it lives. Name the path, for
example "starting in the guest's `~/<project>`, where bombyx cloned the
repository". Raised on PR #120. `/review`'s stage 3 does not edit clap
help, because `bombyx --help` prints it, so it was deferred.

### fr-2026-09-24-clone-fallback-narrates-history

**Category:** history in prose

The comment above `readonly CLONE_DIR` in
`crates/bombyx/templates/bootstrap.sh` explains the `project`
fallback through "an older bombyx always cloned into a fixed
`$HOME/project`". The same history is told at `bootstrap.sh`'s header
("a directory an older bombyx wrote") and in the `PROJECT_ENV` doc in
`vagrantfile.rs` ("still clones where it always did"). A reader cannot
tell whether the fallback is a live requirement or dead compatibility
code. State the current reason: `BOMBYX_PROJECT` is unset when someone
runs `vagrant provision` by hand against a Vagrantfile that does not
set it, and the fallback keeps `set -u` from aborting. If that route is
unsupported, say so instead. Raised on PR #120. The text predates that
PR, and fixing it means merging three copies, so it was deferred.

### fr-2026-09-23-firewall-doc-narrates-history

**Category:** history in prose

`docs/vm-host-firewall.md` tells two incidents where it should state
rules: "That has happened here: a host ran a predecessor ruleset for
weeks whose DNS accept was not pinned", and "Earlier versions of this
section tried to give you one sentence...". `docs/todo.md` under
`host-network-isolation` says "since the probe was corrected". A
reader cannot tell whether "predecessor ruleset" means an older script
they might still have loaded. Keep the rule and drop the incident: for
example, `status` does not compare against `show`, so a table from an
older script passes; re-run `apply` after changing the script. Raised
on PR #114; outside that PR's change, so deferred.
