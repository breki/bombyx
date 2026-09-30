# Reads a KeePassXC master password for bombyx without bombyx
# seeing it.
#
# bombyx runs this as `sh -c <this> bombyx-vault <tty>` and joins
# its stdout straight to the stdin of `keepassxc-cli open`, with an
# OS pipe bombyx never reads. This script reads the password from
# the terminal, with echo off, and writes it into that pipe ahead
# of anything else. It then becomes `cat`, so every later line
# keepassxc-cli reads is a command bombyx wrote to this script.
# The password lives in this shell, never in bombyx.
#
# `printf` is a shell builtin, so the password is in no process's
# argument list.

tty=$1

# Echo is restored on every way out: the normal path below, and
# Ctrl-C while the operator is typing, which reaches this shell too
# because it shares the terminal's process group.
trap 'stty echo <"$tty" 2>/dev/null' EXIT
trap 'exit 130' INT TERM HUP
stty -echo <"$tty" 2>/dev/null
IFS= read -r pw <"$tty"
stty echo <"$tty" 2>/dev/null
trap - EXIT
printf '%s\n' "$pw"
unset pw
exec cat
