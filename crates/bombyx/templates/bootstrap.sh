#!/usr/bin/env bash
#
# This script runs INSIDE the guest VM, not on your machine.
# The Vagrantfile that bombyx generates points at it.
#
# It is the same file for every project. bombyx never edits it
# or pastes anything into it -- it is copied across exactly as
# you see it here. Everything that changes per project arrives
# as an environment variable, set by Vagrant.
#
# That is a deliberate rule, not a coincidence. Building a shell
# script by pasting config values into it is how you get quoting
# bugs, and worse, how a config value ends up being run as a
# command. Keeping this file fixed means there is nothing to get
# wrong. See docs/trust-boundary.md.
#
# It starts as root, and by the end almost nothing here needs
# to be. Two things do: clearing a key an earlier bombyx left
# in /root/.ssh, and being able to drop privilege in the first
# place. Everything else -- the clone, every git command, the
# project's own script -- runs as the unprivileged OWNER
# declared below, which is the account the agent works as.
#
# Root touches a path in that account's home exactly once, on
# the refusal path below, and the comment there says why `rm`
# is safe where `chmod` would not be.
#
# Past the hand-over at the end of this file, everything in
# this VM is assumed untrustworthy.

# Three separate settings, and each one turns a silent failure
# into a loud one:
#
#   -e            stop at the first command that fails, instead
#                 of carrying on with the next line
#   -u            treat reading an unset variable as an error,
#                 instead of substituting an empty string
#   -o pipefail   let a pipeline fail when *any* command in it
#                 fails, not only the last one
#
# Without -u, a typo like $BOMYX_REF expands to nothing, and
# `git clone --branch ""` fails somewhere far from the typo.
set -euo pipefail

# `${VAR:?message}` means: expand VAR, but if it is unset or
# empty, print the message and exit. The leading `:` is the
# shell's do-nothing command -- it evaluates its arguments and
# returns success -- so each line here is a bare check that the
# variable arrived, with nothing else happening.
#
# Vagrant sets these three from the operator's config. If one
# is missing, the reason is a bug in the generated Vagrantfile,
# and failing here names the variable instead of failing later
# inside `git`.
: "${BOMBYX_REPO:?bombyx: BOMBYX_REPO is not set}"
: "${BOMBYX_REF:?bombyx: BOMBYX_REF is not set}"
: "${BOMBYX_SCRIPT:?bombyx: BOMBYX_SCRIPT is not set}"


# The agent works as this user, and two things here depend on
# that.
#
# The chown further down hands it the clone. Without that, the
# tree would stay owned by root and the agent could read it but
# not change it -- a VM built for editing code, in which the
# code is read-only.
#
# And the `exec` at the end of this file drops to it before
# running the project's script, so whatever that script installs
# lands in this user's home rather than in root's.
readonly OWNER=vagrant

# THE DEPLOY KEY, when the operator's config named one.
#
# A private repository needs a credential inside the guest, and
# this is how it arrives. Before this script runs, the
# Vagrantfile has already had Vagrant upload the key to
# DEPLOY_KEY below. That path is fixed, so nothing about it is
# pasted into this file -- see the header.
#
# WHETHER a key was configured arrives as BOMBYX_DEPLOY_KEY,
# which the Vagrantfile sets to `1` or `0` on every render, and
# is never read off this filesystem. The upload lands in the
# `vagrant` user's own .ssh directory, and that user is the one
# the agent works as -- so testing for the file would let the
# guest answer a question about the operator's config. A
# leftover from an interrupted provision, or one `touch` from
# inside the VM, would keep reinstalling a credential the
# operator had already removed.
#
# The guest cannot forge the variable either way, because a
# provisioner's `env:` becomes a prefix on the command line and
# is applied after any /etc/profile.d has been sourced. It is
# read as `${VAR:-}` all the same: `set -u` above makes an
# unset variable fatal, and the one thing that could leave it
# unset is a Vagrantfile older than this script. Defaulting to
# empty takes the deleting branch, which is the safe answer to
# "these two files disagree".
#
# NOTHING IN THIS FILE MAY EXIT BEFORE THE KEY IS DEALT WITH.
# Vagrant has already uploaded it by the time this script runs,
# so an exit above the line that tightens or removes it leaves a
# credential in the agent's own directory for the life of a VM
# that never finished provisioning. The git check below and the
# `runuser` refusal are both ordinary ways to reach such an
# exit, and both sit after this. Nothing here needs git.
readonly DEPLOY_KEY=/home/vagrant/.ssh/bombyx-deploy-key

