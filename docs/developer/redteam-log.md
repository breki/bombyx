# Red Team Findings -- Deferred backlog

Security (Red Team) review findings. Newest first.


---

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

### rt-2026-09-04-a-malformed-overlay-defeats-the-host-flag

**Category:** The documented escape hatch stops working

`Config::load` in `crates/bombyx/src/config.rs` reads and
parses `bombyx.local.toml` before `resolve_host` runs, and any
problem with it is fatal. So `--host explicit` fails on a file
whose only contribution was already outranked. The same doc
comment states the opposite principle two paragraphs up for the
per-developer file: it is read only when nothing else supplied
a host, "so `--host` still works on a machine whose
per-developer file is missing or broken". The eager read was
justified while the overlay carried project fields that were
needed regardless, and step 1 removed them.

Verified against the built binary: with `remote_root` in the
overlay, `bombyx --config staging.toml --host explicit
--dry-run status` exits 1. Anyone upgrading with an existing
overlay carrying a project field loses every command until they
edit that file by hand.

The fix is to read the overlay lazily, behind the flag and
environment checks. The tradeoff is that a corrupt overlay then
goes unreported whenever a higher-precedence source won.

Deferred by the operator: step 2 (#23) deletes the file and the
branch.

### rt-2026-09-04-overlay-and-local-config-path-are-pub

**Category:** Public surface nothing public consumes

`Overlay` is `pub` with a `pub host` field, and
`crates/bombyx/src/config.rs` re-exports
`read::local_config_path`. Both were public because
`Config::with_overlay` and the deleted "overrides" notice used
them; neither exists now. `resolve_host` is `pub(crate)` and
`Config::load` builds the `Overlay` internally, so no public
API accepts, returns or hands out one. `Overlay`'s `Default`
derive lost its last caller with the test that used
`..Overlay::default()`.

Narrowing both to `pub(crate)` is a breaking library change and
would stop `Config::load`'s doc comment linking them, since
rustdoc refuses a public page pointing at a private item.

Nothing blocks this. Step 2 (#23) makes it moot if that lands
first, which is the likely outcome.

### rt-2026-09-04-a-committed-overlay-redirects-every-ssh

**Category:** Residual exposure in the config path

`bombyx.local.toml` sits inside the project directory, only
convention keeps it out of git, and it outranks the operator's
own `config.toml`. A repository that commits one redirects
every `ssh` bombyx runs, `destroy` included, to a machine of
its choosing. That is the attack `host` was removed from
`bombyx.toml` to prevent, and removing it from that file did
not close this route.

bombyx does not refuse such a file. Its only mitigation is the
provenance line, which
`rt-2026-09-04-provenance-line-names-the-default-filename`
shows can name the wrong file. `docs/usage.md` now states the
exposure rather than implying it is closed.

Candidate fixes: refuse an overlay that `git` tracks, or
require the file outside the checkout. Step 2 (#23) deletes the
file, which closes this too -- so the value here is the record
of why the file must not come back.

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

### rt-2026-09-02-three-comments-still-describe-the-push

**Category:** Stale prose about a removed capability

`remote/probe.rs` says `up` "creates the remote directory and ships a
tarball before `vagrant` fails"; `config/root.rs` justifies its depth
rule with `up` extracting "a tarball into `/etc`"; `remote/quote.rs`
explains a quoting rule with `scp` writing to the real home
directory. All three state current behaviour and all three are
false. Three sweeps missed them.

### rt-2026-08-31-chmod-symlink-race

**Category:** TOCTOU / privilege escalation (guest)

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
