# Setting up a VM host

This describes how to prepare a machine so that bombyx can
run agent VMs on it. The commands are written for Ubuntu
24.04 LTS. If you are on a different distribution, read the
whole page anyway and then see **Other distributions** at the
end, which explains what changes.

> **This is a written record, not a script, and that is
> deliberate.**
>
> Package names and repositories change between releases.
> Ubuntu 24.04, for example, removed the `vagrant` package
> completely and renamed `qemu-kvm`. When a document goes out
> of date you can see that it has, and you adapt. When a
> script goes out of date it fails half-way through, as root,
> having already changed some things and not others.
>
> Setting up the host is also not bombyx's job. bombyx
> composes `ssh`, `scp`, `tar` and `vagrant`, and nothing
> more. See "wrap, don't reimplement" in `README.md`.
>
> Steps 1 and 2 were verified on Ubuntu 24.04.4 with Vagrant
> 2.4.9 in August 2026. Steps marked *(unverified)* have not
> yet been run from start to finish.

## What has to be true

There are two separate sets of requirements. It is worth
keeping them apart, because they fail in different ways and
you fix them in different places.

### What bombyx itself needs

This list is short and it rarely changes. It is also exactly
what `bombyx doctor` checks, so you rarely need to work through
it by hand.

| Where | Requirement |
|-------|-------------|
| Host | `sshd` running, key auth working *non-interactively* |
| Host | A POSIX login shell (`bash` or `sh`, not `csh` or `fish`) |
| Host | `tar` and `scp` available on `PATH` |
| Host | `vagrant` available on the **non-interactive** `PATH` |
| Host | The project directory writable, or its nearest parent |
| Local | `ssh`, `scp` and `tar` on `PATH` |

### What the host needs in order to run VMs at all

This list is longer, and it changes more often between
distributions and releases. None of it is bombyx's concern.
If something here is wrong, `vagrant` is the thing that will
tell you so.

- Hardware virtualization support in the CPU, and a
  `/dev/kvm` device.
- QEMU and libvirt installed, with your user able to reach
  libvirt without `sudo`.
- Vagrant's `libvirt` provider plugin.
- Enough RAM and disk space for the VMs you plan to boot.

As a rough guide, a machine with 8 cores, 24 GiB of RAM and
about 300 GB of free disk comfortably hosts several agent VMs
at once.

## Check the machine before you install anything

All of these commands only read information. None of them
change the system.

```bash
grep -oE 'vmx|svm' /proc/cpuinfo | sort -u   # no output = VT off in BIOS
systemd-detect-virt                          # "none" = bare metal
ls -l /dev/kvm                               # missing = module not loaded
free -g && df -h /
```

`vmx` means an Intel CPU with VT-x, and `svm` means an AMD CPU
with AMD-V. If neither appears, hardware virtualization is
almost certainly switched off in the BIOS.

You can run VMs on a host that is itself a VM, but it will be
noticeably slower. A physical machine is the better choice for
a host that boots VMs on demand.

## Installing on Ubuntu 24.04

### Step 1: QEMU and libvirt

Note that the package is `qemu-system-x86`. The older name,
`qemu-kvm`, no longer exists in 24.04.

```bash
sudo apt update
sudo apt install -y qemu-system-x86 libvirt-daemon-system \
                    libvirt-clients virtinst
sudo usermod -aG libvirt,kvm "$USER"
```

That last command adds your user to two groups. Membership of
`libvirt` lets you create and control VMs without `sudo`.
Membership of `kvm` gives you access to `/dev/kvm`, which is
what makes the VMs run at close to full speed instead of
being emulated in software.

**The `-a` matters, and leaving it out is genuinely
dangerous.** With `-a`, the command adds these two groups to
the ones you are already in. Without it, `-G` *replaces* your
entire list of extra groups, so you would lose `sudo` and
anything else you had. On a machine you only reach over SSH,
that can lock you out of administering it.

