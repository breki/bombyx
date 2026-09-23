# Keeping agent VMs off your home network *(unverified)*

This is the optional network-isolation step for a VM host; set
the host up with [vm-host-setup.md](vm-host-setup.md) first.

By default an agent VM can reach far more of your network than
its purpose suggests, and nothing warns you about it. This page
explains what it can reach and how to cut that down.

What has actually been done, since the *(unverified)* in the
heading is doing real work. The rules have been applied and
persisted on a Linux host and on two WSL hosts. The guest checks
below have been run in their corrected form on the Linux host and
on the second WSL host; the first WSL host's results predate the
correction and [vm-host-wsl2.md](vm-host-wsl2.md) marks them
suspect. The rules have been confirmed to come back after a
reboot on the second WSL host only. Persistence is the part that
fails silently, so on any other host read "Making it survive a
reboot" below before relying on this. What the rules cannot do
is listed at the end, under "What this does and does not buy".

## What a VM can reach by default

vagrant-libvirt puts guests on a NAT'd network of its own --
`virbr1`, usually `192.168.121.0/24` -- on which **the VM host
itself is the gateway**. Because the host routes for the guest,
everything the host can reach, the guest can reach too. On a
typical workstation-turned-VM-host that includes:

- the home LAN the host sits on, and so the router, any NAS,
  printers and every other machine on it;
- a Tailscale or other overlay network, if one is configured;
- Docker networks on the host;
- other libvirt networks, and so sibling VMs;
- the host's own services, including `sshd` and libvirtd,
  because the gateway address is a normal address the guest can
  connect to.

That last point deserves emphasis. An agent VM exists on the
assumption that the code inside it may be hostile. The machine
that controls it should therefore not be one hop away with its
SSH port open, and by default it is.

Find out what applies to your host before changing anything:

```bash
virsh -c qemu:///system net-dumpxml vagrant-libvirt | grep -E 'bridge|ip address'
ip -4 -o addr show scope global      # every network the host is on
```

## Blocking it on the host with nftables

`scripts/agent-vm-firewall.sh` in the bombyx repository writes
the rules, detecting the bridge and gateway from the libvirt
network rather than assuming them. The rules themselves are not
reproduced here on purpose: a second copy drifts from the first,
and the stale one is the one someone pastes into a root shell.
Run `show` to see exactly what would be loaded on your host.

**Do not run it out of `/tmp`.** That directory is
world-writable, so between copying the file and running it under
`sudo` there is a window in which any other local account can
replace it -- and it then runs as root on the machine this whole
exercise is meant to protect. Install it somewhere only root can
write:

```bash
scp scripts/agent-vm-firewall.sh <host>:~/
ssh -t <host> 'sudo install -o root -g root -m 0755 \
    ~/agent-vm-firewall.sh /usr/local/sbin/agent-vm-firewall'

ssh <host> 'agent-vm-firewall show'          # changes nothing
ssh -t <host> 'sudo agent-vm-firewall apply'
ssh -t <host> 'sudo agent-vm-firewall persist'
```

`-t` gives the remote command a terminal, which `sudo` needs in
order to ask you for your password. `show` needs no `sudo`, so
it needs no `-t` either.

**Run the guest checks before `apply`, not only after.** They
are under "Checking that it worked" below, and several of them
mean nothing unless you have seen them report the unrestricted
answer first. So the order is: boot a guest, run that block,
then come back here for `apply` and `persist`.

If you have already applied and persisted, `revert` gets you
back to an unrestricted guest -- but it removes the systemd
unit and the rules file as well as the loaded table, so you are
then back before `apply`, not before `persist`. Getting where
you were takes both commands again, in order.

The VM host can also be the machine you are sitting at; bombyx
works that way whenever `host` names this machine. Then there
is nothing to copy, but the `install` step still applies:

```bash
scripts/agent-vm-firewall.sh show          # changes nothing
sudo install -o root -g root -m 0755 \
    scripts/agent-vm-firewall.sh /usr/local/sbin/agent-vm-firewall
sudo agent-vm-firewall apply
sudo agent-vm-firewall persist
```

The `show` line runs as you and changes nothing, so running
that one from the checkout is fine. The warning is about the
`sudo` lines, and running *those* straight out of the checkout
is not the same thing as installing first.
The requirement above is that only root can write the file, and
a working tree is writable by every process running as you --
in this repository that includes the agents that edit it. An
unprivileged write to `scripts/agent-vm-firewall.sh` then runs
as root the next time you type that `sudo` line, and whoever
made the write does not have to win a race against you. They
only have to wait. The `/tmp` warning names the worst case
rather than the only one.

