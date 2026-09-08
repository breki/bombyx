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
# It runs as the box's SSH user, which is the account the agent
# works as. The generated Vagrantfile marks the provisioner
# `privileged: false`, and without that flag Vagrant would run
# it as root.
#
# bombyx needs no root inside the guest. The clone sits in this
# account's own home, so this script creates the directory, owns
# everything in it and removes it again, and it runs every git
# command below as that same account on that account's own
# files. A project that has to install packages calls `sudo`
# from its own script, which every Vagrant box configures for
# this user.
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
# unset variable fatal, and one route leaves it unset -- a
# `vagrant provision` run on the VM host by hand, in a
# directory an older bombyx wrote, where the Vagrantfile does
# not set this variable and this script expects it. bombyx
# rewrites both files together on every `up`, `provision` and
# `scratch`, so its own runs cannot produce the mismatch.
# Defaulting to empty takes the deleting branch, which is the
# safe answer to "these two files disagree".
#
# This path stays a literal and cannot come from `$HOME` the way
# `CLONE_DIR` below does. It is the `destination:` of a `file`
# provisioner in the generated Vagrantfile, and Vagrant
# evaluates that before this guest exists.
#
# Declared before anything expands it. Every refusal below
# removes the uploaded key, and `set -u` makes expanding an
# undeclared variable fatal: the message would never print and
# the key would stay.
readonly DEPLOY_KEY=/home/vagrant/.ssh/bombyx-deploy-key

# EVERY REFUSAL IN THIS FILE GOES THROUGH HERE, and that is the
# point.
#
# Vagrant uploads the deploy key before this script starts. So a
# refusal that exits without removing it leaves a credential in
# the guest -- at whatever mode `scp` gave it -- for the life of
# a VM that never finished provisioning.
#
# One function removes the doubt: it clears the key, prints what
# it was given, and exits. `vagrantfile.rs` refuses a bare
# `exit 1` anywhere else, so a refusal cannot be written that
# skips the removal.
#
# THE REMOVAL'S STATUS IS TESTED RATHER THAN ASSUMED. `rm` gives
# up on a file in a directory it cannot write, and the project's
# own script can arrange one: it has `sudo` and it runs with
# this guest to itself. Under `set -e` an unguarded `rm` would
# abort this function before either `echo` below, so the
# operator would get a bare `rm: Permission denied` naming no
# part of bombyx -- and the message they actually need would
# never print.
#
# A command inside an `if` condition is exempt from `set -e`,
# which is what makes testing the status possible here at all.
refuse() {
    if rm -f "$DEPLOY_KEY"; then
        key_note="any uploaded deploy key has been removed from"
        key_note="$key_note this guest."
    else
        key_note="THE UPLOADED DEPLOY KEY IS STILL IN THIS"
        key_note="$key_note GUEST at $DEPLOY_KEY, because it"
        key_note="$key_note could not be removed. The error"
        key_note="$key_note above says why. Remove it in the"
        key_note="$key_note guest."
    fi
    # `$*` joins the arguments with spaces, so a message
    # written across continued lines prints as one sentence
    # with a single `bombyx:` prefix.
    echo "bombyx: $*" >&2
    echo "bombyx: $key_note" >&2
    exit 1
}

# WHERE THE CLONE GOES, and what has to be true of the
# directory it goes in.
#
# `$HOME` answers, because this script *is* the account the
# agent works as. The provisioner is unprivileged, so the shell
# running this file was started with that account's home
# already in the environment.
#
# A project's `[env]` table can set `HOME`, and the clone then
# moves with it. That is a line the operator wrote in their own
# config rather than something the guest arranged, so it is
# allowed -- but it does mean the value is not guaranteed
# sound. The checks below are what stand between an unusable
# value and a bare `git` error naming no part of bombyx. Each
# one names the property it establishes rather than a position
# in a list, so adding another needs no count corrected
# anywhere.
#
# They sit above the deploy-key block and every one refuses
# through `refuse`, which removes the uploaded key -- so a guest
# that stops here keeps no credential. The banner below the
# block says why that matters and what the dangerous case is.

# Set and not empty. `${HOME:-}` rather than `$HOME`
# because `set -u` would otherwise abort with "unbound
# variable" before `refuse` could run.
if [ -z "${HOME:-}" ]; then
    refuse "HOME is not set in this guest, so bombyx cannot" \
        "tell where to clone the project."
fi

