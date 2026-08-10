#!/usr/bin/env bash
#
# Restricts what agent VMs can reach: outbound internet stays
# available, private destinations do not -- the host's LAN,
# overlay networks, Docker, other libvirt networks -- and the VM
# host's own services become unreachable from guests apart from
# DHCP and DNS.
#
# Run it on the VM host. Any Linux with nftables and libvirt.
#
# This is a shell script rather than a cargo xtask command
# because it runs on the VM host, where the bombyx repository
# and cargo are absent. docs/vm-host-setup.md carries the longer
# argument, including why host setup is a written record and not
# an installer.

set -euo pipefail

NETWORK="${NETWORK:-vagrant-libvirt}"
BRIDGE="${BRIDGE:-}"

TABLE="agentvm"
RULES_DIR="/etc/agent-vm-firewall"
RULES_FILE="$RULES_DIR/agentvm.nft"
UNIT_NAME="agent-vm-firewall.service"
UNIT_FILE="/etc/systemd/system/$UNIT_NAME"

# Deliberately not /etc/nftables.d: several distributions have
# /etc/nftables.conf glob that directory, which would load these
# rules through the same service whose leading `flush ruleset`
# this script exists to stay away from.

# Destinations a guest has no business reaching. This is a
# denylist of IPv4 ranges, so it covers RFC1918, CGNAT (where
# Tailscale and similar live), link-local, loopback and
# multicast -- and nothing else. A LAN on public IPv4 space is
# not covered; see "what this does not do" in
# docs/vm-host-setup.md.
BLOCKED_V4="10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16,
            169.254.0.0/16, 100.64.0.0/10, 127.0.0.0/8,
            224.0.0.0/4"

NETWORK_XML=""
GATEWAY=""

# --------------------------------------------------------------
# Output and preconditions
# --------------------------------------------------------------

# First argument is the reason, the rest are indented hints. The
# shape is explicit so a message's layout does not depend on how
# the source happened to be wrapped.
die() {
  echo "error: $1" >&2
  shift
  local hint
  for hint in "$@"; do
    echo "  $hint" >&2
  done
  exit 1
}

warn() { echo "warning: $*" >&2; }

usage() {
  local status="${1:-0}" stream=1
  [ "$status" -eq 0 ] || stream=2
  cat >&"$stream" <<'USAGE'
agent-vm-firewall.sh -- restrict what agent VMs can reach.
Run on the VM host; root is needed for everything but `show`.

  show      print the rules and change nothing (default)
  apply     load the rules now, until reboot
  status    report whether the rules are loaded and match
  persist   also load them at boot
  revert    remove the rules, the file and the unit

Environment:
  NETWORK   libvirt network name (default: vagrant-libvirt)
  BRIDGE    bridge interface; detected from NETWORK if unset
USAGE
  exit "$status"
}

require_root() {
  [ "$(id -u)" -eq 0 ] || die "must run as root" "try: sudo $0 $ACTION"
}

require_nft() {
  command -v nft >/dev/null 2>&1 ||
    die "nft not found" \
      "this needs nftables; on a host still using legacy" \
      "iptables, translate the rules by hand"
}

require_virsh() {
  command -v virsh >/dev/null 2>&1 ||
    die "virsh not found" "run this on the VM host, not the workstation"
}

# --------------------------------------------------------------
# Working out what to protect
# --------------------------------------------------------------

# Interface and network names end up inside generated nft
# syntax, so they are checked rather than trusted: a value
# containing a quote or a newline would otherwise inject rules
# into the file this script writes and loads as root.
valid_name() {
  case "$1" in
    '' | *[!A-Za-z0-9_.-]*) return 1 ;;
  esac
  [ "${#1}" -le 32 ]
}

net_attr() {
  printf '%s\n' "$NETWORK_XML" | grep -m1 -o "$1" | cut -d"'" -f2 || true
}

