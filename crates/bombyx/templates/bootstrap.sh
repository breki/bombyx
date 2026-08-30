#!/usr/bin/env bash
#
# Run inside the guest by the Vagrantfile bombyx generates.
#
# This file is identical for every project. Everything that
# varies arrives as an environment variable set by Vagrant's
# shell provisioner, so bombyx interpolates nothing into it and
# ships it verbatim. That is what keeps config values out of
# shell text bombyx composes -- see docs/trust-boundary.md.
#
# It runs as root, in a machine whose contents are assumed
# untrustworthy once the project's own script takes over.

set -euo pipefail

: "${BOMBYX_REPO:?bombyx: BOMBYX_REPO is not set}"
: "${BOMBYX_REF:?bombyx: BOMBYX_REF is not set}"
: "${BOMBYX_SCRIPT:?bombyx: BOMBYX_SCRIPT is not set}"

readonly CLONE_DIR=/opt/project

# The agent runs as this user, so the checkout has to belong to
# it. A root-owned clone boots a VM whose whole purpose is
# editing code that the editor cannot write to.
readonly OWNER=vagrant

# Not every base box ships git, and the failure without this is a
# bare "command not found" from a provisioner running as root,
# which says nothing about what to change.
if ! command -v git >/dev/null 2>&1; then
    echo "bombyx: git is not installed in this box." >&2
    echo "bombyx: install it in the box, or choose one with" \
        "git, so the guest can clone the project." >&2
    exit 1
fi

# Vagrant runs provisioners again on `vagrant provision`, which
# is the whole reason `bombyx provision` exists. Cloning on the
# first run and fetching on later ones is what makes the second
# command do something.
if [ -d "$CLONE_DIR/.git" ]; then
    # The remote is reset every time rather than trusted from the
    # first clone. Without this, changing `source.repo` in
    # bombyx.toml and re-provisioning keeps fetching the old
    # repository and reports success -- the silent-wrong-answer
    # case, which is worse than a failure.
    git -C "$CLONE_DIR" remote set-url origin -- "$BOMBYX_REPO"
    git -C "$CLONE_DIR" fetch --depth 1 origin -- "$BOMBYX_REF"
    git -C "$CLONE_DIR" checkout --force FETCH_HEAD
else
    git clone --depth 1 --branch "$BOMBYX_REF" \
        -- "$BOMBYX_REPO" "$CLONE_DIR"
fi

chown -R "$OWNER:$OWNER" "$CLONE_DIR"

cd "$CLONE_DIR"

# A script committed without the executable bit is common enough
# on Windows checkouts that failing on it would be a poor first
# experience.
if [ ! -x "$BOMBYX_SCRIPT" ]; then
    chmod +x "$BOMBYX_SCRIPT"
fi

exec "./$BOMBYX_SCRIPT"
