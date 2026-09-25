#!/usr/bin/env bash
#
# This script runs INSIDE the guest VM, not on your machine.
# The Vagrantfile that bombyx generates uploads it, and account.sh
# hands it to the agent's account.
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
# It runs as the account the agent works as, which the config's
# `guest_user` names -- `agent` unless it says otherwise.
# account.sh, the generated Vagrantfile's one shell provisioner,
# runs as root first: it creates that account, moves the staged
# credentials into its home, and hands this script to it through
# `sudo -u`. So Vagrant's own login account, usually `vagrant`,
# runs nothing below.
#
# This script needs no root. The clone sits in this account's own
# home, so this script creates the directory, owns everything in
# it and removes it again, and it runs every git command below as
# that same account on that account's own files. A project that
# has to install packages calls `sudo` from its own script, which
# account.sh configures for this account.
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

# THE ACCOUNT'S HOME, as account.sh made it: /home/<name>, where
# the name is BOMBYX_GUEST_USER. account.sh refuses to hand over
# to an account whose home is anywhere else, so this is the home
# the credentials were written into.
#
# Built from the name rather than read from `$HOME`, because the
# paths below end up inside `core.sshCommand` and
# `credential.helper`, which `git` hands to a shell. bombyx
# checked the name while it read the config -- lowercase letters,
# digits, `_` and `-` -- so it gives that shell nothing to split
# or expand, where a project's `[env]` table can set `HOME` to
# anything. A check below, after `refuse` exists, confirms this
# script really is that account.
#
# Read as `${VAR:-}` so that an unset name reaches that check
# rather than aborting here under `set -u` before `refuse` can
# remove anything.
readonly ACCOUNT_HOME="/home/${BOMBYX_GUEST_USER:-}"


# THE DEPLOY KEY, when the operator's config named one.
#
# A private repository needs a credential inside the guest, and
# this is how it arrives. Before this script runs, account.sh has
# written the key Vagrant staged to DEPLOY_KEY below. That path is
# fixed, so nothing about it is pasted into this file -- see the
# header.
#
# WHETHER a key was configured arrives as BOMBYX_DEPLOY_KEY,
# which the Vagrantfile sets to `1` or `0` on every render, and
# is never read off this filesystem. This flag reports the
# config, where the two further down report what bombyx staged
# for the run. The key is the reason: it already sits on the VM
# host, bombyx only checks it is there, and vagrant uploads it
# -- so there is nothing for bombyx to stage. The key lands in
# this account's own .ssh directory, and this is the account the
# agent works as -- so testing for the file would let the guest
# answer a question about the operator's config. A
# leftover from an interrupted provision, or one `touch` from
# inside the VM, would keep reinstalling a credential the
# operator had already removed.
#
# The guest cannot forge the variable either way, because a
# provisioner's `env:` becomes a prefix on the command line and
# is applied after any /etc/profile.d has been sourced, and
# account.sh carries it here through `sudo --preserve-env`. It is
# read as `${VAR:-}` all the same, so that a missing value takes
# the deleting branch -- the safe answer -- rather than aborting
# under `set -u` before `refuse` could remove anything. bombyx
# rewrites the Vagrantfile and both scripts together on every
# `up`, `provision` and `scratch`, so its own runs always set it.
#
# This path is built from ACCOUNT_HOME above and not from `$HOME`
# the way `CLONE_DIR` below is, for the reason ACCOUNT_HOME's
# banner gives.
#
# Declared before anything expands it. Every refusal below
# removes the uploaded key, and `set -u` makes expanding an
# undeclared variable fatal: the message would never print and
# the key would stay.
readonly DEPLOY_KEY="$ACCOUNT_HOME/.ssh/bombyx-deploy-key"

# WHERE THE GIT CREDENTIAL LANDS, and why this one is built from
# ACCOUNT_HOME while ENV_FILE below is read from passwd.
#
# The file holds one `https://user:token@host` line, and `git`
# reads it through its `store` credential helper. That helper is
# configured as the string `store --file=<path>`, and a helper
# string carrying a space is handed to a SHELL -- measured: with
# HOME set to a temporary directory, a helper spelled
# `--file=$HOME/real` found the file there, while `--file=$NOPE/real`
# found nothing. So the path inside that string gets everything a
# shell does to a word: a space splits it in two, and a `$` is
# expanded. A path built from /home/ and a checked account name
# has neither; one read out of this guest's passwd entry might,
# and a project's `[env]` table can set `HOME`.
#
# This is the same trap KNOWN_HOSTS further down is built from
# ACCOUNT_HOME for: that path ends up inside `core.sshCommand`,
# which `git` also hands to a shell.
#
# Declared before anything expands it, like DEPLOY_KEY above:
# `refuse` removes this file, and `set -u` would make the
# refusal itself fatal otherwise.
readonly GIT_CRED="$ACCOUNT_HOME/.bombyx-git-credentials"

