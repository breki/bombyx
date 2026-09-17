# Red Team Findings -- Deferred backlog

Security (Red Team) review findings. Newest first.

---

### rt-2026-09-13-bootstrap-guards-enumerated-by-hand

**Category:** Correctness (escalated consolidation)

`crates/bombyx/templates/bootstrap.sh` gained a third uploaded
credential, and three separate hand-written lists in
`crates/bombyx/src/vagrantfile/bootstrap_tests.rs` each had to
be extended to match: the declared-before-expanded list, the
`refuse` removal assertion, and the bare-expansion list. The
review found all three lagging, and the file's own prose banners
had the same lag, counting two credentials where three exist.

Every one of them is now current. The pattern is what is
logged: each credential added to that script needs the same
four edits in three files, and nothing fails when one is
forgotten -- the extended lists were proven to bite only by
breaking the script deliberately.

`red-team` proposed one table of `(variable, literal guest
path, presence flag)` triples driving all three guards, so a new
credential is one row. That is a consolidation across three
tests and a template, so it is escalated rather than applied in
the round that found it.

Not applied because it is a refactor of test machinery with no
defect behind it today, and this branch is already large.

---

### rt-2026-09-13-cross-key-rule-count-stated-in-six-places

**Category:** Correctness (escalated consolidation)

The rules spanning more than one `[source]` key are enumerated
in `config.toml.sample`, `docs/usage.md`, `docs/architecture.md`
twice (prose and the refusal table), `llms.txt` and the diary
entry. Adding the fourth rule -- `repo` must name no username --
left five of the six stating a count of three, and the
architecture table, whose own introduction says the table is the
count, missing a row.

All six are now correct. What is logged is that a seventh rule
means six more edits, and `cargo xtask canon-check` reads only
`CLAUDE.md`, `llms.txt` and `.claude/`, so four of the six are
unchecked.

The repair is one authoritative list with the others pointing at
it. Escalated rather than applied: `/review` under **A
consolidation is escalated** says why a 6-to-1 consolidation
does not belong in the round that found it.

---

### rt-2026-09-13-no-gate-for-a-missing-doc-comment

**Category:** Correctness (missing gate)

`CLAUDE.md` under **Coding Standards** requires a doc comment on
every public item, and nothing enforces it. A scripted edit
inserted `RepoUrl::https_userinfo` between `https_host` and its
doc comment, which silently reassigned the comment to the new
function and left `https_host` undocumented and published that
way. Ten gates passed.

Measured while writing this: `RUSTFLAGS="-W missing_docs" cargo
build -p bombyx` reports zero violations once that one is fixed,
so turning the lint on in the workspace lint block is a one-line
change that costs nothing today.

Not applied here because it is a workspace-wide lint change
outside this branch's scope, and it wants its own commit.

---

### rt-2026-09-13-env-file-read-has-no-size-cap-and-a-toctou-gap

**Category:** Security (low)

`EnvFilePath::read` in `crates/bombyx/src/config/env_file.rs`
calls `std::fs::metadata` and then `std::fs::read`, and the
guard is re-resolved from the path rather than held open. Two
gaps follow.

Whoever can write the containing directory can pass the
regular-file check and be a fifo by the time `read` opens the
path, and bombyx then blocks or grows without bound. It needs a
directory the operator does not control -- `/tmp` on a
multi-user workstation is the realistic one -- so it is narrow.

There is also no upper bound on a regular file's size. A config
naming a multi-gigabyte file is read whole into memory and
copied again into a `Stdin`. `config::MAX_CONFIG_BYTES` is the
precedent for a cap.

Closing the first properly is platform-specific: `File::open`
on a fifo blocks before the handle can be asked what it is, so a
correct version needs `O_NONBLOCK` and `O_NOFOLLOW`. The size
cap is cheap on its own and would bound the damage either way.

Raised by red-team on 2026-09-13 while working issue #78 and
deferred: the `metadata` check already closes the case the
review was about -- a config naming `/dev/zero` outright -- and
the rest is a separate piece of work.

---

### rt-2026-09-11-exit-rule-has-no-single-home

**Category:** Duplicated rule deferred for its own commit

