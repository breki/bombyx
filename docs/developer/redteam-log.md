# Red Team Findings -- Deferred backlog

Security (Red Team) review findings. Newest first.

---

### rt-2026-09-29-bare-ps1-script-checks-disagree

**Category:** Correctness

`ScriptPath::is_powershell` refuses a script named `.ps1` with nothing
before the extension, while `bootstrap.ps1` would accept one, because
`[IO.Path]::GetExtension('.ps1')` returns `.ps1`. The config refusal makes
the case unreachable today; the two checks still disagree on it. Raised by
fresh-reader in the #145 review, after stage 2 had closed.

### rt-2026-09-29-windows-guest-user-misses-groups-and-devices

**Category:** Correctness

`WINDOWS_BUILT_IN_USERS` and `account.ps1`'s copy refuse four built-in
accounts, but Windows also cannot give an agent account a built-in
group's name (`users`, `guests`, `administrators`, `replicator`) or a
DOS device name (`con`, `prn`, `aux`, `nul`, `com1`-`com9`,
`lpt1`-`lpt9`), which cannot be a profile folder. A project name that is
a device name cannot be the clone folder either. Each fails in the guest
after the boot. Deferred in the #145 review by the operator's choice.

### rt-2026-09-29-windows-script-backslash-traversal

**Category:** Correctness

`check_inside_clone` splits a `script` on `/` only, so a Windows
`script` such as `..\setup.ps1` or `C:\x.ps1` passes the config and is
refused by `bootstrap.ps1` in the guest, after the boot. For a Windows
project the config could refuse a `\` or a `:` in `script`. Deferred in
the #145 review by the operator's choice; not a security gap, because
the guest refuses it.

### rt-2026-09-29-box-build-answer-file-delete-is-fatal

**Category:** Correctness

`boxes/windows-server-2025/stage.ps1` deletes each answer file under
`C:\Windows\Panther` that holds `<PlainText>` under
`$ErrorActionPreference = 'Stop'`, so a file Windows holds locked
fails the whole build after the update rounds. Since the build resets
the Administrator password first, those files name only a stale
password, so the delete may be better reported and skipped than made
fatal. No build has hit a locked file. Raised by fresh-reader in the
#144 review, after stage 2 had closed.

---

### rt-2026-09-28-listing-temp-file-loses-a-block

**Category:** Correctness

`remote::vagrant_status_many` writes each project's block into a
file under `mktemp -d` on the VM host and prints the files at the
end. When the host's temporary directory is full or read-only, a
fragment's `printf` into its file fails. If that fragment is not
the last one, the script can still exit 0, so `listing::entries`
attaches no host reason, and the row reads "the host did not
report this project" with no explanation. The same path serves
`up` and `shell` through `probe_state`, and it fails safe there:
the state is unknown and `shell` does not refuse.

A fix would print each marker from the parent shell before its
`cat`, so the block survives a failed write, and would make the
script exit non-zero when any file could not be written.

Deferred on 2026-09-28: it needs an unwritable temporary directory
on the VM host, the row fails safe, and the fix changes the script's
failure reporting, which is more than a review fix for issue #115
(PR #133). The comment in `listing::entries` now states the
dependency. Found as RT-2 in the second red-team round.

---

### rt-2026-09-28-guest-advice-names-no-project

**Category:** Correctness

Five messages bombyx prints still spell a whole command line with
no project, such as `bombyx provision`, which is a usage error now
that every VM command takes the project as its first argument:
three in `remote.rs` (the two `run bombyx provision.` refresh
scripts and the `run bombyx provision, or bombyx destroy then
bombyx up` message), and the two `run bombyx destroy, then bombyx
up.` refusals in `templates/account.sh`. None ever named
`--project` either. Most run in a
guest shell script, so naming the project means passing it into
that script safely, and the change alters files bombyx writes onto
the VM host, which wants a real run. Deferred from the #116
review, which fixed the host-side `status` message beside it.
Two constraints on the fix: name the verb and not a whole command
line, as `listing::shell_refusal` explains, and use no backticks
in the messages inside a double-quoted `echo`, where they would
run as a command substitution.

### rt-2026-09-28-up-run-sequence-untested

**Category:** Correctness

`up_run` in `crates/bombyx/src/bin/bombyx/main.rs` decides the
order of the boot, the secrets refresh, the `secrets_refreshed`
hook and the `fresh-install` snapshot, and whether each runs, from
the probed machine state. The binary is outside the tests, so that
sequence is covered only by real runs, and it has been reordered
seven times in a week. `listing::takes_fresh_snapshot` and
`listing::refreshes_secrets_after_up` already hold two of its
decisions in the tested library. Move the rest there too: a
function returning the ordered steps for a probed state and a
config, which `up_run` executes, so the next reordering fails a
test. Found as RT-6 in round 2 of the review on PR #128; deferred
because it restructures a function that PR only extended.

### rt-2026-09-28-up-stages-secrets-for-a-boot-that-reads-none

**Category:** Security

`bombyx up` on a VM that exists but is stopped runs the same plan
as a first `up`, so `write_then` stages `bombyx.env` and the git
credential in the project directory on the VM host for the whole
`vagrant up`. Vagrant usually does not provision an existing
machine, so usually nothing reads those copies; since #125 the
secrets reach the guest through the refresh that follows the boot.

Skipping the staging whenever the status probe says the machine
exists is wrong. vagrant-libvirt's `up` provisions an existing
machine whose `.vagrant/machines/default/<provider>/action_provision`
marker is missing -- a first `up` killed partway, or a `vagrant up
--no-provision` by hand -- and that provision then refuses for want
of the files. The fix has to follow the marker, not the probe: have
the staging writes on the VM host test for it and stage only when
it is absent. Found as RT-1 on PR #126; the probe-based fix was
reverted after its second round (RT-3 there).

### rt-2026-09-25-the-root-script-runs-in-the-projects-environment

**Category:** Security

`crates/bombyx/templates/account.sh` runs as root with the whole
`[env]` table in its environment, because Vagrant applies the
provisioner's `env:` block as a prefix inside the root shell. The
reserved-name list in `config/env.rs` covers the names that change
what `bash` or `git` does, and `SUDO_USER`, but any other variable
a root tool reads is the project's to set. `TMPDIR` was one:
`mktemp` followed it, and it now takes an absolute template in
`/etc/sudoers.d` instead (RT-2 on PR #124).

The general answer is to stop the environment steering root at
all: have `account.sh` read the `BOMBYX_*` names it needs, then
run its own tools under `env -i` with a fixed `PATH`, while still
handing the full list to `sudo --preserve-env` for `bootstrap.sh`.
That changes how the two scripts share the environment, which is
a design change rather than a round's fix.

Deferred on 2026-09-25, while the agent keeps passwordless `sudo`
and so can reach root anyway. It stops being moot the day that
`sudo` is withdrawn. Found as RT-2's broader half.

---

### rt-2026-09-14-the-present-pair-keeps-two-sources-of-truth

**Category:** Design (an invariant asserted rather than made
unrepresentable)

`crates/bombyx/src/vagrantfile.rs` renders
`BOMBYX_ENV_FILE_PRESENT` and `BOMBYX_GIT_CRED_PRESENT` from
the `Staged` it is handed, and then asserts that the two halves
match `cfg.source.env_file` and `cfg.source.repo_token`. So the
config keys and the staged value are both still sources of
truth, the mismatch is still representable, and the failure is
a panic inside a `pub fn`.

The pair had been rewritten in five commits over two days when
this was raised: config key, then config key with a second copy
answered inside the guest, then `Staged`, then `Staged` plus
the assert. Each round was a correct fix for the case that
prompted it.

`Staged`'s fields are private, so the pairing could be made
unrepresentable instead: a value carrying the borrowed `Config`
and the `Staged` read from it, built by one constructor, with
`render` and `plan` taking that. The assert and both `# Panics`
sections would go with it.