# Usable as a path, and not merely non-empty. A relative
# home would put the clone wherever the provisioner happened to
# start, and `cd` and `readlink -f` further down would resolve
# against that same directory -- so the containment check would
# still pass while the tree sat somewhere nobody expects. `/`
# gives `//project` under a root-owned parent, where the clone
# dies with a bare `git` permission error.
#
# `/?*` is "a slash followed by at least one more character",
# which refuses both.
case "$HOME" in
    /?*) ;;
    *)
        refuse "HOME is \"$HOME\", which is not a path bombyx" \
            "can clone into. It has to be absolute and name a" \
            "directory below /."
        ;;
esac

# Present. A home named in passwd need not exist --
# `/nonexistent` is what `useradd -M` writes -- and nothing in
# this flow proves it does. Left unchecked, `git clone` either
# creates the directory itself when the parent is writable, so
# the clone lands somewhere nobody set up, or dies with a bare
# `git` error.
if [ ! -d "$HOME" ]; then
    refuse "HOME is $HOME, which is not a directory on this" \
        "box, so there is nowhere to clone into."
fi

# Writable *and* searchable. Creating a directory needs
# both bits on the parent, and a home at mode 0600 passes
# `test -w` while `mkdir` in it fails -- measured. Checking
# only `-w` would let exactly the bare `git` error these guards
# exist to prevent through.
if [ ! -w "$HOME" ] || [ ! -x "$HOME" ]; then
    refuse "this account cannot create a directory in its own" \
        "home $HOME, so the clone would fail there."
fi

# Owned by this account. The checks above establish that the
# directory is usable and say nothing about whose it is: `/tmp`
# is absolute, present, and mode 1777 gives both bits, so it
# passes every one of them. The clone would then sit in a
# world-writable directory -- and so would `.git/config`, which
# this script writes `core.sshCommand` into, naming the deploy
# key.
#
# `-O` asks whether the effective user owns the file. That is
# the right question rather than a name comparison, because the
# account this script runs as is the one that will do the
# cloning. It follows symlinks, so a HOME linked into another
# account's directory is refused too.
#
# What it establishes is ownership and nothing about the mode. A
# home this account owns at mode 0777 passes, and the message
# below says only what was checked.
if [ ! -O "$HOME" ]; then
    refuse "HOME is $HOME, which this account does not own." \
        "bombyx clones into a directory belonging to the" \
        "account the agent works as."
fi

# The last component is fixed rather than the project's name, so
# nothing about the operator's config is pasted into this file --
# see the header.
readonly CLONE_DIR="$HOME/project"

# A REFUSAL IS SAFE HERE; AN ABORT IS NOT. That is the
# distinction, and the two are easy to run together.
#
# Vagrant uploads the deploy key before this script starts, and
# the deploy-key block below is what tightens or removes it. A
# refusal before that block is harmless, because `refuse`
# removes the key itself. An *abort* before it is not: the
# script dies without running `refuse`, and the credential
# stays in the agent's own directory for the life of a VM that
# never finished provisioning.
#
# `set -u` on an undeclared variable is the way to get such an
# abort, which is why the checks above read `${HOME:-}` before
# anything expands `$HOME` bare. `set -e` on an untested
# command is the other way, which is why every command acting
# on the key below tests its own status.
#
# The `git` check sits after this block rather than before it,
# because a box without `git` has to reach a refusal rather
# than an abort, and no command above needs `git` anyway.


if [ "${BOMBYX_DEPLOY_KEY:-}" = 1 ]; then
    # The config named a key, so the upload must have happened.
    # Failing here rather than carrying on: a missing file
    # means the key went away on the VM host after bombyx
    # checked for it, and the alternative is a clone that
    # authenticates with nothing and a guest that reports
    # success.
    if [ ! -f "$DEPLOY_KEY" ]; then
        refuse "the configured deploy key did not arrive at" \
            "$DEPLOY_KEY. Check it is still on the VM host" \
            "and re-run."
    fi

    # The key is tightened where it landed, not moved out of
    # the agent's reach.
    #
    # The agent has to be able to use it. Committing inside the
    # guest does not survive a provision -- the note on the
    # checkout below explains why -- so pushing is how work
    # leaves this VM, and pushing needs this key.
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
    # `chmod` follows symlinks, and this path sits in a
    # directory the agent owns, so a re-provisioned guest can
    # replace the file with a link to anything it likes. The
    # operation carries exactly the authority of the account
    # that owns the directory, which is the account running
    # this line, so that link reaches nothing new. Vagrant's
    # file provisioner uploads as that same user, so the file
    # already arrives owned by it and there is nothing for a
    # chown to do.
    # The message says what failed and stops there. `refuse`
    # below prints what became of the key, and claiming an
    # exposure here would contradict it: `chmod` follows
    # symlinks and needs the target's ownership, while `rm`
    # unlinks the name and needs only the directory -- so the
    # ordinary shape of this failure is a `chmod` that fails and
    # a removal that succeeds.
    if ! chmod 600 "$DEPLOY_KEY"; then
        refuse "could not tighten the mode on $DEPLOY_KEY." \
            "The error above says why."
    fi

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
    if ! rm -f "$DEPLOY_KEY"; then
        refuse "the config names no deploy key, but the one at" \
            "$DEPLOY_KEY could not be removed. The error above" \
            "says why. Remove it in the guest: a credential" \
            "nobody granted is still in this VM."
    fi

    # And the clone stops pointing at it. Leaving
    # `core.sshCommand` behind is the mirror of the key nothing
    # points at: it names a deleted identity, and
    # `IdentitiesOnly=yes` with `-F /dev/null` stops git
    # falling back to one the agent does hold, so every fetch
    # and push would fail with an ssh error naming nothing
    # about bombyx.
    #
    # The unsetting itself happens further down, after the
    # clone exists -- there is no config file to unset anything
    # from before that.
