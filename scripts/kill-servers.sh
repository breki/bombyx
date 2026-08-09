#!/usr/bin/env bash
# Free the dev-server ports (backend + Vite) by stopping
# whatever process is *listening* on them.
#
# Port-scoped, never by process name -- see lib/port-utils.sh
# for why. Safe to run when nothing is listening.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/lib/port-utils.sh
. "$SCRIPT_DIR/lib/port-utils.sh"

# Dev-server ports. Backend (:3000) and Vite (:5173)
# defaults; the e2e harness frees its own ports separately.
BACKEND_PORT=3000
FRONTEND_PORT=5173

free_port "$BACKEND_PORT"
free_port "$FRONTEND_PORT"

sleep 1
echo "Freed dev ports $BACKEND_PORT and $FRONTEND_PORT."
