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
# This script prints no prompt. keepassxc-cli writes its own
# "Enter password to unlock" to the terminal, and this script
# reads the answer.
#
# The first word after an `sh -c` script becomes `$0`, so
# `bombyx-vault` is only the name this shell's messages use, and
# the terminal to read from arrives as `$1`.
#
# `printf` is a shell builtin, so the password is in no process's
# argument list.

tty=$1

# Echo is restored on every way out: the normal path below; Ctrl-C
# while the operator is typing, which reaches this shell too
# because it shares the terminal's process group; and the SIGTERM
# bombyx sends when keepassxc-cli stopped before reading the
# password.
trap 'stty echo <"$tty" 2>/dev/null' EXIT
trap 'exit 130' INT TERM HUP
stty -echo <"$tty" 2>/dev/null
# `IFS=` keeps leading and trailing blanks and `-r` keeps
# backslashes, so the password arrives byte for byte.
IFS= read -r pw <"$tty"
stty echo <"$tty" 2>/dev/null
trap - EXIT
printf '%s\n' "$pw"
unset pw
exec cat