Group membership is only read when a session starts, so log
out and log back in before you expect it to work. After that,
bombyx opens a new connection for every command, so it will
pick up the new groups on its own.

Two details that save time when something does not work:

- Ubuntu's `libvirt-daemon-system` package may have already
  added you to the `libvirt` group during installation. Run
  `id -nG` to see. Do not assume the `usermod` above is what
  put you there.
- Of the two groups, `libvirt` is the one that actually
  matters here. With the default `qemu:///system` connection,
  libvirtd starts QEMU as root, so your own access to
  `/dev/kvm` is not needed. Being in `kvm` is a sensible
  precaution rather than a requirement, and it only becomes
  necessary if you use `qemu:///session` or run QEMU
  directly.

### Step 2: Vagrant

**Ubuntu 24.04 does not package Vagrant at all.** Running
`apt install vagrant` reports that there is no installation
candidate, even with the `universe` component enabled. You
therefore need HashiCorp's own repository.

```bash
wget -O- https://apt.releases.hashicorp.com/gpg | \
  sudo gpg --dearmor -o /usr/share/keyrings/hashicorp-archive-keyring.gpg
echo "deb [arch=amd64 signed-by=/usr/share/keyrings/hashicorp-archive-keyring.gpg] \
https://apt.releases.hashicorp.com noble main" | \
  sudo tee /etc/apt/sources.list.d/hashicorp.list
sudo apt update && sudo apt install -y vagrant
```

This installs Vagrant under `/opt/vagrant` and puts a small
wrapper script at `/usr/bin/vagrant`. It is a shell script
rather than a symlink, but what matters is only that
`/usr/bin` is on the default `PATH`, which is why bombyx can
find it at all. The section on the non-interactive `PATH`
below explains why that matters so much.

### Step 3: The libvirt provider plugin

```bash
sudo apt install -y libvirt-dev ruby-dev gcc make pkg-config
vagrant plugin install vagrant-libvirt
```

Only the build dependencies need `sudo`. Vagrant keeps plugins
per user under `~/.vagrant.d`, so the second command is run as
yourself.

This plugin compiles native extensions against the Ruby that
ships inside Vagrant, which is the part most likely to fail.
On Ubuntu 24.04 with Vagrant 2.4.9 it built and linked without
any help, installing `vagrant-libvirt 0.12.2` in about a
minute. If the build cannot link on your machine, point it at
Vagrant's own libraries and try again:

```bash
CONFIGURE_ARGS='with-ldflags=-L/opt/vagrant/embedded/lib' \
  vagrant plugin install vagrant-libvirt
```

### The storage pool creates itself, but does not autostart

A fresh libvirt install has no storage pools at all, which
looks alarming if you check before booting anything:

```bash
virsh -c qemu:///system pool-list --all    # empty on a new host
```

You do not need to create one. vagrant-libvirt defines and
starts a pool named `default` the first time it boots a VM,
putting disk images in `/var/lib/libvirt/images`.

It does leave that pool with **autostart disabled**, which is
worth knowing because the consequence is delayed. Check with:

```bash
virsh -c qemu:///system pool-list --all
# Name      State    Autostart
# default   active   no          <-- will not come back after a reboot
```

After the host reboots, an inactive pool makes `vagrant up`
fail in a way that has nothing obviously to do with rebooting.
Turn it on once:

```bash
virsh -c qemu:///system pool-autostart default
```

## A warning that appears on every vagrant command

Once the provider is installed, every vagrant command prints a
line like this before doing anything else:

```
[fog][WARNING] Unrecognized arguments: libvirt_ip_command
```

It is harmless, and it is worth knowing that in advance because
it appears at the top of otherwise successful output, where a
reader naturally looks for the cause of a problem.

The warning comes from `fog-libvirt`, the library the provider
uses to talk to libvirt. `vagrant-libvirt` passes it an option
called `libvirt_ip_command` that current versions of
`fog-libvirt` no longer accept, so the library reports the
unrecognized argument and carries on. Nothing is skipped as a
result.

