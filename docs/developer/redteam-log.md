# Red Team Findings -- Deferred backlog

Security (Red Team) review findings. Newest first.

---

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

Verified on frosti, 2026-09-07: with the key removed from the
config and deleted from the guest, `bombyx reset` restored the
key with its original timestamp. It sat at
`/root/.ssh/bombyx-deploy-key` when that was measured.

Deferred rather than fixed. Closing it means either `reset`
re-provisioning after the restore, or `up` re-taking the
snapshot when the key set changed. Both change what bombyx does
for every project, not only one with a `deploy_key`. That is a
decision about the reset lifecycle rather than about
credentials, so it wants its own issue.

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

Deferred by the operator during the `/review2` on #18: it is a
public-signature change proposed at the end of a review that had
already stopped on non-convergence, and it has no user-visible
effect. Found as RT-5 in round 2.

### rt-2026-09-04-doc-cannot-link-a-plans-own-section

**Category:** The fix stops one step short of what it was for

`--doc` refuses a `#fragment`, so a Done entry can link to a
shared plan but not to the section of it that belongs to that
step. `docs/issues/project-config-off-repo.md` has seven steps
and every one of them would link to the same undifferentiated
file.

That is the weaker half of the improvement this change was made
for. Deriving `issues/<slug>.md` was wrong because the plan is
shared; pointing all seven siblings at the same anchor-less path
is better and still not what a reader wants.

The error no longer claims a fragment is unrenderable --
`[**slug**](issues/plan.md#step-3)` renders and resolves
perfectly well -- it now says `--doc` takes a path. So the
message is honest and the capability is absent.

Candidate fix: split `rel` on the first `#`, run the path half
through the existing rules, and keep the fragment in the
rendered destination. Needs a test row per shape and a decision
about whether a fragment naming no heading should be refused,
which nothing here can check.

Deferred: raised by red-team in round 3 of the `/review2` on #7,
which was the three-round ceiling, so it was logged rather than
fixed and re-reviewed.

### rt-2026-09-04-doc-existence-check-answers-for-this-machine

**Category:** A link vetted on one machine, dead on another

`DocLink::new` in `xtask/src/todo.rs` ends with
`docs.join(rel).is_file()`. That call follows symlinks, and on
Windows and on a default macOS volume it matches
case-insensitively. So `--doc issues/Plan.md` is accepted on
those platforms when the file is `plan.md`, and the link is dead
on GitHub and in every Linux clone.

The same rule refuses a rooted path precisely because it would
"resolve only on a machine laid out like the author's", and
`escapes_repo` deliberately counts components rather than
touching the disk so a missing directory cannot change the
verdict. The existence check then puts the verdict back on the
disk. A symlinked target is the other half: it passes
`escapes_repo`, which is lexical, and `is_file()`, which
resolves, while pointing outside the repository.

Candidate fix: match the final component against the real
directory entry with `read_dir` and an exact string compare, and
refuse a target whose `symlink_metadata` says symlink. Needs a
failing test first, and a fixture that can only be built on a
platform where the difference shows.

Deferred: raised by red-team during the `/review2` on #7 and not
fixed there. Every machine that has run this command is Linux,
so the case-insensitive half has never fired; the symlink half
needs somebody to place a symlink under `docs/` deliberately.

### rt-2026-09-04-provenance-line-names-the-default-filename

**Category:** Misleading provenance on the path `destroy` uses

`HostOrigin::Overlay` renders as the fixed literal
`bombyx.local.toml` in `crates/bombyx/src/config/host.rs`. That
Display value is now the only thing bombyx prints about the
file, because the `<local> overrides <config>` notice is gone.
Under `--config staging.toml` the host comes from
`staging.local.toml` and the line says `bombyx.local.toml`,
naming a file that supplied nothing and need not exist.
Verified against the built binary. `Config::load` already
resolves the real path for its `InvalidHost` error, so the
machinery exists; the fix is to return that path alongside
`HostOrigin`. `destroy` runs `rm -rf` on the host this line
reports, and `README.md` under **A different machine for one
project** rests its guarantee on it. `docs/usage.md` and `README.md`
both carry a marker naming this ID.