Deferred by the operator on 2026-09-14: the change reaches
`plan`, `vagrantfile` and `main`, and belongs in its own commit
rather than folded into a branch that had already had three
review stages. Found as RT-12.

The proposed constructor does not close this alone. `plan` is
handed `Staged::default()` for the seven verbs
`Action::staged_read` skips, and for `shell` when its best-effort
read fails, because each must work after the operator deleted
the secrets file. A value pairing a `Config` with its read
`Staged` cannot exist for those calls, so `plan` would take an
`Option` of it and the three write arms would panic on `None`
instead. Decide what those calls are handed first.

### rt-2026-09-13-cross-key-rule-count-stated-in-six-places

**Category:** Correctness (escalated consolidation)

The rules spanning more than one `[source]` key are stated in five
places -- `config.toml.sample`, `docs/usage.md`,
`docs/architecture.md` twice (prose and the refusal table), and
`llms.txt` -- so a new rule means five edits, and `canon-check`
reads only `CLAUDE.md`, `llms.txt` and `.claude/`. Repair: one
authoritative list, the others pointing at it. Deferred: a
many-to-one consolidation is its own commit per `/review`.

### rt-2026-09-13-env-file-read-has-no-size-cap-and-a-toctou-gap

**Category:** Security (low)

`EnvFilePath::read` in `crates/bombyx/src/config/env_file.rs`
checks the path with `std::fs::metadata` and then opens it again,
so a fifo swapped in between the two makes bombyx block in
`File::open` with no message. It needs a directory the operator
does not control, e.g. `/tmp`. The fix is platform-specific
(`O_NONBLOCK`, `O_NOFOLLOW`). Deferred: the `metadata` check
already closes the `/dev/zero` case the review was about. The
size cap this entry also asked for has landed as
`MAX_ENV_FILE_BYTES`, which bounds what a fifo can feed bombyx
but does not stop the block.

### rt-2026-09-11-exit-rule-has-no-single-home

**Category:** Duplicated rule deferred for its own commit

`bombyx list`'s exit-status rule is stated in five places (clap
help in `main.rs`, `README.md`, `docs/usage.md`, `llms.txt`,
`CHANGELOG.md`) and the `--project` requirement in three, with
none owning the rule. Repair: one owner, pointers behind. Deferred
per `/review` (a consolidation is its own commit); worth deciding
whether the clap help can be a pointer at all, since it is what
`bombyx list --help` prints.