The cause is a version mismatch between two gems that
`vagrant plugin install` resolves separately: it installs the
newest `fog-libvirt` alongside whatever `vagrant-libvirt`
release you asked for. Seen with `vagrant-libvirt 0.12.2` and
`fog-libvirt 0.15.0` in August 2026, which is what a fresh host
set up from this page gets today.

Do not try to fix it by pinning `fog-libvirt` to an older
release. That means overriding dependency resolution inside
Vagrant's embedded Ruby, and a pin that resolves badly breaks
the provider completely -- a much worse outcome than one line
of noise. It will stop appearing when `vagrant-libvirt`
releases a version that no longer passes the option.

## Checking that it worked

The quickest check is `bombyx doctor`, run from a project
directory. It probes every precondition in this page's first
table plus the Vagrant provider plugin, changes nothing on the
host, and names each failure without offering a remedy — the
remedies are here:

```console
$ bombyx doctor
  local   tar               ok    tar 1.35 in C:\Program Files\Git\usr\bin
  vmhost  ssh               ok
  vmhost  login shell       ok    posix
  vmhost  vagrant           ok    /usr/bin/vagrant
  vmhost  libvirt provider  ok    vagrant-libvirt (0.12.2, global)
all checks passed
```

The manual equivalents are below, and remain useful when you
want to see the raw output or are setting up before bombyx is
installed. Run them from your workstation rather than on the
host itself. That is the point of them: they test the same path
that bombyx uses, instead of the more forgiving one you get
when you log in and type commands by hand.

```bash
ssh <host> true                 # the alias and key auth work
ssh <host> vagrant --version    # the important one
ssh <host> 'command -v tar'
ssh <host> 'vagrant plugin list'                    # see below
ssh <host> 'virsh -c qemu:///system net-list --all' # see below
```

If `vagrant --version` prints a version number over a plain,
non-interactive `ssh` call like that, bombyx has everything it
needs.

Two of those commands need care, because the obvious way to
write them tests the wrong thing.

**Always name the libvirt URI.** A bare `virsh list --all` run
as a non-root user connects to `qemu:///session`, a per-user
libvirt instance that is always reachable. It therefore passes
whether or not you have any access to the system daemon, which
is the one vagrant-libvirt actually uses. Check `virsh uri` if
you want to see which one you are getting. Passing
`-c qemu:///system` explicitly is what proves your `libvirt`
group membership works.

**Vagrant plugins are per-user.** Installing the provider with
`sudo vagrant plugin install` puts it in `/root/.vagrant.d`,
where the account bombyx connects as cannot see it.
`vagrant plugin list` then reports "No plugins installed" even
though the install appeared to succeed. Only the `apt install`
of the build dependencies needs `sudo`; the
`vagrant plugin install` must not have it.

## Why the non-interactive PATH causes trouble

This is the failure worth understanding properly, because
nothing in the system will report it for you.

bombyx runs commands of the form
`ssh <host> "cd ... && vagrant ..."`. A command passed to
`ssh` like this runs in a shell that is neither interactive
nor a login shell. Such a shell does not read `~/.profile`,
and it does not read the interactive part of `~/.bashrc`
either. Instead, Ubuntu gives it a fixed default `PATH`:

```
/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/usr/games:/usr/local/games:/snap/bin
```

A Vagrant installed in `/usr/bin`, which is what HashiCorp's
`.deb` gives you, is therefore visible. So is one installed
via snap, in `/snap/bin`. But a Vagrant in `~/.local/bin`, in
`~/bin`, or only in `/opt/vagrant/bin` is invisible.

The symptom is confusing, which is why it is worth knowing in
advance. Vagrant works perfectly when you SSH in and type
`vagrant` yourself, because that is an interactive login shell
with a fuller `PATH`. bombyx, using the same account on the
same machine, gets `bash: vagrant: command not found`. Worse,
it gets it in the middle of a push, after it has already
created the remote directory and copied a tarball across.

You can see the difference directly:

```bash
ssh <host> 'echo $PATH'                # what bombyx sees
ssh <host> 'bash -lc "echo \$PATH"'    # what you see on login
```

Vagrant cannot warn you about this itself, for the simple
reason that it is not running. To fix it, either install
Vagrant somewhere already on that default `PATH`, or add a
symlink into `/usr/local/bin`.

## Keeping agent VMs off your home network *(unverified)*

By default an agent VM can reach far more of your network than
its purpose suggests, and nothing warns you about it. This
section explains what it can reach and how to cut that down. The
rules below have not yet been applied to a running host, so
treat them as a starting point rather than a recipe that is
known to work.

### What a VM can reach by default

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

### Blocking it on the host with nftables

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
53, for instance.

**Rules do not apply to connections that are already open.**
Both chains accept established traffic, so a guest that already
holds a socket to your LAN keeps it until it closes, and a
hostile guest chooses when that is. The script clears those
entries with `conntrack` after loading the rules; if `conntrack`
is not installed it says so, and restarting the guests achieves
the same thing.

To undo everything: `sudo agent-vm-firewall revert`.

### Checking that it worked

Run these inside the VM, with `bombyx shell`. Substitute your
own router and gateway addresses.

```bash
curl -sS -m 5 https://example.com >/dev/null && echo "internet: ok"
getent hosts github.com >/dev/null && echo "dns: ok"
timeout 3 bash -c 'cat </dev/tcp/192.168.1.1/80'  || echo "LAN blocked: good"
timeout 3 bash -c 'cat </dev/tcp/192.168.121.1/22' || echo "host blocked: good"
curl -6 -sS -m 5 https://example.com >/dev/null \
  && echo "ipv6: REACHABLE, unexpected" || echo "ipv6: refused, as intended"
```

The last three are the point of the exercise: a VM that can
still open a connection to the router, or to the host's SSH
port, has not been contained. The IPv6 check matters because an
IPv4-only test suite passes happily while an IPv6 route to the
same LAN devices stays open -- which is why these rules refuse
IPv6 outright rather than listing private ranges.

### Making it survive a reboot

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

### What this does and does not buy

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
export from the host will hang rather than fail clearly. The
Vagrantfile in each project decides whether that applies, and
bombyx does not control it. Disabling the default synced folder
avoids the problem entirely, which is what the jutro VM does.

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

## Configuration for each project

Setting up the host is something you do once per machine.
After that, every project you want a VM for needs two things
of its own, kept in that project's own repository: a
`bombyx.toml` file and a `vagrant/` directory containing a
Vagrantfile.

bombyx does not ship either of them, because the project
repository is meant to be the source of truth for how its VM
is built. See `bombyx.toml.sample`, and the **Configure**
section of `README.md`. [tutorial.md](tutorial.md) writes both
files from scratch and boots the result, if you would rather
follow a worked example than assemble one.

One thing that does *not* go in the project's file is the name
of this host. `bombyx.toml` is committed and a VM host is
personal, so bombyx refuses a `host` key there and reads it
from a per-developer `config.toml` instead --
`~/.config/bombyx/config.toml`, or `%APPDATA%\bombyx` on
Windows. **Where bombyx looks for the host** in `README.md`
lists the four sources and their order.

## Other distributions

The two requirement lists near the top of this page are the
same everywhere. Only the package names and the installation
commands change.

On Fedora, the equivalent packages are `qemu-kvm`, `libvirt`
and `virt-install`, and Vagrant comes from HashiCorp's `dnf`
repository. On Arch, they are `qemu-full`, `libvirt` and
`vagrant`, and Vagrant *is* available from the official
repositories, so no extra repository is needed.

Whatever the distribution, two things are worth checking
carefully, because they are where the surprises tend to be:

1. Whether Vagrant is packaged at all, and if so, where its
   binary ends up. What matters is whether that location is
   on the non-interactive `PATH` described above.
2. Which group grants access to libvirt without `sudo`. It is
   called `libvirt` on some distributions, `libvirtd` or
   `kvm` on others.