Installing narrows that window to one moment rather than
closing it: `install` runs as root and copies whatever the file
holds right then, and the copy in `/usr/local/sbin` is what
runs as root at every later `apply`, `persist` and `status`. So
look at the file before you install it, and install from a
clean tree:

```bash
git status --porcelain scripts/agent-vm-firewall.sh   # expect no output
git diff scripts/agent-vm-firewall.sh
```

`show` is the default action and is read-only. `status` reports
whether the rules are loaded *and* still match the network they
were written for. `revert` removes the rules, the file and the
systemd unit.

The rest of this section explains what those rules do, which is
worth reading once even if you only ever run the script.

The rules live in an nftables table of their own, so libvirt's
rules are untouched and the whole thing comes out in one
command. The load is written as declare-then-delete inside a
single file, which makes it one transaction: if any rule fails
to parse, nftables rolls back and whatever was already in force
stays in force.

Four details are worth understanding rather than copying.

**Every blocking rule matches the guest bridge.** Traffic
arriving on the host's own LAN interface is never considered, so
applying these rules cannot interrupt the SSH session you are
applying them from. That property is what makes this safe to try
on a machine you only reach remotely.

**The `established,related` rule in the input chain is not
optional.** bombyx works by having the host connect *into* the
guest. The guest's replies arrive on the guest bridge and would
be caught by the final drop. Without that first rule, every
bombyx command that touches the VM stops working, and the cause
is not obvious from the symptom.