# WHERE THE SECRETS FILE LANDS, and why this one is read from
# passwd while DEPLOY_KEY above is built from ACCOUNT_HOME.
#
# account.sh writes it to `.bombyx-env` in /home/<name>, and
# refuses an account whose passwd home is anywhere else, so the
# passwd entry read here names that same directory. The path
# never reaches a string `git` hands to a shell, so the passwd
# entry is safe to use for it.
#
# `$HOME` cannot name the same file. A project's `[env]` table
# may set HOME, and this script's environment carries that value
# while account.sh used the real home. `getent` is how a shell
# asks for the passwd entry.
#
# `|| bombyx_home=""` puts the assignment in a condition, which
# exempts it from `set -e` and `set -o pipefail`. Without it a
# box whose `getent` is missing or whose account is absent from
# passwd would abort here, before `refuse` could name the
# problem. The emptiness is handled below instead.
#
# Declared before `refuse`, which removes this file: a refusal
# must not leave the project's secrets in a guest that never
# finished provisioning.
bombyx_home=$(getent passwd "$(id -un)" 2>/dev/null | cut -d: -f6) ||
    bombyx_home=""
readonly ENV_FILE="${bombyx_home:-/nonexistent}/.bombyx-env"

# EVERY REFUSAL IN THIS FILE GOES THROUGH HERE, and that is the
# point.
#
# account.sh writes every credential the config named into this
# account's home before this script starts. So a refusal that
# exits without removing them leaves credentials in the guest for
# the life of a VM that never finished provisioning.
#
# One function removes the doubt: it clears every one of them,
# prints what it was given, and exits. The list it clears is the
# `rm -f` below, and a credential added to this script without
# being added there is the mistake this paragraph exists to
# stop. A test refuses any `exit` or `return`
# outside this function, so a refusal that skips the removal
# fails bombyx's build rather than this guest. That test is in
# bombyx's own source, at
# crates/bombyx/src/vagrantfile/bootstrap_tests.rs, and nothing
# in this VM holds a copy of it.
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
    if rm -f "$DEPLOY_KEY" "$ENV_FILE" "$GIT_CRED"; then
        key_note="any uploaded deploy key and any git"
        key_note="$key_note credential have been removed from"
        key_note="$key_note this guest"
        # The secrets half is claimed only when the passwd
        # lookup found a home. Otherwise `$ENV_FILE` is the
        # placeholder, `rm -f` succeeded on a path that was never
        # the file, and saying it is gone would reassure the
        # operator in the same breath as telling them to go and
        # look for it.
        if [ -n "$bombyx_home" ]; then
            key_note="$key_note, and so has any secrets file at"
            key_note="$key_note $ENV_FILE."
        else
            key_note="$key_note. WHERE A SECRETS FILE WOULD BE is"
            key_note="$key_note unknown, because bombyx could not"
            key_note="$key_note read a home directory for this"
            key_note="$key_note account, so nothing here could"
            key_note="$key_note remove one. Run: getent passwd"
            key_note="$key_note \"\$(id -un)\""
        fi
    else
        key_note="AN UPLOADED CREDENTIAL IS STILL IN THIS GUEST,"
        key_note="$key_note at $DEPLOY_KEY, $GIT_CRED or"
        key_note="$key_note $ENV_FILE, because"
        key_note="$key_note it could not be removed. The error"
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

# THIS SCRIPT IS THE ACCOUNT account.sh HANDED IT TO. Every path
# built from ACCOUNT_HOME assumes it, and so does every command
# below that acts on those paths.
#
# The name is checked here as well as by bombyx, because
# ACCOUNT_HOME reaches two strings `git` hands to a shell: the
# pattern refuses anything that shell could split or expand.
# `id -un` inside `[ ]` is not subject to `set -e`, so a failure
# there reads as a mismatch and reaches the refusal.
case "${BOMBYX_GUEST_USER:-}" in
    "" | [!a-z_]* | *[!a-z0-9_-]*)
        refuse "BOMBYX_GUEST_USER is \"${BOMBYX_GUEST_USER:-}\"," \
            "which is not an account name bombyx creates. The" \
            "generated Vagrantfile and this script came from" \
            "different versions of bombyx."
        ;;