# Converts a dotted netmask to a prefix length.
mask_to_prefix() {
  local bits=0 octet
  local IFS=.
  # shellcheck disable=SC2086
  set -- $1
  for octet in "$@"; do
    case "$octet" in
      255) bits=$((bits + 8)) ;;
      254) bits=$((bits + 7)) ;;
      252) bits=$((bits + 6)) ;;
      248) bits=$((bits + 5)) ;;
      240) bits=$((bits + 4)) ;;
      224) bits=$((bits + 3)) ;;
      192) bits=$((bits + 2)) ;;
      128) bits=$((bits + 1)) ;;
      0) ;;
      *) return 1 ;;
    esac
  done
  echo "$bits"
}

guest_subnet() {
  local addr mask prefix a1 a2 a3 a4 m1 m2 m3 m4
  addr="$GATEWAY"
  mask="$(net_attr "netmask='[^']*'")"
  [ -n "$addr" ] && [ -n "$mask" ] || return 1
  prefix="$(mask_to_prefix "$mask")" || return 1
  local IFS=.
  read -r a1 a2 a3 a4 <<<"$addr"
  read -r m1 m2 m3 m4 <<<"$mask"
  printf '%d.%d.%d.%d/%s\n' \
    "$((a1 & m1))" "$((a2 & m2))" "$((a3 & m3))" "$((a4 & m4))" "$prefix"
}

# Loads the libvirt network definition and derives the bridge
# and gateway from it. virsh's own stderr is kept: the likeliest
# first-run failures are a polkit denial or a stopped libvirtd,
# and discarding that message leaves the operator guessing.
resolve_target() {
  require_virsh

  valid_name "$NETWORK" ||
    die "invalid NETWORK name: $NETWORK" \
      "letters, digits, dot, dash and underscore only"

  NETWORK_XML="$(virsh -c qemu:///system net-dumpxml "$NETWORK")" ||
    die "cannot read libvirt network '$NETWORK'" \
      "list them:  virsh -c qemu:///system net-list --all" \
      "then retry: NETWORK=<name> $0 $ACTION"

  GATEWAY="$(net_attr "ip address='[^']*'")"

  if [ -z "$BRIDGE" ]; then
    BRIDGE="$(net_attr "bridge name='[^']*'")"
  fi

  [ -n "$BRIDGE" ] ||
    die "could not determine the bridge for network '$NETWORK'" \
      "pass it explicitly: BRIDGE=virbr1 $0 $ACTION"

  valid_name "$BRIDGE" ||
    die "invalid BRIDGE name: $BRIDGE" \
      "letters, digits, dot, dash and underscore only"

  # The DHCP and DNS exceptions are pinned to this address, so
  # without it they would have to be written wide open -- every
  # resolver on every address the host holds.
  [ -n "$GATEWAY" ] ||
    die "network '$NETWORK' has no IPv4 address" \
      "this script assumes an IPv4 NAT network"

  case "$NETWORK_XML" in
    *"family='ipv6'"*)
      die "network '$NETWORK' has IPv6 configured" \
        "these rules reject all IPv6 from the guest bridge," \
        "which would break its addressing. Adapt them first."
      ;;
  esac
}

# --------------------------------------------------------------
# The rules
# --------------------------------------------------------------

