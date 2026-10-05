# Red Team Findings -- Deferred backlog

Security (Red Team) review findings. Newest first.

---

### rt-2026-10-05-self-update-takes-an-unpublished-tag

**Category:** Correctness

`update::version::newest_release` picks the highest version among
the remote's tags (`git ls-remote`), whether or not a GitHub
release exists for it. Between a tag push and the release
workflow's Publish step -- several minutes -- `bombyx self-update`
therefore tries to install a version with no assets and fails on
a 404 for `SHA256SUMS`, with advice that the release "predates
checksummed releases". Seen with v0.12.0 on 2026-10-05. Two fixes:
ask for published releases rather than tags, or treat a missing
`SHA256SUMS` on the newest tag as "not published yet" and say so.

### rt-2026-10-03-firewall-skip-wins-over-a-pass-line

**Category:** Correctness (unverified)

`firewall_verdict` in `doctor/probes.rs` returns the "no network"
skip whenever `FIREWALL_NO_NETWORK` appears on any line of the
probe's stdout, so output carrying a pass line and that token
together reads as a skip. Raised in passing by fresh-reader in the
review on `fix/advice-names-the-project`, outside its diff. Nobody
has checked whether the probe can print both.

### rt-2026-10-03-box-build-skips-unreadable-answer-files

**Category:** Correctness

`boxes/windows-server-2025/stage.ps1` picks the answer files to
remove with `Select-String -Quiet -ErrorAction SilentlyContinue`,
and lists `C:\Windows\Panther` with `Get-ChildItem -ErrorAction
SilentlyContinue`. A file Windows holds open without read sharing,
or a subdirectory it cannot list, is skipped with no line in the
build log, so a box can ship an answer file naming the stale
Administrator password and vagrant's well-known one without a
trace. The fix collects those read errors (`-ErrorVariable`) and
reports each file as the delete failure is reported. Deferred from
the #175 review: found in its third red-team round, the ceiling,
so a behaviour fix there would have had no review, and no box
build has run it.

### rt-2026-09-30-windows-refresh-mixed-advice

**Category:** Correctness

When a Windows guest needs provisioning again -- it has no
`refresh.ps1`, its helpers take another call version, or its agent
account is missing -- `refresh-call.ps1` or `refresh.ps1` prints
"run provision for this project" and exits 1. `run_refresh` in the
binary then adds `RefreshOutcome::WriteFailed`'s "could not refresh
a secrets file in the guest; run the command again", which cannot
help. The fix is a status of its own for "provision needed", which
`RefreshOutcome` maps to that advice; it adds a variant to a public
enum. Deferred on 2026-09-30: Windows guests have not shipped, so no
guest can be in this state yet. Found as RT-5 in the first red-team
round on PR #152.

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

### rt-2026-09-13-env-file-read-has-no-size-cap-and-a-toctou-gap

**Category:** Security (low)

The workstation-file read in
`crates/bombyx/src/config/workstation_path.rs`, which
`EnvFilePath::read` goes through, checks the path with
`std::fs::metadata` and then opens it again, so a fifo swapped in
between the two makes bombyx block in
`File::open` with no message. It needs a directory the operator
does not control, e.g. `/tmp`. The fix is platform-specific
(`O_NONBLOCK`, `O_NOFOLLOW`). Deferred: the `metadata` check
already closes the `/dev/zero` case the review was about. The
size cap this entry also asked for has landed as
`workstation_path::MAX_FILE_BYTES`, which bounds what a fifo can
feed bombyx but does not stop the block.