esac
if [ "$(id -un)" != "$BOMBYX_GUEST_USER" ]; then
    refuse "this script runs as $(id -un), and bombyx set up" \
        "$BOMBYX_GUEST_USER for the agent. account.sh hands it" \
        "over; run the bombyx command rather than this script."
fi

# WHERE THE CLONE GOES, and what has to be true of the
# directory it goes in.
#
# `$HOME` answers, because this script *is* the account the
# agent works as. account.sh starts it through `sudo -u -H`, so
# the shell running this file was started with that account's
# home already in the environment.
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
# They sit above the credential blocks and every one refuses
# through `refuse`, which removes every upload -- so a guest that
# stops here keeps no credential. The banner between these
# checks and those blocks says why that matters and what the
# dangerous case is.

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

# The last component is the project's name, so several agent VMs
# can be told apart by their clone directory rather than by asking
# git which repository each one holds. The name arrives as an
# environment variable, not pasted into this file, so the script
# stays byte-identical for every project -- see the header.
#
# `:-project` only keeps `set -u` from aborting on an unset name;
# the Vagrantfile bombyx generates always sets it.
#
# `bombyx shell` opens in this directory, and bombyx spells the
# path itself in `remote::shell_into_vm`, as `$HOME/<project>`
# of this account, so change the two together. The one case they
# part is an `[env]` table that sets `HOME`: the clone follows it,
# while the shell uses the account's passwd home and falls back
# to that home when the clone is not there.
readonly CLONE_DIR="$HOME/${BOMBYX_PROJECT:-project}"

# A REFUSAL IS SAFE HERE; AN ABORT IS NOT. That is the
# distinction, and the two are easy to run together.
#
# account.sh writes whichever credentials the config named before
# this script starts: the project's secrets file, the git
# credential, the deploy key. One block follows per credential,
# and each tightens or removes its own. A refusal before any of
# them is harmless, because `refuse` removes them all itself. An
# *abort* before them is not: the script dies without running
# `refuse`, and the credentials stay in the agent's own
# directory for the life of a VM that never finished
# provisioning.
#
# `set -u` on an undeclared variable is the way to get such an
# abort, which is why the checks above read `${HOME:-}` before
# anything expands `$HOME` bare. `set -e` on an untested
# command is the other way, which is why every command acting
# on either credential tests its own status.
#
# The `git` check sits after this block rather than before it,
# because a box without `git` has to reach a refusal rather
# than an abort, and no command above needs `git` anyway.


# THE PROJECT'S SECRETS FILE, and what this script does and does
# not do with it.
#
# It checks the upload arrived, tightens its mode, removes a
# file an earlier run left when the config no longer names one,
# and tells the project's own script where the file is. What it
# never does is put the file anywhere. bombyx does not know that a project keeps
# its secrets at the top of the clone, or that it calls them
# `.env`, so the project's script does the copy:
#
#     cp "$BOMBYX_ENV_FILE" .env
#
# BOMBYX_ENV_FILE_PRESENT answers whether bombyx staged one for
# this run. Staging is what bombyx does just before it runs
# vagrant: it writes the file into the project directory on the
# VM host, beside the Vagrantfile, and removes it again as soon
# as that vagrant run ends. So the file exists on the VM host
# for the length of one run and no longer.
#
# The flag arrives from the generated Vagrantfile, which sets it
# to 1 or 0 on every render, from the same value bombyx writes
# the file from -- so bombyx cannot announce a file it never
# staged. The file can still be missing by the time this script
# runs, which is what the check further down is for: a
# `vagrant provision` started by hand on the VM host has nothing
# staged beside it. The same reasoning
# the deploy-key block gives applies: the file sits in an
# account the agent works as, so asking the filesystem would
# let the guest answer on the operator's behalf.
#
# Read as `${VAR:-}` so that a missing value reaches the branch
# below rather than aborting under `set -u` before `refuse` could
# say anything.
if [ "${BOMBYX_ENV_FILE_PRESENT:-}" = 1 ]; then
    # AN EMPTY `bombyx_home` MEANS ENV_FILE NAMES A PLACEHOLDER,
    # so every line below is about the wrong path. Both branches
    # of this `if` have to answer for that, and they answer
    # differently -- which is why the check is not hoisted above
    # them both.
    #
    # Configured, and bombyx cannot say where the upload landed:
    # refuse. Carrying on would leave the project's secrets at a
    # path nothing here can name.
    if [ -z "$bombyx_home" ]; then
        refuse "bombyx could not read a home directory for this" \
            "account, so it cannot tell where the uploaded" \
            "secrets file is. Either getent is missing on this" \
            "box or the passwd entry names no home -- run" \
            "\"getent passwd \$(id -un)\" to see which. Look for" \
            "the file by hand in this guest."
    fi

    if [ ! -f "$ENV_FILE" ]; then
        # Not "check the file on your workstation": bombyx reads
        # that file before it builds the plan, so on any
        # bombyx-driven run it was there. What reaches this line
        # is a provisioner nobody started through bombyx.
        refuse "an env_file is configured but nothing arrived at" \
            "$ENV_FILE. bombyx stages that file only for the" \
            "length of its own vagrant run, so a vagrant" \
            "provision started by hand on the VM host does not" \
            "find it. Re-run the bombyx command instead."
    fi

    # account.sh writes the file at 0600, and this line makes
    # that true whatever wrote it. A copy an earlier provision
    # left behind, or one the agent loosened since, keeps its
    # mode until this line fixes it.
    #
    # `chmod` follows a symlink, and this path sits in a
    # directory the agent owns, so the agent could point it at
    # another of its own files. That reaches nothing the agent
    # could not already reach. The deploy key's block below
    # makes the same argument about the same directory.
    if ! chmod 600 "$ENV_FILE"; then
        refuse "could not tighten the mode on $ENV_FILE." \
            "The error above says why."
    fi

    export BOMBYX_ENV_FILE="$ENV_FILE"
