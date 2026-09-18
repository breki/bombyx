# Red Team Findings -- Deferred backlog

Security (Red Team) review findings. Newest first.

---

### rt-2026-09-13-bootstrap-guards-enumerated-by-hand

**Category:** Correctness (escalated consolidation)

`crates/bombyx/templates/bootstrap.sh` gained a third uploaded
credential, and three hand-written lists in
`crates/bombyx/src/vagrantfile/bootstrap_tests.rs` (the
declared-before-expanded list, the `refuse` removal assertion, the
bare-expansion list) plus the file's prose banners each had to be
extended to match; nothing fails when one is missed. `red-team`
proposed one `(variable, guest path, presence flag)` table driving
all three guards. Deferred: a consolidation across three tests and
a template, escalated per `/review`, with no defect behind it
today.

### rt-2026-09-13-cross-key-rule-count-stated-in-six-places

**Category:** Correctness (escalated consolidation)

The rules spanning more than one `[source]` key are stated in five
places -- `config.toml.sample`, `docs/usage.md`,
`docs/architecture.md` twice (prose and the refusal table), and
`llms.txt` -- so a new rule means five edits, and `canon-check`
reads only `CLAUDE.md`, `llms.txt` and `.claude/`. Repair: one
authoritative list, the others pointing at it. Deferred: a
many-to-one consolidation is its own commit per `/review`.

### rt-2026-09-13-no-gate-for-a-missing-doc-comment

**Category:** Correctness (missing gate)

`CLAUDE.md` requires a doc comment on every public item and
nothing enforces it: a scripted edit once reassigned `https_host`'s
comment to a new function and published it undocumented, ten gates
green. `RUSTFLAGS="-W missing_docs" cargo build -p bombyx` reports
zero violations today, so enabling the lint in the workspace block
is a one-line change. Deferred: a workspace-wide lint change wants
its own commit.

### rt-2026-09-13-env-file-read-has-no-size-cap-and-a-toctou-gap

**Category:** Security (low)

`EnvFilePath::read` in `crates/bombyx/src/config/env_file.rs`
calls `std::fs::metadata` then `std::fs::read`, re-resolving the
path rather than holding it open. Two gaps: a fifo swapped in after
the regular-file check makes bombyx block or grow unbounded (needs
a directory the operator does not control, e.g. `/tmp`); and no
size cap, so a multi-gigabyte regular file is read whole into
memory (`config::MAX_CONFIG_BYTES` is the precedent). The fifo fix
is platform-specific (`O_NONBLOCK`, `O_NOFOLLOW`); the size cap is
cheap alone. Deferred: the `metadata` check already closes the
`/dev/zero` case the review was about.

### rt-2026-09-11-exit-rule-has-no-single-home

**Category:** Duplicated rule deferred for its own commit

`bombyx list`'s exit-status rule is stated in five places (clap
help in `main.rs`, `README.md`, `docs/usage.md`, `llms.txt`,
`CHANGELOG.md`) and the `--project` requirement in three, with
none owning the rule. Repair: one owner, pointers behind. Deferred
per `/review` (a consolidation is its own commit); worth deciding
whether the clap help can be a pointer at all, since it is what
`bombyx list --help` prints.

### rt-2026-09-07-snapshot-outlives-the-deploy-key

**Category:** Behaviour defect deferred deliberately

`bootstrap.sh` deletes the deploy key when the config names none,
but `Action::Up` takes the `fresh-install` snapshot *after*
provisioning (so the key is on that disk), `Action::Reset` only
restores the snapshot and never re-runs `bootstrap.sh`, and
`save_snapshot_if_absent` never refreshes it -- so a revoked key
returns on every `reset` for the VM's life. Verified on the VM
host, 2026-09-07. Deferred to issue #57: closing it changes the
reset lifecycle for every project. `docs/trust-boundary.md` under
**What this costs** states the limit and names `bombyx snapshot`
and `bombyx destroy` as what removes the key.

### rt-2026-09-06-two-program-tool-case-has-no-test

**Category:** Test coverage declined deliberately

`check_not_an_option` in `crates/bombyx/src/config/guards.rs`
renders `tool: &str` into "which {tool} would treat as an option",
and no test passes two program names (the one that did was deleted
with its circular comment). All four call sites pass one word.
Declined: the merged test pins the whole message over a real
caller; what stays open is that nothing refuses a future two-name
caller, which would read ungrammatically.

### rt-2026-09-05-load-project-option-hides-two-dead-branches

**Category:** Correctness (dead code)

`Config::load_project(name, Option<&Path>)` returns
`RegistryNotFound` before any field error but
`Invalid { field: "project" }`, so `describe`'s `None` provenance
path in `crates/bombyx/src/config/host.rs` is unreachable and the
coverage gate counts an arm no test reaches. The clean fix takes
`&Path` and raises `RegistryNotFound` in `main.rs`;
`Config::load_all` now takes the same `Option<&Path>`, so it is
two functions. Deferred by the operator on #18: a public-signature
change with no user-visible effect.

### rt-2026-09-03-todo-md-unclassified-for-never-sync

**Category:** An incomplete set

`xtask/src/sync.rs`'s `NEVER_SYNC` lists the reviewer backlogs,
the changelog, the feedback file, the backfeed ledger and
`docs/issues/`, but not `docs/todo.md`, which `cargo xtask todo`
writes per project. Upstream rustbase accumulates its own
`docs/todo.md`, so a `/template-sync` would offer it. Deferred:
adding it is a workflow decision, not a defect in the set.

### rt-2026-09-03-sync-status-column-narrower-than-a-rename

**Category:** Output formatting

`xtask/src/sync.rs` formats the candidate table's status column as
`{:<3}`, but a rename status is four characters (`R100`), so the
row runs one column wide and the table misaligns from there on.
`{:<4}` fixes it. Deferred: cosmetic, and only on a diff
containing a renamed file.

### rt-2026-09-03-commit-message-cites-unrecorded-id

**Category:** An ID that does not grep

`abee0a5`'s message says it "resolves
rt-2026-09-03-implement-pre-launch-step-unclaimed and removes it
from the backlog", but that ID is in no revision and no file, and
the same commit removed nothing from any backlog -- so a reader
cannot tell whether the entry was removed, never written, or is
still open. Deferred: the claim is in a landed commit and
`/review` never amends. Either write the finding here and delete
it in one later commit, or correct the record when `implement.md`
next changes.

### rt-2026-09-02-home-does-not-isolate-ssh-config

**Category:** A comment asserting a property the platform does not give

`doctor_fails_and_says_which_check_failed` sets `HOME` and
`USERPROFILE` to the fixture and claims that stops `ssh` reading
the operator's `~/.ssh/config`, but OpenSSH on Unix takes the home
directory from the passwd entry, not `$HOME` (measured:
`ssh -G <alias>` ignored a fixture `ssh_config`). The isolation
works only on the Windows port, so the test inherits a `Host *`
`ProxyCommand` on Linux and macOS. A stub `ssh` first on `PATH` is
the lever that works on both.