# runuser, AND WHERE IT IS.
#
# `runuser -u NAME -- cmd args` runs `cmd` as NAME. It is a
# root-only tool and needs no sudoers entry, so it works on a
# box with `sudo` locked down. It sets HOME, USER, LOGNAME and
# SHELL for the target user, leaves the working directory
# alone, and passes the rest of the environment through -- which
# is what the BOMBYX_* variables above depend on.
#
# It is resolved here, before anything is installed or cloned,
# because several steps below need it and the `exec` at the end
# of this file cannot happen without it.
#
# `command -v` searches PATH and runuser lives in /usr/sbin --
# a root-only tool in a root-only directory. A root login shell
# has /usr/sbin on PATH, which is what Vagrant's provisioner
# gives us, but a root environment arranged some other way need
# not. So the absolute path is tried as well.
#
# The message then says both halves, in the order they are
# likely: what was searched, because a PATH missing /usr/sbin
# is the usual fault, and the package name second for the box
# where runuser genuinely is not installed.
runuser_bin=$(command -v runuser 2>/dev/null || true)
if [ -z "$runuser_bin" ] && [ -x /usr/sbin/runuser ]; then
    runuser_bin=/usr/sbin/runuser
fi

# Refusing here rather than where `runuser_bin` was resolved,
# under the rule above: a box without it gets the key removed
# first and the refusal second.
#
# Root does that removal, and it is safe. `rm` unlinks the name
# it is given and never follows a symlink at the final
# component, so there is no target the agent could redirect it
# to. Tightening the *mode* is the operation that must not run
# as root, because `chmod` does follow symlinks -- which is why
# there is nothing to do on this path but delete and refuse.
if [ -z "$runuser_bin" ]; then
    rm -f "$DEPLOY_KEY" /root/.ssh/bombyx-deploy-key
    echo "bombyx: runuser was not found on PATH ($PATH) or at" \
        "/usr/sbin/runuser. It ships with util-linux. Install" \
        "it, or choose a box that has it, so the project's" \
        "script can run as $OWNER rather than as root." >&2
    echo "bombyx: any uploaded deploy key has been removed" \
        "from this guest." >&2
    exit 1
fi

