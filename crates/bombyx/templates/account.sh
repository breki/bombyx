#!/usr/bin/env bash
#
# This script runs INSIDE the guest VM, as root, before
# anything else bombyx does there. The Vagrantfile that bombyx
# generates points its one shell provisioner at it, marked
# `privileged: true`.
#
# WHY THE UPLOADS ARE STAGED. Vagrant uploads files as the
# account it logs in as -- the login account, usually `vagrant`
# -- and that account can write only its own home. So the
# Vagrantfile stages every file in ~/.bombyx-staging there, the
# staging directory, and this script moves each one on. The login
# account is never the agent's: bombyx refuses a `guest_user` of
# `vagrant`, and this script refuses a guest_user equal to
# SUDO_USER, which names the account Vagrant logged in as.
#
# It does five things, and then hands over:
#
#   1. creates the account the agent works as, named by
#      BOMBYX_GUEST_USER, if the box does not have it yet;
#   2. gives that account passwordless `sudo`, through a file of
#      its own in /etc/sudoers.d;
#   3. moves each file Vagrant staged -- the deploy key, the
#      secrets file, the git credential -- into that account's
#      home;
#   4. loads an egress rule in the guest, as a backstop to the
#      VM host's firewall;
#   5. installs bootstrap.sh root-owned, and runs it as that
#      account.
#
# Root is needed throughout: to create the account and its
# sudoers file, to read the staging directory in the login
# account's home, to install the egress rule's loader and unit and
# load the rule, and to install bootstrap.sh root-owned. Only the
# writes into the agent's own home run as the agent. This file
# reads nothing from the project's repository: the clone does not
# exist until bootstrap.sh makes it, as the agent.
# docs/trust-boundary.md describes the isolation this serves.
#
# Like bootstrap.sh, this file is the same for every project, and
# bombyx pastes nothing into it. Everything that changes per
# project arrives as an environment variable, set by Vagrant.

# `-e` stops at the first failing command, `-u` makes an unset
# variable an error, and `pipefail` fails a pipeline when any
# command in it fails. bootstrap.sh's header says why each one
# matters.
set -euo pipefail

# WHERE THE STAGED FILES ARE. `sudo` sets SUDO_USER to the
# account Vagrant logged in as, and that account's passwd entry
# names the home the uploads went to. bombyx refuses SUDO_USER in
# a project's `[env]` table, because Vagrant applies the `env:`
# block after `sudo` and an `[env]` value would win.
#
# Both reads use `:-` and `||` so that a missing value reaches a
# refusal below rather than aborting under `set -e` or `set -u`
# first -- an abort would skip the cleanup `refuse` does.
login_user="${SUDO_USER:-}"
staging_home=""
if [ -n "$login_user" ]; then
    staging_home=$(getent passwd "$login_user" | cut -d: -f6) ||
        staging_home=""
fi
readonly STAGING="${staging_home:-/nonexistent}/.bombyx-staging"

# THE ACCOUNT THE AGENT WORKS AS, and its home. bombyx checked the
# name while it read the config: lowercase letters, digits, `_`
# and `-`, starting with a letter or `_`. The check below repeats
# the part this script relies on, for a Vagrantfile nobody
# generated through bombyx.
user="${BOMBYX_GUEST_USER:-}"
readonly home="/home/$user"

# Where bootstrap.sh is installed. It is installed fresh from the
# staged upload on every provision, so what runs is bombyx's copy
# whatever the agent left there. Root ownership adds nothing while
# the agent has passwordless `sudo`; it would matter only if the
# agent lost it, for instance by an operator removing its file in
# /etc/sudoers.d. bombyx has no setting for that.
readonly BOOTSTRAP=/usr/local/libexec/bombyx/bootstrap.sh

# Set once the first credential may have been written into the
# agent's home, which is when `refuse` has more to remove.
placing=""

