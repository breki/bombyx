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
# Vagrant sets these three from bombyx.toml. If one is missing,
# the reason is a bug in the generated Vagrantfile, and failing
# here names the variable instead of failing later inside `git`.
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
# a cosmetic edit to bombyx.toml would be indefensible.
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
exec -- "$script_real"