if [ "${BOMBYX_DEPLOY_KEY:-}" = 1 ]; then
    # The config named a key, so the upload must have happened.
    # Failing here rather than carrying on: a missing file
    # means the key went away on the VM host after bombyx
    # checked for it, and the alternative is a clone that
    # authenticates with nothing and a guest that reports
    # success.
    if [ ! -f "$DEPLOY_KEY" ]; then
        echo "bombyx: the configured deploy key did not arrive" \
            "at $DEPLOY_KEY. Check it is still on the VM" \
            "host and re-run." >&2
        exit 1
    fi

    # The key is tightened where it landed, not moved out of
    # the agent's reach.
    #
    # The agent has to be able to use it. Committing inside the
    # guest does not survive a provision -- the note on the
    # checkout below explains why -- so pushing is how work
    # leaves this VM, and pushing needs this key. A root-owned
    # key would mean the agent could not push at all.
    #
    # What tightening buys, then, is only that no *other* user
    # in the guest can read it. Two cases make it necessary,
    # and neither is the box's umask -- `scp` sends the source
    # file's own mode and a umask can only clear bits, so the
    # uploaded key is never looser than the key on the VM host:
    #
    #   - a loosely-permissioned key on the VM host, delivered
    #     as-is;
    #   - a file already at this path, whose mode `scp` does
    #     not touch at all, so a world-readable leftover stays
    #     world-readable until this line runs.
    #
    # The agent's own code can read it either way, and that is
    # deliberate -- docs/trust-boundary.md accounts for it.
    #
    # THE chmod RUNS AS $OWNER, NOT AS ROOT, and that is the
    # whole reason runuser is resolved above. `chmod` follows
    # symlinks, and this path is inside a directory $OWNER
    # owns. Root running it there would let a re-provisioned
    # guest replace the file with a link to any file it likes
    # and have root change that file's mode instead. Run as
    # $OWNER the operation carries exactly the authority of the
    # account that owns the directory, so a symlink buys
    # nothing. Vagrant's file provisioner uploads as that same
    # user, so the file already arrives owned by it and there is
    # nothing for a chown to do.
    "$runuser_bin" -u "$OWNER" -- chmod 600 "$DEPLOY_KEY"

    # An earlier bombyx put the key in root's own directory.
    # A guest built by one still has it there, and nothing else
    # would ever remove it -- so "taking `deploy_key` out of the
    # config takes the key out of the guest" would be false on
    # every VM that already exists. Root does this one, which is
    # safe because /root/.ssh is root's own: there is no
    # directory the agent could leave a symlink in.
    rm -f /root/.ssh/bombyx-deploy-key

    # Then `git` is told to use it. GIT_SSH_COMMAND is what git
    # passes to `ssh` for every connection it makes, and four
    # options go with the key:
    #
    #   -i <key>                          use this identity
    #   IdentitiesOnly=yes                and no other one found
    #   -F /dev/null                      ignore every ssh_config
    #   StrictHostKeyChecking=accept-new  trust the git host on
    #                                     first sight
    #
    # The third is what makes the second true: `IdentitiesOnly`
    # does not exclude an identity named by an `IdentityFile`
    # line in a config file, so without it a box shipping its
    # own /etc/ssh/ssh_config could authenticate with a key
    # nobody here chose.
    #
    # The fourth trades a first-contact check for a clone that
    # runs unattended. `accept-new` records the git host's key
    # the first time it is seen and refuses a change afterwards;
    # the strict default would stop at a prompt no operator is
    # there to answer.
    ssh_opts="-o IdentitiesOnly=yes -F /dev/null"
    ssh_opts="$ssh_opts -o StrictHostKeyChecking=accept-new"
    export GIT_SSH_COMMAND="ssh -i $DEPLOY_KEY $ssh_opts"
else
    # Taking `deploy_key` out of the config takes the
    # credential out of the guest on the next provision. Left
    # here, it would be a key nothing points at and nobody
    # remembers granting.
    #
    # Anything at that path in this branch is either a
    # leftover from an interrupted provision or something the
    # guest put there itself, and neither is a key the operator
    # asked for.
    "$runuser_bin" -u "$OWNER" -- rm -f "$DEPLOY_KEY"
    rm -f /root/.ssh/bombyx-deploy-key

    # And the clone stops pointing at it. Leaving
    # `core.sshCommand` behind is the mirror of the key nothing
    # points at: it names a deleted identity, and
    # `IdentitiesOnly=yes` with `-F /dev/null` stops git
    # falling back to one the agent does hold, so every fetch
    # and push would fail with an ssh error naming nothing
    # about bombyx.
    #
    # The unsetting itself happens further down, after the
    # chown that normalises ownership and after the `git` check
    # -- git as $OWNER refuses a repository it does not own, and
    # a tree an earlier bombyx left root-owned files in is
    # exactly the case this matters for.
fi

# WHERE THE PROJECT IS CLONED, read from $OWNER's passwd entry
# rather than written out.
#
# `getent passwd NAME` prints that account's line from whatever
# the system uses for accounts, and field six of it is the home
# directory. So this is right whatever the box calls its SSH
# user, where a literal `/home/vagrant` would only be right on
# the boxes that happen to use that name.
#
# `$HOME` would be the obvious thing and is wrong: this script
# runs as root, so `$HOME` is `/root` -- which is precisely the
# mistake that put a toolchain in root's home and started this
# whole line of work.
#
# The clone belongs in that home for one blunt reason. It used
# to live in `/opt/project`, and `/opt` belongs to root, so
# every provision had root create, remove and chown a directory
# the agent then owned -- the last root operations on an
# agent-owned tree, and the shape a symlink turns into an
# escalation. In the agent's own home the agent does all three
# itself and root does nothing to the tree at all.
#
# The name is fixed rather than the project's, so nothing about
# the operator's config is pasted into this file -- see the
# header.
owner_home=$(getent passwd "$OWNER" | cut -d: -f6)
if [ -z "$owner_home" ]; then
    echo "bombyx: no passwd entry for $OWNER in this box, so" \
        "there is no home directory to clone into. Choose a" \
        "box whose SSH user is $OWNER." >&2
    exit 1
