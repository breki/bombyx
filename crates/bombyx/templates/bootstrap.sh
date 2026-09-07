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
# It runs as root, because installing a credential and cloning
# into /opt/project need root. It hands over to the project's
# own script as the unprivileged OWNER below, which is the
# account the agent works as -- see the `exec` at the end of
# this file for why. Past that hand-over, everything in this VM
# is assumed untrustworthy.

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

readonly CLONE_DIR=/opt/project

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
# THIS BLOCK IS FIRST ON PURPOSE, ahead of the git check below.
# Any `exit` above this point would leave the uploaded key
# sitting in the agent's own directory for the life of the VM,
# and a box without git is an ordinary way to reach such an
# exit. Nothing here needs git.
readonly DEPLOY_KEY=/home/vagrant/.ssh/bombyx-deploy-key

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
    # in the guest can read it: `scp` uploads at the box's
    # umask, which is world-readable on some boxes. The agent's
    # own code can read it, and that is deliberate and is the
    # exposure docs/trust-boundary.md accounts for.
    chown "$OWNER:$OWNER" "$DEPLOY_KEY"
    chmod 600 "$DEPLOY_KEY"

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
    rm -f "$DEPLOY_KEY"
fi

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
# new one does not, the guest runs the OLD repo's script, as
# root, and reports success. A wrong answer that looks right.
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

if [ -d "$CLONE_DIR/.git" ]; then
    if current_url=$(git -C "$CLONE_DIR" remote get-url origin 2>/dev/null)
    then
        if ! same_repo "$current_url" "$BOMBYX_REPO"; then
            # Announced, never silent. This throws away
            # uncommitted work, and an operator who sees a fresh
            # clone with no explanation has no way to know why.
            echo "bombyx: this VM holds a clone of $current_url" \
                "but the config asks for $BOMBYX_REPO." >&2
            echo "bombyx: discarding the clone and starting" \
                "again. Uncommitted work in $CLONE_DIR is lost." >&2
            rm -rf "$CLONE_DIR"
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
# THE `--` SEPARATOR, once for the whole file. It appears in the
# `fetch` below, in the `git clone`, and in the final `exec`. It
# tells the program that everything after it is a value and
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
    git -C "$CLONE_DIR" fetch --depth 1 origin -- "$BOMBYX_REF"
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
    git clone --depth 1 --branch "$BOMBYX_REF" \
        -- "$BOMBYX_REPO" "$CLONE_DIR"
fi

# The agent pushes with the same key this script fetched with,
# so the clone records how to reach it. Without this the agent
# has a key it is allowed to read and no way to know where it
# is, and `git push` falls back to whatever identity `ssh`
# finds -- which in a fresh guest is none.
#
# Set only when a key is configured. On a public repository
# GIT_SSH_COMMAND is unset, and writing an empty value here
# would break every `git` command the agent runs.
if [ -n "${GIT_SSH_COMMAND:-}" ]; then
    git -C "$CLONE_DIR" config core.sshCommand "$GIT_SSH_COMMAND"
fi

# Last, so it covers the config written just above as well as
# the tree.
chown -R "$OWNER:$OWNER" "$CLONE_DIR"

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
# ship `vagrant/provision.sh` as a link to, say, /etc/shadow and
# `chmod +x` would land on a system file, as root. A symlinked
# parent directory does the same thing less obviously, which is
# why this resolves the whole path rather than checking one link.
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
# What that narrows, and what it does not:
#
# Resolving once removes the ability to redirect the path
# through a symlinked parent directory after the check. What
# remains is the final component. `readlink -f` returns a path
# with no symlink in it, but the agent's user owns that file and
# can unlink it and put a symlink there before the `chmod` runs
# a few microseconds later -- and `chmod` follows symlinks. So a
# user who is already running code in this VM, and who can time
# a provision, can have root set the execute bit on one file of
# their choosing.
#
# That stays open deliberately. Closing it means either
# dropping the `chmod` and running the script through a named
# interpreter, which costs the shebang -- a Python or Ruby
# provisioning script would stop working -- or opening the file
# and working on the descriptor, which costs the readability
# this file exists to have. The execute bit alone is worth
# nothing on most targets, and the attacker needs code execution
# in the VM before any of it applies.
#
# The `exec` below has the same exposure and it does not matter:
# the agent's user already owns the script, so it can write its
# own content into the file and skip the race entirely. Only the
# `chmod` can reach a file that user does not own, which is why
# that one is the window worth naming.
if [ ! -x "$script_real" ]; then
    chmod +x "$script_real"
fi

# `exec` replaces this script's process with the project's
# script rather than starting a second one beside it. So the
# project's script inherits this process, and its exit status is
# what Vagrant sees -- nothing here runs afterwards to swallow a
# failure.
#
# `runuser -u NAME --` drops from root to that user first, and
# it is the whole point of this line. Everything above needed
# root: installing the deploy key, cloning into /opt/project,
# chowning it. The project's own script does not, and running it
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
if ! command -v runuser >/dev/null 2>&1; then
    echo "bombyx: runuser is not installed in this box." >&2
    echo "bombyx: it comes with util-linux. Install it, or" \
        "choose a box that has it, so the project's script can" \
        "run as $OWNER rather than as root." >&2
    exit 1
fi

exec -- runuser -u "$OWNER" -- "$script_real"