# EVERY REFUSAL IN THIS FILE GOES THROUGH HERE. The staging
# directory holds every credential the config named, so a
# refusal that left it would leave them in the login account's
# home for the life of a VM that never finished provisioning.
#
# Once `placing` is set, a credential may also sit in the agent's
# home, and bootstrap.sh -- which would otherwise answer for it --
# never runs after a refusal here. So `refuse` removes those
# copies too, as the agent, for the reason step 3 below gives.
refuse() {
    if [ -n "$staging_home" ] && ! rm -rf -- "$STAGING"; then
        echo "bombyx: THE STAGED CREDENTIALS ARE STILL IN THIS" \
            "GUEST, at $STAGING, because they could not be" \
            "removed. Remove them in the guest." >&2
    fi
    if [ -n "$placing" ] && ! sudo -u "$user" -- rm -f -- \
        "$home/.ssh/bombyx-deploy-key" "$home/.bombyx-env" \
        "$home/.bombyx-git-credentials"; then
        echo "bombyx: A CREDENTIAL IS STILL IN $home, because it" \
            "could not be removed. Remove it in the guest." >&2
    fi
    # `$*` joins the arguments with spaces, so a message written
    # across continued lines prints as one sentence.
    echo "bombyx: $*" >&2
    exit 1
}

if [ -z "$staging_home" ]; then
    refuse "bombyx could not find the home of the account Vagrant" \
        "logged in as (SUDO_USER is \"$login_user\"), so it cannot" \
        "find the files Vagrant staged there."
fi

# A name the shell reads as one plain word, in the same pattern
# bootstrap.sh uses: empty, `[!a-z_]*` "starts with anything but
# a lowercase letter or an underscore", or `*[!a-z0-9_-]*` "holds
# any other character anywhere".
case "$user" in
    "" | [!a-z_]* | *[!a-z0-9_-]*)
        refuse "guest_user (\"$user\") is not an account name" \
            "bombyx creates."
        ;;
esac
if [ "$user" = root ] || [ "$user" = "$login_user" ]; then
    refuse "guest_user (\"$user\") is root or the account Vagrant" \
        "logs in as. The agent needs an account of its own."
fi

# The hand-over list, checked for the same reason: `sudo` reads
# it as comma-separated names.
case "${BOMBYX_PRESERVE_ENV:-}" in
    "" | *[!A-Za-z0-9_,]*)
        refuse "BOMBYX_PRESERVE_ENV is missing or holds something" \
            "other than variable names. The generated Vagrantfile" \
            "and this script came from different versions of bombyx."
        ;;
esac

# Base images differ, and a bare "command not found" from inside
# a VM names no file to fix. `useradd` is missing on boxes built
# on busybox, such as Alpine.
for tool in useradd visudo install; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        refuse "$tool is not installed in this box, and bombyx needs" \
            "it to set up the agent's account. Choose a box that" \
            "has it."
    fi
done

if [ ! -f "$STAGING/bootstrap.sh" ]; then
    refuse "bootstrap.sh did not arrive at $STAGING/bootstrap.sh," \
        "so there is nothing to hand to the agent's account."
fi

# A VM SET UP FOR ANOTHER ACCOUNT IS REFUSED. Everything this
# script and bootstrap.sh write sits under the current
# guest_user, so after a rename nothing would ever remove the old
# account's sudoers file or the credentials in its home -- and
# the operator who dropped a credential in the same edit would
# believe it gone. So a sudoers file bombyx wrote for another
# name stops the run here, before anything is created. The glob
# is left unexpanded when nothing matches, which `-e` answers.
#
# A VM that bombyx 0.7.0 or earlier built carries no such file,
# because its agent was the login account itself. Its credentials
# sit in that account's home instead, at the paths the second loop
# checks, so finding one of them is refused the same way.
for granted in /etc/sudoers.d/bombyx-*; do
    if [ -e "$granted" ] &&
        [ "${granted#/etc/sudoers.d/bombyx-}" != "$user" ]; then
        refuse "this VM was set up for guest_user" \
            "\"${granted#/etc/sudoers.d/bombyx-}\", and the config now" \
            "names \"$user\". bombyx does not move a VM from one" \
            "account to another; run destroy, then up, for this" \
            "project."
    fi
done
for left in "$staging_home/.ssh/bombyx-deploy-key" \
    "$staging_home/.bombyx-env" \
    "$staging_home/.bombyx-git-credentials"; do
    if [ -e "$left" ]; then
        refuse "this VM was set up by bombyx 0.7.0 or earlier, which" \
            "left a credential at $left, in the home of the account" \
            "Vagrant logs in as. bombyx does not move a VM to the" \
            "agent's own account; run destroy, then up, for this" \
            "project."
    fi
