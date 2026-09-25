# Red Team Findings -- Deferred backlog

Security (Red Team) review findings. Newest first.

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
handed `Staged::default()` for the eight verbs
`Action::needs_staged_files` excludes, because each must work
after the operator deleted the secrets file. A value pairing a
`Config` with its read `Staged` cannot exist for those calls, so
`plan` would take an `Option` of it and the three write arms
would panic on `None` instead. Decide what those eight verbs are
handed first.

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