print_ruleset() {
  cat <<NFT
# Generated by agent-vm-firewall.sh for libvirt network
# '$NETWORK' on bridge '$BRIDGE'. Remove with:
#   nft delete table inet $TABLE

# Declare-then-delete makes the whole load one transaction: if
# anything below fails to parse, nft rolls back and the rules
# already in force stay in force.
table inet $TABLE {}
delete table inet $TABLE

table inet $TABLE {
  chain forward {
    type filter hook forward priority -10; policy accept;

    ct state established,related accept

    # Outbound to the internet is left alone; private
    # destinations are refused. reject rather than drop, so a
    # blocked attempt fails at once instead of hanging.
    iifname "$BRIDGE" ip daddr { $BLOCKED_V4 } counter reject

    # All IPv6 is refused, not just unique-local and
    # link-local. A LAN with native IPv6 from the ISP gives
    # every device a global address, which a denylist of
    # private ranges would not cover. Valid because the guest
    # network is IPv4-only, which resolve_target checks.
    iifname "$BRIDGE" meta nfproto ipv6 counter reject
  }

  chain input {
    type filter hook input priority -10; policy accept;

    # Replies to connections this host started. Without this,
    # vagrant ssh stops working -- and so does every bombyx
    # command that touches a VM -- because the host connects
    # into the guest and the answers arrive here.
    ct state established,related accept

    # DHCP and DNS from libvirt's dnsmasq, pinned to the
    # gateway address so this does not expose every other
    # resolver the host happens to run.
    iifname "$BRIDGE" ip daddr $GATEWAY udp dport { 53, 67 } accept
    iifname "$BRIDGE" ip daddr $GATEWAY tcp dport 53 accept

    # Nothing else on this host is reachable from a guest:
    # not sshd, not libvirtd, not a published container port.
    iifname "$BRIDGE" counter drop
  }
}
NFT
}

# --------------------------------------------------------------
# Actions
# --------------------------------------------------------------

cmd_show() {
  resolve_target

  echo "libvirt network:  $NETWORK"
  echo "bridge:           $BRIDGE"
  echo "gateway (host):   $GATEWAY"
  echo "guest subnet:     $(guest_subnet || echo unknown)"
  echo "rules file:       $RULES_FILE"
  echo
  echo "Rules that apply would load:"
  echo
  print_ruleset
  echo
  echo "Nothing has been changed. Apply with: sudo $0 apply"
}

cmd_apply() {
  require_root
  require_nft
  resolve_target

  ip link show "$BRIDGE" >/dev/null 2>&1 ||
    die "bridge '$BRIDGE' does not exist" "is the libvirt network started?"

  local staged
  staged="$(mktemp)"
  # shellcheck disable=SC2064
  trap "rm -f '$staged'" EXIT

  print_ruleset > "$staged"

  # Checked before anything on the host changes. Without this,
  # a rule nft refuses to parse would leave the host with the
  # old table deleted, no new table, and the good rules file
  # already overwritten -- isolation silently gone.
  nft -c -f "$staged" ||
    die "the generated ruleset is not valid for this nft" \
      "nothing has been changed; see $staged content above"

  mkdir -p "$RULES_DIR"
  chmod 0755 "$RULES_DIR"
  install -m 0644 "$staged" "$RULES_FILE"
  nft -f "$RULES_FILE"

  echo "applied: table inet $TABLE on bridge $BRIDGE"

  # Rules do not affect connections that are already open,
  # because both chains accept established traffic. A guest
  # holding a socket to the LAN keeps it until it closes, and a
  # hostile guest chooses when that is.
  local subnet
  if subnet="$(guest_subnet)"; then
    if command -v conntrack >/dev/null 2>&1; then
      conntrack -D -s "$subnet" >/dev/null 2>&1 || true
      echo "cleared existing connections from $subnet"
    else
      warn "conntrack not installed: connections open before now
       are still allowed. Install conntrack, or restart the
       guests, for the rules to take full effect."
    fi
  fi

  echo
  echo "Verify from inside a VM (bombyx shell):"
  echo "  curl -sS -m 5 https://example.com >/dev/null && echo internet ok"
  echo "  getent hosts github.com >/dev/null && echo dns ok"
  echo "  timeout 3 bash -c 'cat </dev/tcp/<your-router>/80' || echo LAN blocked"
  echo "  timeout 3 bash -c 'cat </dev/tcp/$GATEWAY/22' || echo host blocked"
  echo
  echo "Lasts until reboot. Make it permanent: sudo $0 persist"
  echo "Undo it now:                           sudo $0 revert"
}