else
    # Nothing staged, so the name still has to be exported:
    # the project's script reads it, and an unset name under
    # `set -u` would abort that script instead of telling it
    # there is no file.
    export BOMBYX_ENV_FILE=""

    # An earlier run may have left one, and bombyx staged no
    # secrets file this time. Leaving it would keep a credential
    # alive that nothing in this run named.
    #
    # Nothing staged, and the lookup found nothing: say so and
    # carry on. `bombyx_home` is read for this file and nothing
    # else -- the clone uses `$HOME` -- so refusing here would
    # stop a project that has never had an `env_file` from
    # provisioning at all, on exactly the boxes the empty
    # fallback exists for.
    if [ -z "$bombyx_home" ]; then
        echo "bombyx: could not read a home directory for this" \
            "account -- either getent is missing on this box or" \
            "the passwd entry names no home; run \"getent passwd" \
            "\$(id -un)\" to see which. So bombyx cannot look" \
            "for a secrets file an earlier run may have left. If" \
            "this project ever used env_file, check for one by" \
            "hand." >&2
    elif ! rm -f "$ENV_FILE"; then
        refuse "no env_file is configured and the one at" \
            "$ENV_FILE could not be removed. The error above" \
            "says why."
    fi
fi

# THE GIT CREDENTIAL, when the operator's config named a
# `repo_token`.
#
# bombyx read one variable out of the secrets file on the
# workstation and built this file from it: a single
# `https://user:token@host` line, which is the format `git`'s
# `store` credential helper reads. The conversion happens there
# rather than here because a `.env` file is not that format and
# the project's own script runs long after the clone.
#
# BOMBYX_GIT_CRED_PRESENT answers whether bombyx staged one for
# this run -- staged in the sense the secrets-file block above
# gives -- and it arrives from the generated Vagrantfile on
# every render. The deploy-key block near the top of this file
# argues at length why that is the only trustworthy answer:
# this file sits in an account the agent works as, so asking
# the filesystem would let the guest answer on the operator's
# behalf and keep a credential alive that nothing staged.
if [ "${BOMBYX_GIT_CRED_PRESENT:-}" = 1 ]; then
    # Staged, so the upload must have happened. Carrying on
    # would reach the clone with no credential and fail there,
    # naming the repository rather than the file that went
    # missing.
    if [ ! -f "$GIT_CRED" ]; then
        refuse "a repo_token is configured but no git credential" \
            "arrived at $GIT_CRED. bombyx stages that file only" \
            "for the length of its own vagrant run, so a vagrant" \
            "provision started by hand on the VM host does not" \
            "find it. Re-run the bombyx command instead."
    fi

    # Tightened here whatever wrote the file, for the reason the
    # secrets block above gives.
    if ! chmod 600 "$GIT_CRED"; then
        refuse "could not tighten the mode on $GIT_CRED." \
            "The error above says why."
    fi
else
    # Nothing staged, so a file an earlier run left has to go.
    # Leaving it would keep a token alive that nothing in this
    # run named, and `git` would go on sending it.
    if ! rm -f "$GIT_CRED"; then
        refuse "no repo_token is configured and the git" \
            "credential at $GIT_CRED could not be removed. The" \
            "error above says why."
    fi