**Dropping DHCP and DNS silently strands the guest.** libvirt's
dnsmasq serves both from the gateway address, so those two
accepts are what keep the VM addressable and able to resolve
names at all. They are pinned to that one address rather than
written by port alone, so they do not also expose every other
resolver the host happens to run -- a container publishing port
53, for instance. DHCP gets one more address, the broadcast
`255.255.255.255`: a guest without a lease cannot address the
gateway yet, so its first DISCOVER and REQUEST are broadcasts,
and without that exception it never gets an address. Only lease
renewals are unicast to the gateway, which is why a guest that
already holds a lease hides the problem. The exception exposes no
DHCP server the gateway rule did not already reach: a broadcast
arrives only at a server listening on every address (which
includes the gateway's) or on the broadcast address itself, and
`ss -ulpn 'sport = :67'` on the host lists what is listening.

**Rules do not apply to connections that are already open.**
Both chains accept established traffic, so a guest that already
holds a socket to your LAN keeps it until it closes, and a
hostile guest chooses when that is. The script clears those
entries with `conntrack` after loading the rules; if `conntrack`
is not installed it says so, and restarting the guests achieves
the same thing.

To undo everything: `sudo agent-vm-firewall revert`.

## Checking that it worked

These run inside the VM, with `bombyx shell`. Read the next two
paragraphs first; the block itself comes after them.

How the probe is written decides whether its answer means
anything. The obvious test opens a connection and then reads
from it, as `cat </dev/tcp/host/port` does. Plenty of ports
accept a connection and send nothing until the client speaks
first: a web server waits for a request, and `sshd` sends one
line of banner and then waits as well. So `cat` sits there,
`timeout` kills it after three seconds, and the non-zero exit
status reports "blocked" for a port that answered. A probe
written that way prints the reassuring result whether or not
the rules are loaded. Opening the socket and stopping there
separates the two cases, and `exec 3<>` does that.

Paste the whole block below into an interactive shell in the
guest, substituting your own router and gateway addresses. It
defines `chk` and then calls it twice, so everything from
`chk() {` to its matching `}` is a definition rather than
something to run on its own -- paste it whole, because stopping
part-way leaves bash at a continuation prompt rather than
giving you an error. It needs bash: `/dev/tcp/host/port` is a
pseudo-path bash
invents, not a real device, so `sh` or `dash` will not do. Do
not put it in a script with `set -e` without changing it --
there the `msg=$(...)` assignment carries the probe's non-zero
status and ends the script before the `case` sees it. Replace
those two lines with one that keeps the status:

    msg=$(...) && status=0 || status=$?

Appending `|| true` instead does not work. It swallows the
status, so `status=$?` reads `true`'s zero and every probe
reports REACHABLE.

```bash
chk() {
  local msg status first
  msg=$(timeout 3 bash -c 'exec 3<>/dev/tcp/$1/$2' _ "$1" "$2" 2>&1)
  status=$?
  first=${msg%%$'\n'*}
  case $status in
    0)   echo "$3 ($1:$2): REACHABLE" ;;
    124) echo "$3 ($1:$2): no answer" ;;
    *)   case $msg in
           *"Connection refused"*) echo "$3 ($1:$2): refused" ;;
           *) echo "$3 ($1:$2): probe broken -- ${first##*: }" ;;
         esac ;;
  esac
}

curl -sS -m 5 https://example.com >/dev/null && echo "internet: ok"
getent hosts github.com >/dev/null && echo "dns: ok"
chk 192.168.1.1   80 "router"
chk 192.168.121.1 22 "host gateway sshd"
curl -6 -sS -m 5 https://example.com >/dev/null \
  && echo "ipv6: REACHABLE, unexpected" || echo "ipv6: refused, as intended"
```

Four things in that helper are worth a word each.

`exec 3<>` opens file descriptor 3 on the socket for reading
and writing, and then stops. Nothing reads from it, which is
the whole point.

The arguments after `bash -c '...'` become `$0`, `$1` and `$2`
inside it, so the `_` is a throwaway name for `$0` and the two
addresses arrive as arguments. Writing them into the quoted
string instead would hand bash text to re-parse, and an address
copied from a router page or a ticket would then be executed.

`status=$?` comes immediately after the probe, because any
command in between replaces the value. That includes an
innocent-looking assignment, which is why `first=` is set
afterwards rather than before.

Bash reports a refusal, a failed name lookup and an unreachable
network all with exit status 1, distinguishing them only in the
message it writes to stderr. So the helper keeps that message:
`${msg%%$'\n'*}` takes its first line, and `${first##*: }`
drops everything up to the last `": "`, leaving the part worth
reading. A mistyped address then prints `probe broken` rather
than quietly counting as a success.

**Start on the host, not in the guest.** `sudo agent-vm-firewall
status` prints the table that is actually loaded and confirms
the bridge it names is still the one libvirt uses. What follows
is a sanity check on top of that: it shows you what the rules
do to a real guest, and it is not the thing that tells you they
are loaded.

`status` does not compare the loaded rules against the ones
this script would generate now, so a table left over from an
older version passes it. That has happened here: a host ran a
predecessor ruleset for weeks whose DNS accept was not pinned
to the gateway, reporting green throughout. Until `status`
checks this itself, **read the table it prints against `show`**
whenever you have changed the script or cannot say when the
rules were last applied. Running `apply` again costs nothing
and settles it.

Run the block before `apply` as well as after, and compare the
two runs against the table below rather than against a rule of
thumb. Earlier versions of this section tried to give you one
sentence for reading a result, and each of them was wrong for a
host somebody had not thought of. The table is scoped instead,
and a result it does not cover is a question rather than an
answer.

On a host set up like the example above -- guest bridge
`virbr1`, gateway `192.168.121.1`, a LAN inside the private
ranges -- expect this:

| line | before `apply` | after `apply` |
|-|-|-|
| `router` | REACHABLE, or the router's own refusal | `refused` |
| `host gateway sshd` | REACHABLE | `no answer` |
| `internet: ok` | printed | printed |
| the IPv6 line | either | `refused, as intended` |

The router and gateway lines are the measurement, and they are
the two whose value is supposed to change. `internet: ok` is a
control: it is meant to be identical, and a change there means
the rules took away something they should have left alone.
`dns: ok` is absent from the table on purpose and is dealt with
below, as is the IPv6 line, whose value depends on whether the
guest has an IPv6 route at all.

A router line reading REACHABLE both times is the case to be
careful with, because it has two causes that look alike. Either
the probe is not measuring anything, or your LAN is outside the
ranges the rules cover -- a LAN on public IPv4 space is not
covered, which the limits at the end of this section explain.
Confirm which by checking the loaded table for the range your
router falls in, rather than by assuming the probe is at fault.

Read anything else against the loaded table itself rather than
against this page. `status` prints that table at the end of its
own output, so you have it already; `sudo nft list table inet
agentvm` is the same thing on a host where the script is not
installed. Two results come up often enough to name here. `refused` on the
gateway line means something answered, so the guest's packet
reached the host and the input chain's drop is not in force.
And `REACHABLE` on the router line after `apply` is what a LAN
numbered out of public IPv4 space gives, because the denylist
does not cover it -- see the limits at the end of this section.

`dns: ok` proves less than it appears to. It passes whenever
the guest resolves a name by any route at all, and a box image
that pins its own public resolvers answers without ever asking
the gateway. Run `resolvectl status` in the guest to see which
resolver it uses before reading that line as evidence about the
pinned DNS accept.

If the host runs another resolver -- a second libvirt network,
or a container publishing port 53 -- check that one too. The DNS
accepts are pinned to the gateway address, so no other resolver
on the host should answer. Do not use `chk` for it:
`chk` opens a TCP connection, and a resolver published on UDP
alone is silent over TCP whether or not any rules are loaded.
Ask over UDP instead, before `apply` as well as after. The
address below is the gateway of libvirt's own `default`
network, which is the second resolver most hosts have;
`ip -4 -o addr show scope global`, from the start of this
section, lists the others yours might run.

```bash
dig +timeout=3 +tries=1 @192.168.122.1 example.com
```

Expect an answer before, and `no servers could be reached`
after.

The IPv6 check matters because an IPv4-only test suite passes
happily while an IPv6 route to the same LAN devices stays open
-- which is why these rules refuse IPv6 outright rather than
listing private ranges. A guest with no IPv6 route at all
prints the same reassuring line, so it confirms the rules only
on a host where IPv6 reaches the guest bridge.

## Making it survive a reboot

Debian and Ubuntu ship an `/etc/nftables.conf` that begins with
`flush ruleset`. That single line causes two different problems,
and both are worth knowing because neither reports an error.

Folding these rules into that file wipes **libvirt's** rules as
well on any boot where libvirt started first, and VM networking
breaks in a way that looks unrelated to the change you made. So
`persist` writes its own systemd unit instead, and puts the
rules in `/etc/agent-vm-firewall/` rather than `/etc/nftables.d/`
-- some distributions have `nftables.conf` glob that second
directory, which would pull the rules back into the service
being avoided.

The other problem runs the other way. If `nftables.service` is
enabled and happens to start *second*, its `flush ruleset`
deletes this table and the guests are unrestricted, silently.
The unit is therefore ordered `After=nftables.service`. It is
deliberately not ordered against libvirt: the rules do not need
the bridge to exist, since nftables accepts an interface name
for an interface that is not there, and the daemon is called
`libvirtd` on some hosts and `virtnetworkd` on others.

**Reboot and run `sudo agent-vm-firewall status` afterwards.**
Persistence is the one part of this that cannot be confirmed
without a reboot, and a silent failure here leaves you believing
the guest is contained when it is not.

## What this does and does not buy

Five limits are worth stating plainly, because the rules look
more complete than they are.

**Guests on the same bridge can still reach each other.** Two
VMs on the same libvirt network exchange frames at layer 2,
which the `forward` hook never sees, so these rules do not
separate them. That matters when a scratch VM running untrusted
code sits beside a persistent project VM. Separating them needs
either a libvirt network per VM, or filtering in the `bridge`
family rather than `inet`. Guests on a *different* libvirt
network are blocked, because that traffic is routed.

**What is blocked is a list of address ranges, not "everything
private".** The IPv4 side covers the RFC1918 ranges, the
carrier-grade NAT range that Tailscale and similar use,
link-local, loopback and multicast. A LAN numbered out of public
IPv4 space is not covered, and neither is anything else that
does not appear in that list. IPv6 is handled differently and
more bluntly: *all* of it is refused from the guest bridge,
because a home network with native IPv6 gives the router and
every device a global address that no private-range list would
catch. That is only correct while the guest network is
IPv4-only, which the script checks and refuses to proceed
without.

**Only the one libvirt network is protected.** The rules name a
single bridge. A guest booted onto a different libvirt network
is not restricted at all, and `bombyx` does not stop a project
from defining one. `status` re-checks that the named bridge
still exists and still belongs to the network, because nftables
happily accepts a rule naming an interface that is gone -- which
would list perfectly while matching nothing.

**A guest cannot reach services on the host, including ones it
may want.** The input chain drops everything the guest starts.
NFS synced folders are the case to watch: a guest mounting an
export from the host will hang rather than fail clearly.
Vagrant's own share does not arise on a VM bombyx generates --
the Vagrantfile it renders disables the default one and adds
none -- but a mount the guest makes for itself still hangs, so
a provisioning script reaching back to the host is the case to
look for.

**The enforcement lives on the machine being protected.** A
guest that escalates to root on the VM host can remove these
rules. Enforcing the same policy at the router -- a separate
VLAN for agent VMs with an allowlist for outbound traffic --
does not have that weakness, because the device applying the
rules is not the device under attack.

Treat this as the version you can have today. It closes the
guest-to-LAN and guest-to-host paths, which is most of the
exposure, and it costs nothing to keep in place once a VLAN
exists.
