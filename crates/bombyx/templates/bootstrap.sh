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
# It runs as root. After it hands over to the project's own
# script, everything in this VM is assumed untrustworthy.

set -euo pipefail

: "${BOMBYX_REPO:?bombyx: BOMBYX_REPO is not set}"
: "${BOMBYX_REF:?bombyx: BOMBYX_REF is not set}"
: "${BOMBYX_SCRIPT:?bombyx: BOMBYX_SCRIPT is not set}"

readonly CLONE_DIR=/opt/project

# The agent works as this user. Everything here runs as root, so
# without the chown further down, the cloned project would be
# owned by root and the agent could read it but not change it --
# a VM built for editing code, in which the code is read-only.
readonly OWNER=vagrant

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

# If the clone came from a different repository than the one we
# are being asked for, throw it away rather than fetching over
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
# a cosmetic edit to bombyx.toml would be indefensible.
#
# And it only acts on a definite mismatch. If `git remote
# get-url` fails for any reason, that is "cannot tell", not
# "different", so the clone stays.
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
                "but bombyx.toml asks for $BOMBYX_REPO." >&2
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
if [ -d "$CLONE_DIR/.git" ]; then
    git -C "$CLONE_DIR" fetch --depth 1 origin -- "$BOMBYX_REF"
    git -C "$CLONE_DIR" checkout --force FETCH_HEAD
    # Deliberately no `git clean` here. It would make the
    # checkout match the repository exactly, but it deletes
    # untracked files -- which in this VM means whatever the
    # agent has been working on and not yet committed. Losing
    # that on a `bombyx provision` is worse than carrying a
    # file the upstream repo deleted. The case that actually
    # mattered, a changed `repo`, is handled above by removing
    # the clone outright.
else
    git clone --depth 1 --branch "$BOMBYX_REF" \
        -- "$BOMBYX_REPO" "$CLONE_DIR"
fi

chown -R "$OWNER:$OWNER" "$CLONE_DIR"

cd "$CLONE_DIR"

# Missing is the ordinary mistake, so check for it first and
# say so plainly. Reporting "the file is not there" as an
# attempted escape would send somebody hunting a symlink that
# does not exist.
if [ ! -e "$BOMBYX_SCRIPT" ]; then
    echo "bombyx: $BOMBYX_SCRIPT is not in the cloned" \
        "project. Check \`script\` in bombyx.toml." >&2
    exit 1
fi

# Now check where it really points before touching it.
#
# bombyx checks the *config value* -- no leading slash, no `..`
# -- but that says nothing about what the repository put at that
# path. `chmod` and `exec` both follow symlinks, so a repo could
# ship `vagrant/provision.sh` as a link to, say, /etc/shadow and
# we would `chmod +x` a system file as root. A symlinked parent
# directory does the same thing less obviously, which is why
# this resolves the whole path rather than checking one link.
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
# That is not tidiness. `chown` above hands this whole tree to
# the agent's user, so on a re-provision of a running VM that
# user can replace the script with a symlink at any moment. If
# `chmod` and `exec` resolved the name a second time, they would
# resolve whatever is there *now* rather than the thing that was
# just checked, and the check would guard nothing. Resolving
# once and using the answer closes that window.
if [ ! -x "$script_real" ]; then
    chmod +x "$script_real"
fi

exec -- "$script_real"