fi

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
    # in the guest can read it. account.sh writes the key at
    # 0600 already, and this line makes that true whatever wrote
    # the file: a leftover from an earlier provision, or a key
    # the agent loosened since, keeps its mode until this line
    # runs.
    #
    # The agent's own code can read it either way, and that is
    # deliberate -- docs/trust-boundary.md accounts for it.
    #
    # `chmod` follows symlinks, and this path sits in a
    # directory the agent owns, so a re-provisioned guest can
    # replace the file with a link to anything it likes. The
    # operation carries exactly the authority of the account
    # that owns the directory, which is the account running
    # this line, so that link reaches nothing new. account.sh
    # writes the key as this same account, so the file already
    # arrives owned by it and there is nothing for a chown to
    # do.
    #
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

    # The options that point `git` at this key are assembled
    # below, under HOW `git` REACHES THE GIT HOST, together with
    # the ones that decide whether the host gets verified. Both
    # halves end up in one `git_ssh` string, so building them in
    # one place is what stops the two disagreeing.
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
    # points at: it names a deleted identity, and the
    # `IdentitiesOnly=yes` and `-F /dev/null` this script adds
    # for a configured key stop git falling back to one the
    # agent does hold, so every fetch and push would fail with
    # an ssh error naming nothing about bombyx.
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

# THE GIT HOST'S OWN SSH KEYS, when bombyx knows where that host
# publishes them.
#
# `ssh` decides whether it is talking to the right server by
# comparing the key the server offers against a `known_hosts`
# file. A guest that has just booted has no such file, so there
# is nothing to compare against, and the first connection is the
# one that fetches the code this script is about to run.
#
# What closes that is fetching the host's published keys over
# HTTPS, whose trust comes from a certificate authority rather
# than from whatever answers on port 22. bombyx does not make
# that request itself -- it has no HTTP client, deliberately --
# so the URL arrives here and the guest fetches it.
#
# Three variables carry it, and all three are empty when bombyx
# has no key source for this repository. That covers an `https`
# clone, which opens no ssh connection at all, and a git host
# absent from bombyx's table, such as a self-hosted one.
#
#   BOMBYX_GIT_HOST           the host name, lower-cased
#   BOMBYX_HOST_KEYS_URL      where its keys are published
#   BOMBYX_HOST_KEYS_FORMAT   `json` or `lines`
#
# All three are copied into lower-case variables below and read
# from there. Each is read once with a `:-` default, so an unset
# one becomes an empty string rather than an abort. That matters
# because `set -u` aborts the script where it stands, and an
# abort inside `refuse` prints no message and leaves the
# uploaded deploy key in the guest.
#
# The two formats differ because the two hosts differ. GitHub
# serves a JSON document whose `ssh_keys` array holds bare
# `<type> <base64>` pairs, so the host name has to be put in
# front of each one and reading the array needs `jq`. Bitbucket
# serves finished `known_hosts` lines and needs nothing.
#
# Read as `${VAR:-}` for the reason the deploy-key banner gives:
# an unset value falls back to `accept-new` rather than aborting
# under `set -u` before `refuse` could say anything.
#
# `KNOWN_HOSTS` IS BUILT FROM ACCOUNT_HOME, and not from `$HOME`,
# for two reasons.
#
# The first is that this is bombyx's own bookkeeping rather than
# the project's, so it belongs beside the deploy key in the
# account's real home, not wherever an `[env]` HOME moved the
# clone.
#
# The second is about quoting, and it is the one that bites. The
# path ends up inside `core.sshCommand`, which `git` hands to a
# shell -- and a shell expands `$` and a backtick inside double
# quotes as readily as outside them. A project's `[env]` table
# can set `HOME`, and `config::guards::check_renderable` refuses
# a quote, a backslash and `#{` but allows `$`. So a `HOME` of
# `/home/vagrant/$WORKDIR` would reach that shell and be
# rewritten -- measured: the argument arrived at `ssh` as
# `/home/vagrant/EXPANDED/kh`. ACCOUNT_HOME holds only a checked
# account name, so it has nothing to expand.
readonly KNOWN_HOSTS="$ACCOUNT_HOME/.ssh/bombyx-known-hosts"

git_host="${BOMBYX_GIT_HOST:-}"
keys_url="${BOMBYX_HOST_KEYS_URL:-}"
keys_format="${BOMBYX_HOST_KEYS_FORMAT:-}"