fi
readonly CLONE_DIR="$owner_home/project"

# Base images do not all come with git installed. Without this
# check you would get a bare "command not found" from a script
# running as root inside a VM, which tells you nothing about
# which file to go and fix.
if ! command -v git >/dev/null 2>&1; then
    echo "bombyx: git is not installed in this box." >&2
    echo "bombyx: install it in the box, or choose one with" \
        "git, so the guest can clone the project." >&2
    exit 1
fi

# If the clone came from a different repository than the one
# bombyx was asked for, throw it away rather than fetching over
# it.
#
# Pointing the existing clone at the new URL and fetching is not
# enough, and the way it fails is nasty. A fetch updates the
# files the new repo has; it does not delete files only the old
# repo had. So the directory ends up holding a mixture of the
# two, and if the old repo had a provisioning script where the
# new one does not, the guest runs the OLD repo's script and
# reports success. A wrong answer that looks right.
#
# Two things this is careful about, because discarding the clone
# also discards whatever the agent has not committed.
#
# It compares loosely. The same repository can be written more
# than one way -- with or without a trailing `.git`, with or
# without a trailing slash -- and deleting somebody's work over
# a cosmetic edit to the config would be indefensible.
#
# And it only acts on a definite mismatch. If `git remote
# get-url` fails for any reason, that is "cannot tell", not
# "different", so the clone stays.
#
# `${VAR%text}` expands VAR with `text` removed from the END, if
# it is there, and leaves it alone if it is not. So `${1%/}`
# drops a trailing slash and `${a%.git}` then drops a trailing
# `.git`, in that order, which turns all four spellings of the
# same address into one string to compare:
#
#   https://host/p.git   https://host/p.git/
#   https://host/p       https://host/p/
same_repo() {
    a=${1%/}; a=${a%.git}
    b=${2%/}; b=${b%.git}
    [ "$a" = "$b" ]
}

# ROOT DOES NOTHING TO THIS TREE. It sits in $OWNER's own home,
# so the agent creates it, removes it and owns everything in
# it, and every `git` command below runs as $OWNER.
#
# That is not a tidiness question. Root running `git` in a tree
# the agent owns is a root escalation:
#
# git normally refuses to parse a repository owned by another
# user. That guard does not apply here. From `safe.directory` in
# git-config(1): a git process running as root under `sudo` also
# trusts the uid in `SUDO_UID`, and Vagrant runs this script
# through `sudo` with the agent's uid recorded there. So root
# would read the agent's `.git/config` and run its hooks. A
# `post-checkout` hook planted by the agent was measured
# executing as `uid=0` on the next provision.
#
if [ -d "$CLONE_DIR/.git" ]; then
    if current_url=$("$runuser_bin" -u "$OWNER" -- \
        git -C "$CLONE_DIR" remote get-url origin 2>/dev/null)
    then
        if ! same_repo "$current_url" "$BOMBYX_REPO"; then
            # Announced, never silent. This throws away
            # uncommitted work, and an operator who sees a fresh
            # clone with no explanation has no way to know why.
            echo "bombyx: this VM holds a clone of $current_url" \
                "but the config asks for $BOMBYX_REPO." >&2
            echo "bombyx: discarding the clone and starting" \
                "again. Uncommitted work in $CLONE_DIR is lost." >&2
            "$runuser_bin" -u "$OWNER" -- rm -rf "$CLONE_DIR"
        fi
    fi
fi

