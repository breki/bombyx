# Red Team Findings -- Deferred backlog

Security (Red Team) review findings. Newest first.

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