if [ -n "$keys_url" ]; then
    if [ -z "$git_host" ]; then
        refuse "bombyx published a URL for the git host's ssh" \
            "keys and no host name to check them against. The" \
            "generated Vagrantfile and this script came from" \
            "different versions of bombyx."
    fi

    # `curl` is needed either way. `jq` only for the JSON
    # format, so a box carrying neither can still clone from
    # Bitbucket.
    #
    # Checking here rather than letting the pipeline fail: a
    # bare "command not found" from inside a VM says nothing
    # about which box to fix.
    if ! command -v curl >/dev/null 2>&1; then
        refuse "curl is not installed in this box, and bombyx" \
            "needs it to fetch $git_host's ssh host keys" \
            "before cloning. Install curl in the box, or choose" \
            "one that has it."
    fi
    if [ "$keys_format" = json ] &&
        ! command -v jq >/dev/null 2>&1; then
        refuse "jq is not installed in this box, and bombyx" \
            "needs it to read $git_host's published ssh" \
            "host keys. Install jq in the box, or choose one" \
            "that has it."
    fi

    # Both failures are reported here rather than left to the
    # fetch. `mkdir` fails when the home cannot hold a new
    # directory; it *succeeds* when `.ssh` already exists and
    # belongs to another account, and then only the redirect
    # fails -- so without the second check the refusal would
    # blame the published-keys URL for a permission problem.
    if ! mkdir -p "$ACCOUNT_HOME/.ssh"; then
        refuse "bombyx could not create $ACCOUNT_HOME/.ssh in" \
            "this guest, so there is nowhere to put" \
            "$git_host's ssh host keys. The error above says" \
            "why."
    fi
    if [ ! -w "$ACCOUNT_HOME/.ssh" ]; then
        refuse "$ACCOUNT_HOME/.ssh is not writable by this" \
            "account, so bombyx cannot put $git_host's ssh" \
            "host keys there."
    fi

    # `--proto` and `--proto-redir` pin the request to HTTPS,
    # including across a redirect. Without the second, a
    # redirect to `http` would be followed, and the whole
    # mechanism rests on the certificate authority that the
    # plain-text answer would not have.
    #
    # `-f` makes an HTTP error status a curl failure rather
    # than a page written into the file. `-sS` prints curl's
    # own error and nothing else.
    #
    # `set -o pipefail` at the top of this file is what makes a
    # curl failure fail the pipeline the JSON branch builds. A
    # partly written file survives the refusal, and the next
    # provision overwrites it -- nothing reads it in between,
    # because a refusal ends this guest's provisioning.
    case "$keys_format" in
        json)
            if ! curl -fsSL --proto '=https' \
                --proto-redir '=https' --max-time 30 \
                -- "$keys_url" |
                jq -r --arg host "$git_host" \
                    '.ssh_keys[] | $host + " " + .' \
                    >"$KNOWN_HOSTS"; then
                refuse "bombyx could not read $git_host's" \
                    "ssh host keys from $keys_url." \
                    "The error above says why. Without them the" \
                    "guest cannot tell that host from an" \
                    "impostor, so it will not clone."
            fi
            ;;
        lines)
            if ! curl -fsSL --proto '=https' \
                --proto-redir '=https' --max-time 30 \
                -- "$keys_url" >"$KNOWN_HOSTS"; then
                refuse "bombyx could not fetch" \
                    "$git_host's ssh host keys from" \
                    "$keys_url. The error above says" \
                    "why. Without them the guest cannot tell" \
                    "that host from an impostor, so it will not" \
                    "clone."
            fi
            ;;
        *)
            refuse "bombyx asked for $git_host's ssh" \
                "host keys in a format this script does not" \
                "know: \"$keys_format\". The" \
                "generated Vagrantfile and this script came" \
                "from different versions of bombyx."
            ;;
    esac

    # One check for four ways the fetch can succeed and still
    # leave nothing usable: an empty body, a truncated one, a
    # document whose shape has changed, and a response for some
    # other host. Every one of them ends in a `known_hosts` file
    # with no line for the host about to be contacted.
    #
    # Left unchecked, each would surface at clone time as an
    # `ssh` host-key failure, which reads as an attack rather
    # than as a fetch that came back wrong.
    #
    # The dots in a host name are wildcards to `grep`, so this
    # pattern is looser than the name it came from. That costs
    # nothing: the file holds what an HTTPS-authenticated host
    # served, `ssh` still compares the offered key against the
    # whole file, and this is a check on the response's shape
    # rather than on its contents.
    if ! grep -q "^$git_host " "$KNOWN_HOSTS"; then
        refuse "the keys bombyx fetched from" \
            "$keys_url hold no line for" \
            "$git_host, so there is nothing to verify" \
            "that host against. Check whether that URL still" \
            "publishes host keys."
    fi
fi