fi

# Base images do not all come with git installed. Without this
# check you would get a bare "command not found" from a script
# inside a VM, which tells you nothing about which file to go
# and fix.
if ! command -v git >/dev/null 2>&1; then
    refuse "git is not installed in this box. Install it in" \
        "the box, or choose one with git, so the guest can" \
        "clone the project."
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

if [ -d "$CLONE_DIR/.git" ]; then
    if current_url=$(git -C "$CLONE_DIR" \
        remote get-url origin 2>/dev/null)
    then
        if ! same_repo "$current_url" "$BOMBYX_REPO"; then
            # Announced, never silent. This throws away
            # uncommitted work, and an operator who sees a fresh
            # clone with no explanation has no way to know why.
            echo "bombyx: this VM holds a clone of $current_url" \
                "but the config asks for $BOMBYX_REPO." >&2
            echo "bombyx: discarding the clone and starting" \
                "again. Uncommitted work in $CLONE_DIR is lost." >&2
            # This can fail: `rm -rf` gives up on a directory it
            # cannot write and does not chmod its way in.
            # Reachable through a mode-0500 directory the agent
            # owns, an immutable file, or a mount point inside
            # the clone -- and the project's own script has
            # `sudo` and this tree as its working directory, so
            # any of those is a supported outcome rather than an
            # anomaly.
            #
            # Without this the failure would abort the
            # provision through `set -e`, after the message
            # above has already said the clone is being
            # discarded and after `rm` has deleted part of it.
            if ! rm -rf "$CLONE_DIR"; then
                refuse "could not remove the clone. The" \
                    "message above says what stopped it. Clear" \
                    "it in the guest, then provision again:" \
                    "$CLONE_DIR"
            fi
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
# one in front of it. It tells the program that everything
# after it is a value and never an option, so a ref named
# `--upload-pack=/bin/sh` is read as a branch name rather than
# as an instruction naming a program to run on the other end.
# bombyx also refuses such a value when it reads the config, so
# each `--` here is the second of two guards; see
# `check_not_an_option` in `config/guards.rs` for why both are
# kept.
#
# `FETCH_HEAD` is a file git writes during a fetch, naming the
# commit that fetch just brought down. Checking it out is how
# you land on exactly what was fetched. Using `$BOMBYX_REF` in
# the checkout instead would resolve the name a second time,
# and a `--depth 1` fetch does not create a local branch for it
# to resolve to.
if [ -d "$CLONE_DIR/.git" ]; then
    # Both of these fail on a tracked file inside a directory
    # the agent cannot write -- which the project's own script
    # can leave behind, because it has `sudo` and runs with this
    # tree as its working directory. `git checkout --force` is
    # the nastier one: it exits 1 after printing "Switched to
    # branch", so the worktree is half-changed, and without this
    # check `set -e` would abort with nothing naming bombyx.
    if ! git -C "$CLONE_DIR" fetch --depth 1 origin \
        -- "$BOMBYX_REF"
    then
        refuse "could not update the clone. The message above" \
            "says why. If something in it belongs to another" \
            "user, clear it in the guest: $CLONE_DIR"
    fi

    if ! git -C "$CLONE_DIR" checkout --force FETCH_HEAD
    then
        refuse "could not update the clone. The message above" \
            "says why, and the checkout may be half-changed." \
            "If something in it belongs to another user, clear" \
            "it in the guest: $CLONE_DIR"
    fi
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
    # A directory here with no `.git` in it is a leftover --
    # from a discard that failed part-way, or from anything
    # else. `git clone` into it dies with "destination path
    # already exists and is not an empty directory", which
    # names nothing about bombyx, and by then the message that
    # diagnosed the original failure is one provision in the
    # past.
    if [ -d "$CLONE_DIR" ] && [ -n "$(ls -A "$CLONE_DIR")" ]; then
        refuse "$CLONE_DIR is not empty and holds no" \
            "checkout, so it is a leftover. Remove it in the" \
            "guest, then provision again."
    fi

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
    git -C "$CLONE_DIR" config --replace-all \
        core.sshCommand "$GIT_SSH_COMMAND"