# This script runs more than once. Vagrant runs it when the VM
# is first created, and again on `vagrant provision`, which is
# what `bombyx provision` triggers.
#
# So it has two jobs: clone the project the first time, and
# fetch the latest changes every time after that. If it only
# handled the first case, `bombyx provision` would do nothing.
#
# The directory is tested again rather than reusing the answer
# from above, because the block above may have just deleted it.
#
# THE `--` SEPARATOR, argued once for the whole file. Every
# command here that is handed a value bombyx did not write puts
# one in front of it, and so does every `runuser` call above.
# It tells the program that everything after it is a value and
# never an option, so a ref named `--upload-pack=/bin/sh` is
# read as a branch name rather than as an instruction naming a
# program to run on the other end. bombyx also refuses such a
# value when it reads the config, so each `--` here is the
# second of two guards; see `check_not_an_option` in
# `config/guards.rs` for why both are kept.
#
# `FETCH_HEAD` is a file git writes during a fetch, naming the
# commit that fetch just brought down. Checking it out is how
# you land on exactly what was fetched. Using `$BOMBYX_REF` in
# the checkout instead would resolve the name a second time,
# and a `--depth 1` fetch does not create a local branch for it
# to resolve to.
if [ -d "$CLONE_DIR/.git" ]; then
    "$runuser_bin" -u "$OWNER" -- \
        git -C "$CLONE_DIR" fetch --depth 1 origin -- "$BOMBYX_REF"
    "$runuser_bin" -u "$OWNER" -- \
        git -C "$CLONE_DIR" checkout --force FETCH_HEAD
    # Deliberately no `git clean` here. It would make the tree
    # match the commit exactly, but it deletes untracked files
    # -- which in this VM means whatever the agent has been
    # working on and not yet committed.
    #
    # What that costs is a tree that is a superset of the
    # commit: build output and generated files stay behind. It
    # is not a tree that disagrees about tracked files, because
    # `--force` above already deletes a tracked file the new
    # commit does not have. Stale leftovers are a fair price for
    # not deleting the agent's work.
    #
    # It narrows the loss rather than removing it, in two ways
    # worth knowing.
    #
    # `--force` overwrites an untracked file when the fetched
    # commit carries one at the same path; git refuses that only
    # without `--force`. So an agent's `notes.md` survives until
    # upstream adds a `notes.md`, and then it goes silently.
    #
    # And checking out `FETCH_HEAD` detaches HEAD. A commit the
    # agent makes after that sits on no branch, and the next
    # provision moves HEAD away from it: `git log` stops showing
    # it and only the reflog can find it. Committing inside the
    # guest is therefore not a way to survive a provision --
    # pushing is.
else
    "$runuser_bin" -u "$OWNER" -- \
        git clone --depth 1 --branch "$BOMBYX_REF" \
        -- "$BOMBYX_REPO" "$CLONE_DIR"
fi

# The agent pushes with the same key this script fetched with,
# so the clone records how to reach it. Without this the agent
# has a key it is allowed to read and no way to know where it
# is, and `git push` falls back to whatever identity `ssh`
# finds -- which in a fresh guest is none.
#
# Decided by BOMBYX_DEPLOY_KEY and never by GIT_SSH_COMMAND.
# The second is inherited, so a guest that exports it from
# /etc/profile.d could re-pin the clone to a key the operator
# had just removed from the config -- the header above says why
# the flag is the only trustworthy answer to "was a key
# configured".
#
# `--replace-all` because a plain set refuses with "cannot
# overwrite multiple values" and exits 5 once the key has two,
# which `set -e` would turn into a provision aborted after the
# clone and the fetch had already run. A project script doing
# `git config --add core.sshCommand` reaches that by accident.
if [ "${BOMBYX_DEPLOY_KEY:-}" = 1 ]; then
    "$runuser_bin" -u "$OWNER" -- \
        git -C "$CLONE_DIR" config --replace-all \
        core.sshCommand "$GIT_SSH_COMMAND"
else
    # The mirror: no key, so the clone must not keep pointing
    # at one. `--unset-all`, because `--unset` against two
    # values warns, exits 5 and removes nothing -- which is
    # indistinguishable from the exit 5 that means there was
    # nothing there. Only that code is tolerated.
    "$runuser_bin" -u "$OWNER" -- \
        git -C "$CLONE_DIR" config --unset-all core.sshCommand \
        || { rc=$?; [ "$rc" = 5 ]; }