`bombyx list`'s exit-status rule is stated in five places -- the
clap help in `main.rs`, `README.md`, `docs/usage.md`, `llms.txt`
and `CHANGELOG.md` -- and the `--project` requirement in three:
`README.md`, `crates/bombyx/README.md` and `docs/usage.md`. None
of them says which copy owns the rule.

Two rounds of the review that added `list` produced prose
defects from exactly this. Round 1 wrote the exit rule into all
five; round 2 found the code implements a wider rule than any of
them states ("any project left `unknown`", not "a machine that
does not answer"), and that "`--offline` always exits zero" was
false in three of them, since a bad config file exits 1 whatever
`--offline` says. Correcting copies is what made the next
round's findings, both times.

Every copy now agrees with the code and with the others, so
nothing is wrong today. What is deferred is giving each rule one
owner and leaving pointers behind. `/review` forbids a
consolidation in the round that finds it, because N copies
become one statement plus N-1 pointers and a pointer can name
the wrong section or chain two deep -- so it wants a commit of
its own with nothing else in it.

Worth deciding at the same time: whether the clap help can be a
pointer at all. It is what `bombyx list --help` prints, so it
has to state the rule rather than refer to a document, which
means the real question is which of the four remaining copies
survive.

### rt-2026-09-07-snapshot-outlives-the-deploy-key

**Category:** Behaviour defect deferred deliberately

`bootstrap.sh` deletes the deploy key when the config names
none, so removing `deploy_key` and re-running `bombyx provision`
takes the credential out of the guest's live disk. `bombyx
reset` puts it back.

The key's path moved after this entry was written: it now lives
at `/home/vagrant/.ssh/bombyx-deploy-key`, in the agent's own
home, because the agent has to push with it. The finding is
unaffected -- a snapshot holds whatever the disk held.

`Action::Up` takes the `fresh-install` snapshot *after*
provisioning, so that snapshot's disk holds the key. `Action::
Reset` plans one command, `restore_snapshot`, and nothing
re-runs `bootstrap.sh` afterwards. `save_snapshot_if_absent`
skips a machine that already carries the name, so a later `up`
does not refresh it either. The revoked key therefore comes
back on every reset for the life of the VM.

Verified on the VM host, 2026-09-07: with the key removed from the
config and deleted from the guest, `bombyx reset` restored the
key with its original timestamp. It sat at
`/root/.ssh/bombyx-deploy-key` when that was measured.

Deferred rather than fixed. Closing it means either `reset`
re-provisioning after the restore, or `up` re-taking the
snapshot when the key set changed. Both change what bombyx does
for every project, not only one with a `deploy_key`. That is a
decision about the reset lifecycle rather than about
credentials, so it went to its own issue, #57.

What landed instead: `docs/trust-boundary.md` under **What this
costs** states the limit, and names `bombyx snapshot` and
`bombyx destroy` as what actually removes the key.

---

### rt-2026-09-06-two-program-tool-case-has-no-test

**Category:** Test coverage declined deliberately

`check_not_an_option` in `crates/bombyx/src/config/guards.rs`
takes `tool: &str` and renders it into "which {tool} would treat
as an option". No test passes two program names any more. The
test that did, `the_option_message_reads_the_same_for_one_tool_
or_two`, was deleted on 2026-09-06 along with the comment citing
it, because the comment justified the wording by that test and
the test existed to hold the wording -- the circularity a
fresh-reader finding was raised against.

Declined rather than restored. The merged test asserts the whole
message, "which ssh would treat as an option", so a rewrite to
"reads" fails it: the wording is pinned by an assertion over a
real caller instead of by a fictional one. All four production
call sites pass one word, `ssh` or `git`.

What this leaves open: nothing refuses a future caller that
passes two program names, and the message would read
ungrammatically if one did. Raised as RT-11 in the `/review2` on
the backlog sweep, 2026-09-06.

### rt-2026-09-05-load-project-option-hides-two-dead-branches

**Category:** Correctness (dead code)

`Config::load_project(name, registry: Option<&Path>)` returns
`RegistryNotFound` from `registry.ok_or_else(missing)?` before
it can produce any field error except
`Invalid { field: "project" }`, which `main.rs` catches in its
own arm. So two `None` branches in `main.rs` are unreachable:
the `"the registry"` fallback in the error-context closure, and
`describe`'s `None` path at the provenance notice. Both are code
worrying about a state that cannot occur, which makes the one
real case harder to see, and the coverage gate counts an arm no
test can reach.

The clean fix changes the signature: have `load_project` take
`&Path` and let `main.rs` raise `RegistryNotFound` itself, which
removes the `Option` and both dead branches. That needs the
wording for a machine with no config directory to be reachable
from the binary, and `config::host::registry_place` is
`pub(crate)` today.

Deferred by the operator during the review on #18: it is a
public-signature change proposed at the end of a review that had
already stopped on non-convergence, and it has no user-visible
effect. Found as RT-5 in round 2.

**Swept 2026-09-11.** Half of it is stale and half has grown.
The `"the registry"` fallback is gone from `main.rs`; the
`describe` path is still there
(`crates/bombyx/src/config/host.rs:327`). And `Config::load_all`
now takes the same `Option<&Path>` and raises `NoRegistry` the
same way, so the signature change would be two functions rather
than one.

### rt-2026-09-03-todo-md-unclassified-for-never-sync

**Category:** An incomplete set

`xtask/src/sync.rs`'s `NEVER_SYNC` now matches the reviewer
backlogs by shape and lists the diary, the changelog, the
feedback file, the backfeed ledger and `docs/issues/`.
`docs/todo.md` is not in it, and `cargo xtask todo` writes that
file per project, so it is the same kind of record as the rest.
Upstream rustbase does accumulate its own `docs/todo.md`: the
`template` remote is configured now, and
`git show template/main:docs/todo.md` at `6528907` returns 76
lines carrying upstream's own pending items.

Deferred: adding it changes what a `/template-sync` run offers,
which is a decision about the workflow rather than a defect in
the set.

Found by the red team review (RT-3), 2026-09-03.

---

### rt-2026-09-03-sync-status-column-narrower-than-a-rename

**Category:** Output formatting

`xtask/src/sync.rs` formats the candidate table's status column
as `{:<3}`. A rename status is four characters, `R100`, which
the test at the bottom of that file asserts. Rust's width is a
minimum rather than a limit, so nothing is truncated -- the row
simply runs one column wide and the table misaligns from that
row on. `{:<4}` fixes it.

Deferred: cosmetic, and it only shows on a diff containing a
renamed file.

Raised by the Fresh Reader review as a correctness matter for
the other two reviewers, 2026-09-03.

---

### rt-2026-09-03-commit-message-cites-unrecorded-id

**Category:** An ID that does not grep

`abee0a5`'s message says the `implement.md` change "resolves
rt-2026-09-03-implement-pre-launch-step-unclaimed and removes it
from the backlog". That ID exists in no revision:
`git log --oneline -S"implement-pre-launch" --all` is empty and
`grep -rn` over `docs/` and `.claude/` finds nothing. The same
commit added 14 lines to `docs/developer/redteam-log.md` and
deleted none, so nothing was removed from any backlog.

The date-slug scheme exists so that an ID greps and `git log -S`
finds both the finding and its resolution. Here the resolution
half cites an ID with no record, so a reader cannot tell whether
an entry was removed, never written, or is still open somewhere
they have not looked.

Deferred rather than fixed: the claim is in a landed commit
message, and `/review` never amends. Either write the finding
into this file and delete it in one later commit, so both halves
grep, or correct the record in the commit that next touches
`implement.md`.

Found by the red team review (RT-5), 2026-09-03.

---

### rt-2026-09-02-home-does-not-isolate-ssh-config

**Category:** A comment asserting a property the platform does not give

`doctor_fails_and_says_which_check_failed` sets `HOME` and
`USERPROFILE` to the fixture and claims that stops `ssh` reading the
operator's `~/.ssh/config`. OpenSSH on Unix takes the home directory
from the passwd entry, not from `$HOME`. Measured: with `HOME`
pointed at a fixture whose `ssh_config` rewrites an alias,
`ssh -G <alias>` ignores it. The isolation works only on the Windows
port, so the test still inherits a `Host *` `ProxyCommand` on Linux
and macOS.

A stub `ssh` first on `PATH` is the lever that works on both.