# HOW `git` REACHES THE GIT HOST. Two questions are answered
# here, and they are independent of each other.
#
# GIT_SSH_COMMAND is what git passes to `ssh` for every
# connection it makes.
#
# `-F /dev/null` goes on either way: it tells `ssh` to ignore
# every ssh_config, so a box shipping its own
# /etc/ssh/ssh_config cannot add an identity or a host alias
# that nothing here chose.
#
# WHICH IDENTITY. `-i` names the deploy key, and
# `IdentitiesOnly=yes` stops `ssh` offering any other one it
# finds. `IdentitiesOnly` does not exclude an identity named by
# an `IdentityFile` line in a config file, which is the other
# half of what `-F /dev/null` above is for.
#
# WHETHER THE HOST IS VERIFIED. With keys fetched above,
# `StrictHostKeyChecking=yes` refuses any key that is not in a
# known-hosts file, and the two `KnownHostsFile` options say
# which files those are. Naming both is necessary: `-F` above
# only makes `ssh` ignore /etc/ssh/ssh_config, and the host key
# database is a different setting -- `GlobalKnownHostsFile`
# defaults to /etc/ssh/ssh_known_hosts and
# /etc/ssh/ssh_known_hosts2. Left at that default, a key
# planted in either of those satisfies the check as readily as
# one bombyx fetched, and the project's script has the `sudo` to
# plant one before the next provision.
#
# `UserKnownHostsFile` likewise replaces the account's own
# `~/.ssh/known_hosts`, so an `accept-new` entry an earlier
# provision left there cannot stand in for a fetched key.
#
# Without fetched keys, `accept-new` records the host's key on
# first sight and refuses a change afterwards. That is the
# weaker answer, and it is what a host bombyx has no key source
# for gets; docs/trust-boundary.md says what it costs.
#
# IT IS NEVER EXPORTED. `git_ssh` is built here and named on
# each of the two `git` commands below that talk to the network,
# one command at a time.
#
# The reason is `exec` at the end of this script, which hands
# the environment to the project's own script. An exported
# GIT_SSH_COMMAND would govern every `git` command that script
# runs, and every one the agent runs afterwards. With keys
# fetched, that means each of them checked against a file naming
# one host, with the account's own `~/.ssh/known_hosts` and
# `~/.ssh/config` switched off -- so a second ssh git host
# becomes unreachable, and unreachable with no local remedy,
# because `UserKnownHostsFile` replaced the file somebody would
# add it to. bombyx has nothing to say about those connections.
#
# What does carry forward is `core.sshCommand` on the clone, set
# further down. That reaches the repository bombyx cloned and
# stops there.
#
# `-F /dev/null` TRAVELS WITH THE KEY and not on its own. It is
# there to make `IdentitiesOnly` true, so it belongs to the
# identity half; with no key there is no identity to protect,
# and switching off the account's `~/.ssh/config` would cost the
# agent its own `Host` blocks, `User` lines and any
# `ProxyCommand` for nothing. This string is persisted as
# `core.sshCommand` further down, so an option added here
# outlives the provision.
#
# The host-key options do not need it. An `-o` on the command
# line outranks any config file, so a box setting
# `StrictHostKeyChecking no` in /etc/ssh/ssh_config cannot
# loosen what is asked for here.
git_ssh="ssh"
if [ "${BOMBYX_DEPLOY_KEY:-}" = 1 ]; then
    git_ssh="$git_ssh -F /dev/null -i $DEPLOY_KEY"
    git_ssh="$git_ssh -o IdentitiesOnly=yes"
fi
if [ -n "$keys_url" ]; then
    git_ssh="$git_ssh -o StrictHostKeyChecking=yes"
    git_ssh="$git_ssh -o UserKnownHostsFile=$KNOWN_HOSTS"
    git_ssh="$git_ssh -o GlobalKnownHostsFile=/dev/null"
elif [ "${BOMBYX_DEPLOY_KEY:-}" = 1 ]; then
    # A key and no published key source. `accept-new` records
    # the host on first sight and refuses a change afterwards.
    #
    # With neither, `git_ssh` stays plain `ssh` and bombyx adds
    # nothing at all -- which is `ssh`'s own default of `ask`.
    # Such a clone has no credential either, so it could not
    # have authenticated whatever this said.
    git_ssh="$git_ssh -o StrictHostKeyChecking=accept-new"
fi

