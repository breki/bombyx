# shellcheck shell=bash
# Shared port helpers for the dev/e2e scripts.
#
# Port-scoped, never by process name: a by-name kill
# (`taskkill /IM`, `pkill -x`) is machine-wide and would
# also terminate a production instance of the same binary
# on another port. These helpers stop only the process
# *listening* on a given port.
#
# Source this file; do not execute it:
#   . "$SCRIPT_DIR/lib/port-utils.sh"

# Echo the integer value for <key> from a .ports-format
# file ($1=file, $2=key, $3=default). Lines are `key=value`
# with optional spaces and `#` comments. Returns <default>
# if the file or key is absent, or the value is non-numeric.
read_port() {
    local file="$1" key="$2" def="$3" line val
    if [[ -f "$file" ]]; then
        while IFS= read -r line; do
            line="${line%%#*}"            # strip inline comment
            line="${line//[[:space:]]/}"  # strip spaces/tabs/CR
            if [[ "$line" == "$key="* ]]; then
                val="${line#*=}"
                if [[ "$val" =~ ^[0-9]+$ ]]; then
                    printf '%s' "$val"
                    return
                fi
            fi
        done <"$file"
    fi
    printf '%s' "$def"
}

# Stop the process listening on $1 (Windows / Git Bash).
free_port_windows() {
    local port="$1"
    powershell -NoProfile -Command \
        "Get-NetTCPConnection -LocalPort $port -State Listen \
         -ErrorAction SilentlyContinue | \
         Select-Object -ExpandProperty OwningProcess -Unique | \
         ForEach-Object { Stop-Process -Id \$_ -Force \
         -ErrorAction SilentlyContinue }" 2>/dev/null || true
}

# Stop the process listening on $1 (Unix). Filters to the
# LISTEN socket so an unrelated client connection to the
# same port is never killed.
free_port_unix() {
    local port="$1"
    local pids
    pids="$(lsof -ti "tcp:$port" -sTCP:LISTEN 2>/dev/null || true)"
    if [[ -n "$pids" ]]; then
        # shellcheck disable=SC2086 -- pids is a space list
        kill $pids 2>/dev/null || true
    fi
}

# Free $1 on the current platform. Ignores a non-numeric
# port (defence in depth: the port must never be
# interpolated into the PowerShell command as anything but
# digits).
free_port() {
    [[ "$1" =~ ^[0-9]+$ ]] || return 0
    if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" ]]; then
        free_port_windows "$1"
    else
        free_port_unix "$1"
    fi
}