else
    # The mirror: no key, so the clone must not keep pointing
    # at one. `--unset-all`, because `--unset` against two
    # values warns, exits 5 and removes nothing -- which is
    # indistinguishable from the exit 5 that means there was
    # nothing there. Only that code is tolerated.
    git -C "$CLONE_DIR" config --unset-all core.sshCommand \
        || { rc=$?; [ "$rc" = 5 ]; }
fi

# bombyx runs no `chown`. The tree sits in this account's own
# home, and bombyx runs every command above as that account, so
# there is nothing to correct. The project's own script may
# leave root-owned content in there through `sudo`, which is why
# the discard above checks whether it succeeded.

cd "$CLONE_DIR"

# Missing is the ordinary mistake, so check for it first and
# say so plainly. Reporting "the file is not there" as an
# attempted escape would send somebody hunting a symlink that
# does not exist.
if [ ! -e "$BOMBYX_SCRIPT" ]; then
    refuse "$BOMBYX_SCRIPT is not in the cloned project." \
        "Check the \`script\` key in your config."
fi

# Now check where it really points before touching it.
#
# bombyx checks the *config value* -- no leading slash, no `..`
# -- but that says nothing about what the repository put at that
# path. `chmod` and `exec` both follow symlinks, so a repo could
# ship `vagrant/provision.sh` as a link to, say, /etc/passwd and
# `chmod +x` would land on a system file. This account could
# already reach whatever the link names -- but a containment
# check the repository cannot talk its way past is worth having
# all the same. A symlinked parent directory does the same thing
# less obviously, which is why this resolves the whole path
# rather than checking one link.
script_real=$(readlink -f -- "$BOMBYX_SCRIPT")
clone_real=$(readlink -f -- "$CLONE_DIR")
case "$script_real" in
    "$clone_real"/*) ;;
    *)
        refuse "$BOMBYX_SCRIPT points outside the cloned" \
            "project; refusing to run it."
        ;;
esac

if [ ! -f "$script_real" ]; then
    refuse "$BOMBYX_SCRIPT is not a regular file."
fi

# From here on, use `$script_real` and never `$BOMBYX_SCRIPT`.
#
# That is not tidiness. The tree sits in the agent's own home,
# so on a re-provision of a running VM that user can replace the
# script at any moment. If `chmod` and `exec` resolved
# `$BOMBYX_SCRIPT` a second time, they would follow whatever
# chain of symlinks is in place *now* rather than the one that
# was just checked, and the containment check would guard
# nothing.
#
# Resolving once removes the ability to redirect the path
# through a symlinked parent directory after the check. The
# final component is not covered by that: the agent owns the
# file and can replace it with a symlink between the check and
# the use. That reaches only what this account could reach
# anyway.

# Unconditional rather than guarded by `test -x`, because
# `chmod +x` is idempotent and `test -x` answers a different
# question when the file belongs to somebody else: it is true
# when any of the three execute bits is set, while the `exec`
# below needs the one this account will use. A file at mode
# 0011 would pass the test and fail the hand-over.
chmod +x "$script_real"

# `exec` replaces this script's process with the project's
# script rather than starting a second one beside it. So the
# project's script inherits this process, and its exit status is
# what Vagrant sees -- nothing here runs afterwards to swallow a
# failure.
#
# This `exec` changes no privilege, because the whole
# provisioner already runs as the account the agent logs in as.
# So whatever the project's script installs -- a rust toolchain,
# a node toolchain, an agent's own configuration -- lands in
# that account's home, where the agent will find it.
#
# The script also inherits this environment as it stands, which
# is how a project's own variables reach it: the generated
# Vagrantfile puts the `[env]` table into the provisioner
# environment, and nothing between there and here removes any
# of it.
#
# Root is reachable from the project's script through `sudo`,
# which every Vagrant box configures for this user. That is the
# right shape: the script asks for root at the steps that need
# it, rather than having it throughout.
exec -- "$script_real"