# HOW `git` AUTHENTICATES AN https CLONE, when a token was
# configured.
#
# `credential.helper` tells `git` which program to ask for a
# username and a password. `store --file=<path>` names the
# helper `git-credential-store` and hands it that path, and the
# helper answers from the line bombyx put there. GIT_CRED's
# banner near the top of this file says why that path is a
# literal.
#
# The setting is named on one command at a time, the way
# `git_ssh` is, and for the same reason: `exec` at the end of
# this script hands the environment to the project's own script,
# and a global setting would govern every `git` that script and
# the agent afterwards run. What carries forward instead is the
# setting written into the clone further down, which reaches
# that one repository and stops.
#
# Empty when nothing was configured, which covers a public
# repository and an ssh clone.
if [ "${BOMBYX_GIT_CRED_PRESENT:-}" = 1 ]; then
    git_cred_helper="store --file=$GIT_CRED"
else
    git_cred_helper=""
fi

# The two `git` commands below that talk to the network go
# through here, so the credential and the ssh command are added
# in one place rather than twice.
#
# Two branches rather than one command with a variable in it.
# The helper string carries a space, so an unquoted expansion
# would split it and a quoted one would hand `git` an empty
# `-c` when nothing is configured.
git_net() {
    if [ -n "$git_cred_helper" ]; then
        GIT_SSH_COMMAND="$git_ssh" git \
            -c "credential.helper=$git_cred_helper" "$@"
    else
        GIT_SSH_COMMAND="$git_ssh" git "$@"
    fi
}

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
    # Named on the command rather than exported, and it wins
    # over any `core.sshCommand` an earlier provision left in
    # this clone: `git` prefers GIT_SSH_COMMAND to that setting.
    if ! git_net -C "$CLONE_DIR" \
        fetch --depth 1 origin -- "$BOMBYX_REF"
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

    git_net clone \
        --depth 1 --branch "$BOMBYX_REF" \
        -- "$BOMBYX_REPO" "$CLONE_DIR"
fi

# The clone records the command this script cloned with, so
# every later `git` run in that directory reaches the host the
# same way. Two things need carrying: the deploy key, without
# which the agent has a credential it may read and no idea where
# it is, and the fetched host keys, without which a later
# `git fetch` falls back to `~/.ssh/known_hosts` -- a file
# bombyx never writes -- under the stock
# `StrictHostKeyChecking=ask`.
#
# Written only when there is one of those to carry. With
# neither -- a public repository and no recognised git host --
# the clone gets no setting, so `git` in it keeps the account's
# own `~/.ssh/config` and `known_hosts`.
#
# The condition reads the two flags rather than testing
# `$git_ssh`, which is never empty. Both flags come from the
# generated Vagrantfile, and the deploy-key header above says
# why that is the only trustworthy answer to "was a key
# configured": a guest can set its own environment, and it
# cannot set a provisioner's `env:` block.
#
# `--replace-all` because a plain set refuses with "cannot
# overwrite multiple values" and exits 5 once the key has two,
# which `set -e` would turn into a provision aborted after the
# clone and the fetch had already run. A project script doing
# `git config --add core.sshCommand` reaches that by accident.
if [ "${BOMBYX_DEPLOY_KEY:-}" = 1 ] || [ -n "$keys_url" ]; then
    git -C "$CLONE_DIR" config --replace-all \
        core.sshCommand "$git_ssh"
else
    # The mirror: bombyx set nothing, so the clone must not
    # keep pointing at something. `--unset-all`, because
    # `--unset` against two values warns, exits 5 and removes
    # nothing -- which is indistinguishable from the exit 5
    # that means there was nothing there. Only that code is
    # tolerated.
    git -C "$CLONE_DIR" config --unset-all core.sshCommand \
        || { rc=$?; [ "$rc" = 5 ]; }
fi

# And the same for the credential, so the agent can fetch and
# push in this clone after provisioning has finished.
#
# `--replace-all` and the tolerated exit 5 are there for the
# reasons the `core.sshCommand` block above gives: a plain set
# refuses once the key has two values, and `--unset` against two
# values exits 5 having removed nothing, which is the same code
# as "there was nothing there".
if [ -n "$git_cred_helper" ]; then
    git -C "$CLONE_DIR" config --replace-all \
        credential.helper "$git_cred_helper"
else
    git -C "$CLONE_DIR" config --unset-all credential.helper \
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
# This `exec` changes no privilege, because account.sh already
# handed this script to the account the agent works as. So
# whatever the project's script installs -- a rust toolchain, a
# node toolchain, an agent's own configuration -- lands in that
# account's home, where the agent will find it.
#
# The script also inherits this environment as it stands, which
# is how a project's own variables reach it: the generated
# Vagrantfile puts the `[env]` table into the provisioner
# environment, and account.sh carries every one of those names
# across through `sudo --preserve-env`.
#
# Root is reachable from the project's script through `sudo`,
# which account.sh configures for this account. That is the
# right shape: the script asks for root at the steps that need
# it, rather than having it throughout.
exec -- "$script_real"