done

# 1. THE ACCOUNT. Created with its home at /home/<name>, a login
# shell of bash, and a group of the same name. `useradd` locks
# the password, so the account is reachable only through `sudo`
# from an account that already has it.
if ! getent passwd "$user" >/dev/null; then
    if ! useradd --create-home --home-dir "$home" --shell /bin/bash \
        --user-group "$user"; then
        refuse "could not create the account $user. The error" \
            "above says why."
    fi
fi

# An account the box already had is used as it is, so long as its
# home is where bombyx looks. bootstrap.sh builds the paths it
# hands to `git` from /home/<name>, so another home would put the
# credentials where nothing reads them.
actual_home=$(getent passwd "$user" | cut -d: -f6) || actual_home=""
if [ "$actual_home" != "$home" ] || [ ! -d "$home" ]; then
    refuse "the account $user has its home at \"$actual_home\"," \
        "and bombyx needs it at $home. Choose another guest_user."
fi

# 2. SUDO. One line in a file named after the account, checked by
# `visudo` before it is installed: a sudoers file with a syntax
# error stops `sudo` working for every account on the box,
# Vagrant's own included. The file name holds no `.`, which
# matters because `sudo` skips a file in /etc/sudoers.d whose
# name contains one.
#
# The draft is created in /etc/sudoers.d itself, under a name
# holding a `.` so `sudo` skips it, rather than wherever `TMPDIR`
# points: the project's `[env]` table is in this script's
# environment, so a bare `mktemp` would follow it into a
# directory the agent may own.
sudoers_tmp=$(mktemp /etc/sudoers.d/.bombyx-XXXXXX) ||
    refuse "could not create a temporary file in /etc/sudoers.d."
if ! printf '%s ALL=(ALL) NOPASSWD: ALL\n' "$user" >"$sudoers_tmp" ||
    ! visudo -cqf "$sudoers_tmp" ||
    ! install -m 0440 -o root -g root "$sudoers_tmp" \
        "/etc/sudoers.d/bombyx-$user"; then
    rm -f -- "$sudoers_tmp"
    refuse "could not install a sudoers entry for $user. The" \
        "error above says why."
fi
rm -f -- "$sudoers_tmp"

# 3. THE CREDENTIALS. Each is written by the agent's account
# itself, from a file root opened: `sudo -u` runs the writer, and
# the redirect hands it the staged copy on its standard input. So
# root writes nothing into a directory the agent owns, and a link
# the agent left at one of these paths reaches only what the
# agent could already reach.
#
# Written only when the Vagrantfile announced it AND the upload
# arrived. When it was announced and did not arrive, the copy an
# earlier provision left is removed instead, so bootstrap.sh
# refuses the key as missing rather than using a stale one. When
# it was not announced at all, bootstrap.sh removes the old copy
# itself, and says so.
#
# $1 is the announcement, $2 the staged file, and $3 the path it
# ends up at. The writer creates $3's directory, `${1%/*}` being
# the path with its last component removed. `umask 077` makes
# `cat >` create a new file at 0600; without it the file would sit
# at the inherited 0644 until `chmod` ran, and an account's home
# is often traversable by the others.
place() {
    if [ "$1" = 1 ] && [ -f "$2" ]; then
        # SC2024 warns that the redirect is opened by root and not
        # by the account `sudo` switches to. That is the intent:
        # the agent cannot read the login account's home.
        # shellcheck disable=SC2024
        if ! sudo -u "$user" -- sh -c 'umask 077 &&
            mkdir -p -m 700 "${1%/*}" && cat >"$1" && chmod 600 "$1"' \
            sh "$3" <"$2"; then
            refuse "could not write $3 as $user. The error above" \
                "says why."
        fi
    elif [ "$1" = 1 ]; then
        if ! sudo -u "$user" -- rm -f -- "$3"; then
            refuse "could not remove the stale copy at $3. The" \
                "error above says why."
        fi
    fi
}

placing=1
place "${BOMBYX_DEPLOY_KEY:-}" "$STAGING/deploy-key" \
    "$home/.ssh/bombyx-deploy-key"