fi

# No chown anywhere. The tree is in $OWNER's home and every
# command that made it ran as $OWNER, so it belongs to that
# user by construction rather than by correction.

cd "$CLONE_DIR"

# Missing is the ordinary mistake, so check for it first and
# say so plainly. Reporting "the file is not there" as an
# attempted escape would send somebody hunting a symlink that
# does not exist.
if [ ! -e "$BOMBYX_SCRIPT" ]; then
    echo "bombyx: $BOMBYX_SCRIPT is not in the cloned" \
        "project. Check \`script\` in your config." >&2
    exit 1
fi

# Now check where it really points before touching it.
#
# bombyx checks the *config value* -- no leading slash, no `..`
# -- but that says nothing about what the repository put at that
# path. `chmod` and `exec` both follow symlinks, so a repo could
# ship `vagrant/provision.sh` as a link to, say, /etc/passwd and
# `chmod +x` would land on a system file. It runs as $OWNER, so
# that reaches only what $OWNER could already reach -- but a
# containment check the repository cannot talk its way past is
# worth having whoever runs it. A symlinked parent directory
# does the same thing less obviously, which is why this
# resolves the whole path rather than checking one link.
script_real=$(readlink -f -- "$BOMBYX_SCRIPT")
clone_real=$(readlink -f -- "$CLONE_DIR")
case "$script_real" in
    "$clone_real"/*) ;;
    *)
        echo "bombyx: $BOMBYX_SCRIPT points outside the cloned" \
            "project; refusing to run it." >&2
        exit 1
        ;;
esac

if [ ! -f "$script_real" ]; then
    echo "bombyx: $BOMBYX_SCRIPT is not a regular file." >&2
    exit 1
fi

# From here on, use `$script_real` and never `$BOMBYX_SCRIPT`.
#
# That is not tidiness. The `chown` above hands this whole tree
# to the agent's user, so on a re-provision of a running VM that
# user can replace the script at any moment. If `chmod` and
# `exec` resolved `$BOMBYX_SCRIPT` a second time, they would
# follow whatever chain of symlinks is in place *now* rather
# than the one that was just checked, and the containment check
# would guard nothing.
#
# Resolving once removes the ability to redirect the path
# through a symlinked parent directory after the check. The
# final component is not covered by that: the agent owns the
# file and can replace it with a symlink between the check and
# the use. Both lines that use it below run as $OWNER, so a
# symlink there reaches only what the agent could reach anyway.
# The containment check above runs as root and only reads.

# Unconditional rather than guarded by `test -x`, because
# `chmod +x` is idempotent and root's `test -x` answers a
# different question: it is true when *any* of the three
# execute bits is set, while the `exec` below needs the one for
# $OWNER. A file at mode 0011 would pass the test and fail the
# hand-over.
"$runuser_bin" -u "$OWNER" -- chmod +x "$script_real"

# `exec` replaces this script's process with the project's
# script rather than starting a second one beside it. So the
# project's script inherits this process, and its exit status is
# what Vagrant sees -- nothing here runs afterwards to swallow a
# failure.
#
# `runuser -u NAME --` drops from root to that user first, and
# it is the whole point of this line. Almost nothing above this
# needed root either -- the header lists the three things that
# did. The project's own script is not one of them, and running it
# as root would put whatever it installs -- a rust toolchain, a
# node toolchain, an agent's own configuration -- into /root
# rather than into the home directory of the account the agent
# logs in as. The agent would then find none of it.
#
# `runuser` rather than `sudo`: it is a root-only tool that
# needs no sudoers entry, so a box with sudo locked down still
# works. It sets HOME, USER, LOGNAME and SHELL for the target
# user and passes the rest of the environment through, which is
# what the BOMBYX_* variables above need.
#
# Root is still reachable from the project's script through
# `sudo`, which every Vagrant box configures for this user. That
# is the right shape: the script asks for root where it needs
# it, rather than having it throughout.
exec -- "$runuser_bin" -u "$OWNER" -- "$script_real"
