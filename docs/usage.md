# Using bombyx

This is the command reference for a working setup: bombyx
installed and a `config.toml` with a table for your project.
[../README.md](../README.md) and [tutorial.md](tutorial.md) cover
getting there, and the README's **Use** section is the short
version. For why bombyx behaves the way it does -- the
file-writing design, the config validation rules, the `--dry-run`
shell internals -- see [trust-boundary.md](trust-boundary.md) and
[architecture.md](architecture.md).

The examples use the README's config: a host alias `vmhost` and a
project `myproject`.

**Every command but `list` and `self-update` takes the project as
its first argument**, as in `bombyx up myproject`. bombyx reads
nothing out of the project's own directory, so it cannot work out
which project you mean from where you are standing.
`--config <path>` names a registry file other than the
one in your config directory -- point it only at a file you
trust, because a config can name a key to copy into the VM
([trust-boundary.md](trust-boundary.md) explains).

- [Commands](#commands)
- [up and provision](#up-and-provision)
- [Rotating a secret](#rotating-a-secret)
- [reset and snapshot](#reset-and-snapshot)
- [destroy and discard](#destroy-and-discard)
- [list](#list)
- [doctor](#doctor)
- [--dry-run](#--dry-run)

## Commands

```bash
bombyx doctor myproject     # check the preconditions, change nothing
bombyx up myproject         # write the generated files, boot the VM
bombyx provision myproject  # re-run provisioning in the guest
bombyx shell myproject      # open a shell inside the VM
bombyx status myproject     # vagrant status on the host
bombyx reset myproject      # restore the fresh-install snapshot
bombyx snapshot myproject   # replace the fresh-install snapshot
bombyx down myproject       # halt the VM
bombyx destroy myproject    # destroy the VM and remove its dir

bombyx scratch myproject pr-1234  # boot a throwaway VM
bombyx discard myproject pr-1234  # destroy it

bombyx list                 # every registered project and its VM state
```

There are two lifecycles, on purpose:

- **Persistent** (`up`/`down`) for your own projects -- warm
  caches, fast boots, rolled back by snapshot.
- **Ephemeral** (`scratch`/`discard`) for untrusted code --
  external PRs, unfamiliar dependencies. Nothing survives, which
  is the point.

A scratch VM lives in `<remote_root>/scratch/<project>/<name>`, so
the same name in two projects does not collide.

On a Windows guest (`guest = "windows"`), `shell` opens PowerShell as
the agent's account, in its clone. It always exits 0 there, because
Windows' sshd reports 0 for a session with a terminal, whatever the
session exited with.

## up and provision

`up` boots the VM: it writes the generated files, boots with
`vagrant up`, and on the *first* boot takes the `fresh-install`
snapshot that `reset` returns to. The box downloads on the first
`up` and later boots are quick. Provisioning runs only on that
first `up`.

Because `up` provisions only when it first creates a VM, a second
`up` after you change your provisioning script boots the existing
machine and reports success without re-running it. Use `provision`
instead:

```bash
bombyx provision myproject
```

`provision` re-runs the bootstrap in the guest, fetching
`[source]` again at your configured ref and checking it out. The
checkout keeps the agent's work. When it would overwrite an edit
to a tracked file, or an untracked file at a path the fetched
commit adds, the guest refuses and lists the files; an edit the
fetched commit does not touch carries over. Push or stash the work
from `bombyx shell`, or overwrite it on purpose:

```bash
bombyx provision myproject --discard
```

`--discard` is a flag of `provision`, and has nothing to do with
the `discard` subcommand, which destroys a scratch VM.

The guest's clone sits on a detached HEAD, meaning no branch is
checked out. Every fresh clone leaves it that way -- the first
`up`, and a provision that starts over after a change of
`source.repo` -- and so does every provision that fetches. A
commit made on that detached HEAD lands on no branch, so the next
such provision leaves it behind, with or without `--discard`. Push
it.

A branch the agent makes before committing still points at the
agent's commit after a provision. The provision moves HEAD off that
branch to the fetched commit, and `git switch <branch>` in the
guest brings the work back.

Files your `.gitignore` matches are not protected. Git treats them
as build output, so the checkout overwrites one the fetched commit
adds. Keep nothing there you cannot make again, such as an `.env`
the agent wrote by hand.

Changing `source.repo` to a different repository discards the
clone and starts over. The guest refuses that while the clone
holds uncommitted edits or untracked files git does not ignore,
unless you pass `--discard`. Ignored files are deleted with the
clone either way.

To re-run the provisioning script without fetching or checking
anything out, pass `--no-fetch`. The script then runs from the
guest's current checkout, the agent's edits included, and the
guest prints the commit it ran from. HEAD stays where it is.
`--no-fetch` is refused on a VM that has no clone yet, and on one
whose clone belongs to a repository other than `source.repo`:

```bash
bombyx provision myproject --no-fetch
```

The guest's clone holds every branch with its whole history, so
`git log`, `git blame` and a diff against another branch work
there. Each provision fetches every branch as well as `ref`. For a
repository too large for that, set `history = "shallow"` under
`[source]`: the clone then holds one commit of `ref` and no other
branch. Because `full` is the default, an existing VM whose clone
is shallow gets the rest of its history on its next provision
unless the config sets `history = "shallow"`, and on a large
repository that fetch takes a while; `--no-fetch` skips it.
Switching to `shallow` keeps the history the clone already has.

`provision` needs a VM that already exists, so run `up` first, and
it targets the project VM only; for a scratch VM the answer is
`discard` then `scratch`.

## Rotating a secret

To change a value in your `env_file`, such as an expired token,
edit the file on your workstation and run `up` or `shell`. With a
`vault`, edit the entry in KeePassXC instead; `up` and `shell`
then ask for the master password once and read every entry again.
To rotate a deploy key, replace its file, or with a vault import
the new key over the attachment with `keepassxc-cli
attachment-import -f DATABASE "ENTRY" id_ed25519 KEYFILE` (without
`-f` the import refuses an attachment that exists):

```bash
bombyx up myproject   # or: bombyx shell myproject
```

Both commands write the file over its copy in the guest,
`~/.bombyx-env` in the agent's home: `shell` before the shell
opens, and `up` once the VM is up. When the config names a
`repo_token`, they rewrite the git credential built from it too,
and when it names a deploy key, the key at
`~/.ssh/bombyx-deploy-key`. So a key you rotate reaches the guest
the same way, without a provision. Nothing is fetched or checked
out, so the work in the guest's clone is not touched. `up` on a
running VM does only this. When a rewrite fails, `up` exits
non-zero, and `shell` warns and opens the shell anyway. A `shell`
that cannot read your `env_file` or `deploy_key` on the
workstation, or unlock your `vault`, also warns and opens, and
then refreshes nothing: one failed read skips every file, not only
the one that failed.

A Windows guest is refreshed the same way. There the refresh calls
`refresh.ps1`, which provisioning installs, so a Windows VM that
has no `refresh.ps1` says to run `bombyx provision myproject`
first. Provisioning checks the clone out afresh at the configured
`ref` and refuses when that would overwrite uncommitted work, so
`bombyx provision myproject --no-fetch` is the way to install the
helpers without fetching or checking anything out.

bombyx sends each file down a pipe, through `ssh` and then
`vagrant ssh`, so the refresh itself stores nothing on the VM host.
An `up` that has to boot the VM also stages the files on the VM
host for the length of the boot, as every boot does, and refreshes
afterwards. [trust-boundary.md](trust-boundary.md) describes both
routes.

Only those copies change:

- **A copy your provisioning script made keeps the old values**,
  until the step that made it runs again. Put the copy in a script
  of its own, which writes `.env` with
  `install -m 600 "$BOMBYX_ENV_FILE" .env`, and name it as the
  project's `secrets_refreshed` hook (below). bombyx then runs it
  whenever it writes the file. A project that also uses `scratch`
  VMs, which run no hook, calls the same script from its
  provisioning script, so the two cannot drift apart. A link
  (`ln -sf ~/.bombyx-env .env`) needs no step, but it suits only a
  project that never writes to `.env` itself: a line appended
  through the link lands in `~/.bombyx-env`, and the next rewrite
  removes it. A project that checks `.env` is a plain file will
  refuse the link as well.
- **A running process keeps the values it read.** Restart it.
- **Adding `env_file`, `repo_token` or a deploy key to a project
  still needs `provision`**, because the provisioning script is
  what uses the file, and `bootstrap.sh` is what tells `git` about
  the credential and the key. `up` and `shell` do write a new key
  into the guest, but `git` uses it only once `provision` has
  pointed it there.
- **So does removing one.** `up` and `shell` send only the files
  the config names, so taking `env_file`, `repo_token` or the key
  out leaves the guest's copy in place, and `git` keeps sending a
  token or a key you meant to withdraw. `provision` deletes the
  copies the config no longer names.

### The `secrets_refreshed` hook

A project names the hook in its own table, as a path inside the
clone:

```toml
[projects.myproject.hooks]
secrets_refreshed = ".bombyx/refresh-env.sh"
```

It needs `env_file`, or a `vault` with `entries`, in the project's
`[source]` table, because the rewrite of the secrets is what it
follows; bombyx refuses a hook without one. The path:

- is relative to the clone root and holds no `..` segment;
- does not start with `-`;
- holds only letters, digits, `.`, `_`, `-` and `/`;
- names a file, so it has no final `.` and no trailing `/`;
- and, checked in the guest, does not lead out of the clone
  through a symlink.

On a Windows guest the hook is a `.ps1` file, which PowerShell runs
with `-File`. It starts from the system's own variables (such as
`SystemRoot`, `TEMP` and `USERPROFILE`), a fixed `PATH` and the two
`BOMBYX_` names. bombyx refuses a link anywhere on its path, even one
that stays inside the clone, and its whole path,
`C:\Users\<guest_user>\<project>\<hook>`, must fit Windows' default
259 characters, with the hook path itself 200 characters at most.

When it runs: whenever bombyx has written `~/.bombyx-env`. That is
after the provisioning run of the `up` that creates the VM (before
the `fresh-install` snapshot, so `reset` returns to a guest with
the copy), after the provisioning run of `provision`, and after
`up` or `shell` has rewritten the file in a running guest. In every
case the hook runs in the same guest command as a rewrite of
`~/.bombyx-env`, and only once that rewrite succeeded. In a running
guest the credential is rewritten first, so a hook that runs `git`
sees a rotated token. It runs every time,
whether the secrets changed or not, so it must be safe to run
twice.

So for a project VM the hook is the copy step, and your
provisioning script needs none of its own. Two exceptions keep one
there. `scratch` VMs do not run the hook, so a project that uses
them calls the hook script from its provisioning script as well;
the hook is safe to run twice, so the project VM paying for both is
harmless. And the hook runs *after* the provisioning script, so a
provisioning script that needs a secret during its own run reads
`$BOMBYX_ENV_FILE` rather than `.env`.

How it runs: as the agent's account, with the clone as its working
directory, through `/bin/bash`, so it needs no execute bit. Its
environment is empty apart from four variables:

| Variable | Value |
|----------|-------|
| `HOME` | the agent's home, from its passwd entry |
| `PATH` | `/usr/local/bin:/usr/bin:/bin` |
| `BOMBYX_PROJECT` | the project's name, which is also the clone's directory |
| `BOMBYX_ENV_FILE` | `$HOME/.bombyx-env`, the same name provisioning exports |

The project's `[env]` table does not reach it. Its standard input
is empty, and the guest stops it after 60 seconds: with `SIGTERM`,
then `SIGKILL` five seconds later if it ignores that.

What it prints, on either stream, goes to a temporary file in the
guest, which bombyx relays to your terminal once the hook ends:
the first 65536 bytes, with any control character shown as `?`,
and a note when there was more. So a process the hook leaves
running, such as a dev server it restarted, does not keep `shell`
waiting. Give that process its own log: what it prints after the
hook ends goes to a file that has already been removed, and that
file keeps taking disk, or memory where `/tmp` is a tmpfs, until
the process stops.

When the hook is missing, leads out of the clone, fails or runs too
long, bombyx says so and says the secrets themselves are current:
`up` and `provision` exit non-zero, and `shell` warns and opens the
shell anyway. After a first `up` the snapshot is taken all the
same, because no later `up` would take it. A hook that itself exits
with status 124 or 137 is reported as having run too long, because
those are the statuses `timeout` uses.

`--dry-run` prints the hook's command for `shell`, `provision` and
`up`; for `up` it shows the shape of a first `up`, where the hook
follows provisioning.

[trust-boundary.md](trust-boundary.md) says what the empty
environment protects and what it does not.

## reset and snapshot

`reset` rolls the project VM back to the `fresh-install` snapshot.
`up` takes that snapshot on the first boot only, after
provisioning finishes, and never overwrites it -- so it keeps
pointing at the clean install rather than whatever an agent has
done since.

When a snapshot cannot be taken -- a provider without snapshot
support, for one -- `up` warns on stderr and still succeeds. So a
`reset` that finds nothing to restore usually means that warning
went by unread.

To move the return point on purpose:

```bash
bombyx snapshot myproject
```

That replaces `fresh-install` without asking; the old return point
is gone, but the VM, its disk and its caches are untouched. Run it
when the snapshot no longer records a clean install, or when you
have reached a state worth returning to, such as after a long
dependency build.

## destroy and discard

`destroy` throws away the VM and its directory. It first prints
the resolved `<host>:<directory>`, then asks you to type the
project name:

```console
$ bombyx destroy myproject
bombyx: this destroys vmhost:~/vms/myproject
type the project name to confirm: myproject
bombyx: destroying vmhost:~/vms/myproject
```

**Read the target it prints before you type.** That target is the
part you can check against reality: which machine and which
directory your config resolved to. A name that does not match, or
no answer at all, refuses and destroys nothing.

`--yes` skips the question. `destroy` needs it wherever nobody can
answer, because it asks only when both stdin and stderr are
terminals. Under cron, in CI, from a pipe, or with stderr
redirected to a file, it refuses rather than asking a question
you cannot see or reading the answer from somewhere else. A
script started from a terminal inherits that terminal, so it
still asks. A `--dry-run` asks nothing, since it destroys nothing.

`discard` does the same for a scratch VM. Both remove the VM's
directory after destroying the VM, and both are re-runnable, so an
interrupted `up` leaves nothing stranded.

## list

`bombyx list` reports every project in your config and its VM
state:

```console
$ bombyx list
NAME        HOST             BOX                 CPUS    MEM  STATE
faraway     offsite.invalid  generic/ubuntu2204     4   8192  unknown
jutro       vmhost           generic/ubuntu2404     8  16384  not created
neverbuilt  vmhost           generic/ubuntu2204     4   8192  not created
vmtest      vmhost           generic/ubuntu2204     2   4096  running
```

Everything but `STATE` comes from your config; `STATE` comes from
`vagrant status` on the machine that owns the project. A few
things to know:

- A machine that cannot be reached leaves its own projects
  `unknown` and does not hold up the others. The reason goes to
  stderr, one line per machine, rather than into the table.
- `not created` means either no VM exists yet, or the project has
  never been `up`-ed.
- Any project left `unknown` makes `list` exit non-zero, so a
  script can tell a complete table from one with gaps.
- `bombyx list --offline` reads only your config and contacts no
  machine; it answers instantly and drops the `STATE` column.
- Scratch VMs are not listed, because no config table names them.

## doctor

Run `bombyx doctor myproject` first on a new host. It changes
nothing, runs every check rather than stopping at the first
failure, and exits non-zero if any fails:

```console
$ bombyx doctor myproject
  local   ssh               ok    OpenSSH_for_Windows_9.5p2 3.8.2 in C:\Win...
  vmhost  ssh               ok
  vmhost  login shell       ok    posix
  vmhost  vagrant           ok    /usr/bin/vagrant
  vmhost  project dir       ok    /home/igor (will create /home/igor/vms/myproject)
  vmhost  libvirt provider  ok    vagrant-libvirt (0.12.2, global)
  vmhost  host firewall     ok    active, bridge virbr1; table not read
all checks passed
```

The `libvirt provider` and `host firewall` rows appear only for a
libvirt project; a Hyper-V project shows a `provider` row and a
`host firewall` row reading `skip` instead. See
[vm-host-setup.md](vm-host-setup.md) for what to do about each
failure.

The `host firewall` row reads `skip` while the libvirt network the
firewall filters does not exist: before the first `bombyx up`
creates it, and again if vagrant-libvirt removes it as VMs are
destroyed; [vm-host-firewall.md](vm-host-firewall.md) says when it
does. While the network exists, the row fails unless the VM
host's firewall from [vm-host-firewall.md](vm-host-firewall.md) is
active and names the bridge your guests use. It runs without root,
so it cannot read the loaded rules; `sudo agent-vm-firewall status`
on the VM host does.

**Changing `provider` on a project that already has a VM does
nothing until you destroy it** -- vagrant records the provider it
built the machine with. Run `bombyx destroy myproject`, then
`bombyx up myproject`.

On a machine that is its own VM host, `doctor` checks `sh` rather
than `ssh`; [local-host.md](local-host.md) covers that route.

A project with a `vault` adds a `local keepassxc-cli` row, which
fails when `keepassxc-cli` is not on your `PATH`. On a Windows
workstation that row fails with the refusal `up` would give,
because bombyx cannot open a vault there yet. `doctor` never asks
for the master password: it checks that the program is there, not
that the database opens.

## --dry-run

Every command takes `--dry-run`, which prints the exact shell it
would run and touches nothing:

```bash
bombyx --dry-run up myproject
```

It is worth using whenever you are unsure what a command is about
to do, `destroy` above all. The plan is for reading, and for
pasting one line at a time.

A dry run still reads the project's secrets, because the plan it
prints includes the step that writes them. So with a `vault`, a
dry run of `up`, `provision`, `shell` or `scratch` asks for the
master password as a real run would.

**Do not pipe the plan into a shell.**
`bombyx --dry-run up myproject | sh` writes the generated files
empty, or not at all depending on the shell, and can leave
`vagrant up` running against an empty Vagrantfile with a zero
exit that reads as success. To run the commands, run bombyx
without `--dry-run`.