place "${BOMBYX_ENV_FILE_PRESENT:-}" "$STAGING/env" "$home/.bombyx-env"
place "${BOMBYX_GIT_CRED_PRESENT:-}" "$STAGING/git-credentials" \
    "$home/.bombyx-git-credentials"

# 4. A BACKSTOP EGRESS RULE. The guest refuses its own new
# connections to the private address ranges that
# scripts/agent-vm-firewall.sh refuses on the VM host, and all
# IPv6, as that script does. This is a backstop, not containment:
# the agent has passwordless `sudo` and can delete the rule with
# one command. What it catches is a mistake on the VM host, such
# as rules never applied, wiped at boot, or naming a bridge
# libvirt has moved. docs/trust-boundary.md says the same.
#
# A systemd unit loads the rule at every boot, after the network
# is up. Its loader reads the guest's default routes at that
# moment, so a renumbered libvirt network cannot leave a stale
# gateway in the rule. The rule covers only the interfaces that
# carry a default route, so a container bridge inside the guest
# keeps working. Replies to connections made from outside, such
# as `vagrant ssh`, pass. DHCP passes, and DNS passes to each
# default gateway.
#
# Every failure here warns and carries on, and none refuses: a
# backstop the agent can remove must not stop a VM being built.
readonly EGRESS_LOADER=/usr/local/libexec/bombyx/guest-egress.sh
readonly EGRESS_UNIT=/etc/systemd/system/bombyx-guest-egress.service

# Callers give the cause; this states the outcome, once.
egress_warning() {
    echo "bombyx: warning: $* So the guest has no egress rule of" \
        "its own, and the VM host's firewall is still its only" \
        "containment." >&2
}

# The loader is written from a quoted heredoc, so nothing in it is
# expanded here: every `$` in it is read when it runs, at boot or
# when a provision restarts the unit. It is a program of its own,
# so its `exit` leaves the loader, never this script; systemd
# records a non-zero exit as a failed unit.
write_egress_loader() {
    install -d -m 0755 -o root -g root "${EGRESS_LOADER%/*}" &&
        cat >"$EGRESS_LOADER" <<'LOADER' && chmod 0755 "$EGRESS_LOADER"
#!/bin/sh
# Loads bombyx's egress rule in this guest. account.sh writes this
# file on every provision, and bombyx-guest-egress.service runs it
# at boot. A backstop, not containment: account.sh says why.
set -eu

blocked4="10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16,
          169.254.0.0/16, 100.64.0.0/10, 127.0.0.0/8,
          224.0.0.0/4"
# nft ends a statement at a newline, even inside a set's braces,
# so the list is joined onto one line before it reaches a rule.
blocked4=$(printf '%s' "$blocked4" | tr -s ' \n' '  ')

# Checked apart from the routes, so a box without `ip` is not
# reported as one without a default route.
if ! command -v ip >/dev/null 2>&1; then
    echo "bombyx-guest-egress: ip is not installed, so no rule" \
        "was loaded" >&2
    exit 1
fi

# One "<interface> <gateway>" line per IPv4 default route.
default_routes() {
    ip -4 route show default | awk '{
        d = ""; g = ""
        for (i = 1; i < NF; i++) {
            if ($i == "dev") d = $(i + 1)
            if ($i == "via") g = $(i + 1)
        }
        if (d != "") print d, g
    }'
}

# At boot the unit can start before DHCP has given the guest a
# default route, so the loader waits up to 30 seconds for one
# rather than failing once and never trying again.
routes=$(default_routes)
waited=0
while [ -z "$routes" ] && [ "$waited" -lt 30 ]; do
    sleep 1
    waited=$((waited + 1))
    routes=$(default_routes)
done
if [ -z "$routes" ]; then
    echo "bombyx-guest-egress: no IPv4 default route after 30" \
        "seconds, so no rule was loaded" >&2
    exit 1
fi

rules=""
while read -r dev gw; do
    if [ -n "$gw" ]; then
        rules="$rules
        oifname \"$dev\" ip daddr $gw udp dport 53 accept
        oifname \"$dev\" ip daddr $gw tcp dport 53 accept"
    fi
    rules="$rules
        oifname \"$dev\" ip daddr { $blocked4 } counter reject
        oifname \"$dev\" meta nfproto ipv6 counter reject"