# Reports whether the rules are loaded *and still match the
# network they were written for*. `nft` accepts an iifname for
# an interface that does not exist, so a table naming a bridge
# that libvirt has since renamed matches nothing while still
# listing perfectly -- green in exactly the case where
# containment has evaporated.
cmd_status() {
  require_root
  require_nft

  local loaded
  loaded="$(nft list table inet "$TABLE" 2>/dev/null)" || {
    echo "table inet $TABLE is not loaded"
    return 1
  }

  local loaded_bridge
  loaded_bridge="$(printf '%s\n' "$loaded" |
    grep -m1 -o 'iifname "[^"]*"' | cut -d'"' -f2 || true)"

  echo "table inet $TABLE is loaded, bridge $loaded_bridge"

  if ! ip link show "$loaded_bridge" >/dev/null 2>&1; then
    echo
    echo "$loaded"
    die "bridge '$loaded_bridge' does not exist" \
      "the rules match nothing: guests are NOT contained" \
      "re-run: sudo $0 apply"
  fi

  # Only compared when virsh is usable, so status still works
  # on a host where libvirt is down.
  if command -v virsh >/dev/null 2>&1 &&
    NETWORK_XML="$(virsh -c qemu:///system net-dumpxml "$NETWORK" 2>/dev/null)"; then
    local current
    current="$(net_attr "bridge name='[^']*'")"
    if [ -n "$current" ] && [ "$current" != "$loaded_bridge" ]; then
      die "network '$NETWORK' is now on bridge '$current'" \
        "the loaded rules name '$loaded_bridge' and match nothing" \
        "re-run: sudo $0 apply"
    fi
  fi

  echo
  echo "$loaded"
}

cmd_persist() {
  require_root
  require_nft
  [ -f "$RULES_FILE" ] || die "no rules to persist" "run '$0 apply' first"

  local nft_bin
  nft_bin="$(command -v nft)"

  # Ordered after nftables.service, not after libvirtd.
  #
  # The hazard is that a distribution's /etc/nftables.conf opens
  # with `flush ruleset`; if that service runs second it wipes
  # this table and the guests are unrestricted, with no error
  # anywhere. libvirt is not in the ordering because these rules
  # do not need the bridge to exist -- nft accepts an iifname
  # for a missing interface -- and because the unit is called
  # libvirtd.service on some hosts and virtnetworkd.service on
  # others.
  #
  # ExecStop is prefixed with `-` so a stop where the table is
  # already gone is not recorded as a failure.
  cat > "$UNIT_FILE" <<UNIT
[Unit]
Description=Network isolation for agent VMs
After=nftables.service
After=network-pre.target
Wants=network-pre.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=$nft_bin -f $RULES_FILE
ExecStop=-$nft_bin delete table inet $TABLE

[Install]
WantedBy=multi-user.target
UNIT

  systemctl daemon-reload
  systemctl enable --now "$UNIT_NAME"
  echo "enabled $UNIT_NAME (loads $RULES_FILE after nftables.service)"
  echo
  echo "Reboot and run 'sudo $0 status' to confirm it survives."
}

cmd_revert() {
  require_root
  require_nft

  if [ -f "$UNIT_FILE" ]; then
    systemctl disable --now "$UNIT_NAME" >/dev/null 2>&1 || true
    rm -f "$UNIT_FILE"
    systemctl daemon-reload
    echo "removed $UNIT_FILE"
  fi

  if nft delete table inet "$TABLE" 2>/dev/null; then
    echo "removed table inet $TABLE"
  else
    echo "table inet $TABLE was not loaded"
  fi

  rm -f "$RULES_FILE"
  rmdir "$RULES_DIR" 2>/dev/null || true
}

# --------------------------------------------------------------
# Dispatch
# --------------------------------------------------------------
# Each action listed here has a matching cmd_<action> function,
# and only show and apply need libvirt: revert in particular
# must keep working on a host where the network has been removed.

ACTION="${1:-show}"
case "$ACTION" in
  -h | --help | help) usage 0 ;;
  show | apply | status | persist | revert) ;;
  *)
    echo "unknown action: $ACTION" >&2
    usage 1
    ;;
esac

"cmd_$ACTION"