Deferred by the operator: step 2 of the config move
(`overlay-drop-host-source`, #23) deletes `bombyx.local.toml`
and this branch with it.

This does not block
`rt-2026-09-04-overlay-and-local-config-path-are-pub`, though
an earlier version of this entry said it did. The fix above
returns the path from `Config::load`, so `main.rs` never calls
`local_config_path` -- and after step 1 it calls nothing in
that module, so the narrowing is free to go ahead. Only the
alternative fix, resolving the path in `main.rs`, would need
`local_config_path` to stay `pub`.

**Closed 2026-09-04 by #23.** `bombyx.local.toml` is gone, so
nothing in bombyx reaches the code this describes.

### rt-2026-09-03-round-three-findings-have-no-durable-home

**Category:** A disposition with nowhere to go

Round three reports and fixes nothing, and `/review` logs only
deferred findings. So a round-three finding is neither fixed nor
deferred, and no rule writes it anywhere that outlives the run:
`target/review-<n>.findings` is the only record and `target/` is
not committed. Commit `d1908d6` exists because of exactly this
-- round three's findings lived only in the session and had to be
back-filled into 14 backlog entries by hand afterwards.

Deferred: the fix is either to log a round-three finding the way
a deferred one is logged, or to say that round three's report is
the developer's to act on before committing. That is a decision
about who owns the last round, and `/review` is frozen until a
run against a real code diff has exercised it.

Found by the red team review (RT-2), 2026-09-03.

---

### rt-2026-09-03-todo-md-unclassified-for-never-sync

**Category:** An incomplete set

`xtask/src/sync.rs`'s `NEVER_SYNC` now matches the reviewer
backlogs by shape and lists the diary, the changelog, the
feedback file, the backfeed ledger and `docs/issues/`.
`docs/todo.md` is not in it, and `cargo xtask todo` writes that
file per project, so it is the same kind of record as the rest.
Whether upstream rustbase accumulates its own `docs/todo.md`
was not checked -- there is no `template` remote configured
here, so confirm with `git ls-tree <upstream>:docs` before
deciding.

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

### rt-2026-09-03-review-has-no-allowed-tools

**Category:** Command definition

`.claude/commands/review.md` declares only `description`, while
`commit.md` declares an `allowed-tools` list. `/review` needs
`Bash`, `Agent`, `Edit` and `AskUserQuestion`, and a reader
cannot tell whether the omission means unrestricted or means it
inherits the session's permissions. Deferred because the right
scoping is a decision about the whole command set, not about
this file: `check.md` scopes to a single `cargo xtask` pattern,
and nothing states what the convention is for a command that
edits files and spawns agents.

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

### rt-2026-08-31-chmod-symlink-race

**Category:** CLOSED 2026-09-07 -- was TOCTOU / privilege
escalation (guest)

**Closed** by the branch that made the project's script run as
the agent. `bootstrap.sh` now runs the `chmod +x` through
`runuser -u "$OWNER"`, so root never sets an execute bit on a
path the agent controls, and the comment beside it says the
race is closed rather than accepted. The cost this entry
weighed -- losing the shebang, or losing the readability of the
one file meant to be read straight through -- was avoided by a
third option it did not consider: drop the privilege instead of
the mechanism. The entry is kept because that is the lesson.

The original entry follows.

`bootstrap.sh` resolves the configured provisioning script with
`readlink -f`, checks the result is inside the clone, then
`chmod +x`es it and `exec`s it as root. The `chown -R` a few
lines earlier gives the agent's unprivileged user ownership of
every file in the clone, so on a re-provision of a *running* VM
that user can unlink the resolved path and put a symlink there
between the resolve and the `chmod`. `chmod` follows symlinks
even on an already-resolved path, so root sets the execute bit
on a file of the attacker's choosing.

Deferred deliberately, with the trade written into the script
beside the `chmod` rather than left for a reader to work out.
Closing it costs either the shebang (drop the `chmod`, run the
script through a named interpreter, so a Python or Ruby
provisioning script stops working) or the readability of the one
file meant to be read straight through (open the file, operate
on `/dev/fd/N`).

What it yields is the execute bit alone -- not content, not a
write -- which is worth nothing on most targets and is a
privilege escalation only on a file the user can already write
to. It also requires code already executing in the VM as that
user, timing a provision.

The `exec` has the same exposure and it does not matter: the
user owns the script already, so it can write its own content
into the file rather than race. Only the `chmod` reaches a file
outside that ownership.

Found by the fresh-reader review (FR-7), 2026-08-31. Neither the
Red Team nor Artisan pass covered it.