done <<ROUTES
$routes
ROUTES

# Declaring the table, deleting it and declaring it again in one
# file replaces it in one transaction, on the first load and on
# every later one.
#
# `tcp sport 22` lets sshd's replies out whatever conntrack says.
# conntrack is the kernel's connection tracker, which `ct state`
# reads, and it starts tracking only once a rule first needs it.
# A provision runs this loader inside Vagrant's SSH session, which
# the VM host opened from its address on the libvirt network: the
# guest's default gateway, inside the blocked 192.168.0.0/16. When
# this rule is the first to need conntrack, the session predates
# it, so the session's next packet counts as a new connection to
# the gateway, not an established one. Without the accept, loading
# the rule cuts the session that is loading it.
nft -f - <<RULES
table inet bombyx_guest
delete table inet bombyx_guest
table inet bombyx_guest {
    chain output {
        type filter hook output priority 0; policy accept;
        oifname "lo" accept
        ct state established,related accept
        tcp sport 22 accept
        udp dport 67 accept
$rules
    }
    chain forward {
        type filter hook forward priority 0; policy accept;
        ct state established,related accept
        udp dport 67 accept
$rules
    }
}
RULES
LOADER
}

# $1 is the absolute path to `nft`, for the unit's stop command;
# older systemd versions take only an absolute path there. The `-`
# in front of it makes a stop with the table already gone count as
# success. The unit starts after nftables.service because a box's
# /etc/nftables.conf may open with `flush ruleset`, which would
# delete this table if that service ran second.
write_egress_unit() {
    printf '%s\n' \
        "[Unit]" \
        "Description=bombyx guest egress rule, a backstop" \
        "Wants=network-online.target" \
        "After=network-online.target nftables.service" \
        "" \
        "[Service]" \
        "Type=oneshot" \
        "RemainAfterExit=yes" \
        "ExecStart=$EGRESS_LOADER" \
        "ExecStop=-$1 delete table inet bombyx_guest" \
        "" \
        "[Install]" \
        "WantedBy=multi-user.target" >"$EGRESS_UNIT"
}

if ! nft_bin=$(command -v nft); then
    egress_warning "nft is not installed in this box."
elif ! command -v systemctl >/dev/null 2>&1; then
    egress_warning "this box has no systemctl to load the rule at boot."
elif ! write_egress_loader || ! write_egress_unit "$nft_bin" ||
    ! systemctl daemon-reload ||
    ! systemctl enable --quiet bombyx-guest-egress.service ||
    ! systemctl restart bombyx-guest-egress.service; then
    # The loader runs under systemd, so its own error is in the
    # journal rather than in this output. Print it here. `:` does
    # nothing, so `|| :` keeps a missing journal from counting as
    # an error in this branch.
    journalctl -u bombyx-guest-egress.service -n 20 --no-pager -o cat \
        >&2 || :
    egress_warning "the egress rule did not load; its log is above."
else
    echo "bombyx: loaded the guest's egress rule, a backstop to" \
        "the VM host's firewall."
fi

# 5. THE HAND-OVER. bootstrap.sh is installed root-owned before
# the staging directory goes, and then runs as the agent.
if ! install -D -m 0755 -o root -g root "$STAGING/bootstrap.sh" \
    "$BOOTSTRAP"; then
    refuse "could not install bootstrap.sh at $BOOTSTRAP. The" \
        "error above says why."
fi

if ! rm -rf -- "$STAGING"; then
    refuse "could not remove $STAGING. The error above says why."
fi

# `sudo` keeps the working directory, and the one Vagrant starts
# in is the login account's home, which the agent may not be able
# to read.
if ! cd "$home"; then
    refuse "could not change into $home. The error above says why."
fi

# `sudo` clears the environment by default, keeping only the
# names `--preserve-env` lists, so BOMBYX_PRESERVE_ENV is what
# carries the Vagrantfile's variables across. A HOME in that list
# wins over the one `-H` sets, which is how a project's `[env]`
# HOME still moves the clone. `PATH` becomes sudo's
# `secure_path`; a project cannot set `PATH` anyway.
exec sudo -u "$user" -H --preserve-env="$BOMBYX_PRESERVE_ENV" -- \
    "$BOOTSTRAP"
