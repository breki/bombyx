# Running the VM host in WSL2

This describes how to use a WSL2 distribution on a Windows
machine as a bombyx VM host, and the ways it behaves differently
from the dedicated Linux host that
[vm-host-setup.md](vm-host-setup.md) describes: nested
virtualization, installing the distribution, the guest bridge
surviving a restart, Vagrant treating WSL as Windows, WSL
stopping idle distributions, and reaching the host without
opening a port.

Two arrangements are covered. In the first, the Windows machine
is also the workstation: bombyx runs on Windows and drives WSL on
the same machine. In the second, the Windows machine is only the
VM host, and bombyx runs on another workstation that reaches it
over the network. The sections apply to both unless they say
otherwise; "Reaching the host without opening a port" is where
the two part ways.

Read that page first. Everything in it applies here: the same
packages, the same Vagrant repository, the same provider plugin,
the same non-interactive `PATH` trap. This page covers only what
is different, and each difference is a failure you would
otherwise spend an afternoon diagnosing.

This page is deliberately detailed, including findings still
marked *(unverified)*: each is a WSL-specific trap that is
expensive to rediscover, so the detail is kept rather than
trimmed for length.

> **Verified end to end on 14 August 2026** against Windows 11
> (build 26200.9168), WSL 2.7.11 with kernel 6.18.33.2, Ubuntu
> 24.04.4, libvirt 10.0.0, QEMU 8.2.2, Vagrant 2.4.9 and
> vagrant-libvirt 0.12.2, on an Intel i7-1260P. Every command
> here was run, and a Debian 13 guest was booted, provisioned and
> compiled inside it. Steps marked *(unverified)* were not.
>
> **The second arrangement was verified on 23 September 2026**
> against a separate Windows 11 machine (build 26200.9457), WSL
> 2.7.14 with kernel 6.18.33.2, Ubuntu 24.04 imported from
> Canonical's WSL image, and the same libvirt, QEMU, Vagrant and
> vagrant-libvirt versions, driven from a Linux workstation with
> bombyx 0.7.0. `bombyx doctor` passed, and `bombyx up` built,
> provisioned and snapshotted a guest that kept running with no
> client connected.

## Whether to do this at all

A WSL2 host gives up the property bombyx exists to provide, and
it is worth being precise about which one.

The agent does **not** run in WSL. It runs in a QEMU guest
*inside* WSL, a genuine second virtual machine with its own
kernel, no view of the WSL filesystem beyond what the Vagrantfile
mounts, and no view of Windows at all. The enclosure around the
agent is the same kind of enclosure it would have on a dedicated
host.

What changes is what waits behind that enclosure. On a dedicated
host, code that escapes the guest lands on a spare Linux box
holding nothing. Here it lands on the machine holding your
password manager, your SSH keys and your browser profiles. The
containment is as strong as on a dedicated host. The difference
is the cost when it fails.

That makes a WSL2 host a good way to exercise bombyx, to develop
a Vagrantfile, or to work on code you have reason to trust. It is
a poor place to point genuinely untrusted work, and no amount of
configuration below changes that, because the hardware is shared
and that is the whole problem.

## Hardening the distribution

WSL is built to be porous toward Windows. Two features do almost
all of the damage, and both are switched off in one file.

`automount` mounts every Windows drive under `/mnt`, read and
write, as your Windows user. It is not a network share you have
to authenticate to; it is simply there. `interop` lets anything
in the distribution execute Windows binaries, so `powershell.exe`
run from inside WSL runs on Windows with your privileges.

Neither is needed by a machine that only ever answers SSH, so
`/etc/wsl.conf` in the VM-host distribution should read:

```ini
[boot]
systemd=true

[user]
default=youruser

[automount]
enabled=false

[interop]
enabled=false
appendWindowsPath=false
```

`systemd=true` is not optional. libvirtd, the guest network and
the storage pool are all systemd services, and without it none of
them start.

Check the hardening rather than assuming it, because a mount that
did not happen and a mount that was never attempted look
identical from inside:

```bash
mount | grep -E 'drvfs|9p'            # expect only /usr/lib/wsl/drivers
ls /proc/sys/fs/binfmt_misc/          # expect no WSLInterop entry
```

A read-only 9p mount of `/usr/lib/wsl/drivers` remains. That is
WSL's own plumbing for GPU drivers, it is mounted `ro`, and it is
not a route to your files.

Note that `/mnt/c` and `/mnt/d` still exist as **empty
directories** after `automount` is disabled. Testing a Windows
path for existence is therefore not a test of anything. Ask
`mount` instead.

## Nested virtualization

The guests are nested: Hyper-V runs WSL, WSL runs QEMU. On
Windows 11 this works without configuration, but confirm it
before installing anything, since nothing else on this page
matters if it is missing:

```bash
ls -l /dev/kvm                        # expect crw-rw---- root kvm
grep -oE 'vmx|svm' /proc/cpuinfo | sort -u
```

`/dev/kvm` is group `kvm`, so the login user needs to be in that
group exactly as on a dedicated host.

## Installing the distribution

A WSL distribution belongs to one Windows account: another
account, an administrator included, cannot list it or start it.
That makes the owning account part of the design, not a detail.

When the Windows machine is only the VM host, create a dedicated
**standard** (non-administrator) account to own the distribution,
and let the workstation log in as that account. A stolen
workstation key then reaches a standard account and the
distribution it owns, never an administrator. Installing WSL
itself needs an administrator once; running a distribution does
not.

`wsl --install -d Ubuntu-24.04` does not work for that account
over SSH. Run from a standard account's SSH session, it fails
with:

```
The requested operation requires elevation.
This operation requires an interactive window station.
Error code: Wsl/0x800705b3
```

`wsl --import` needs neither elevation nor a desktop, so install
from Canonical's WSL image instead, and check it against the
published checksum before importing:

```powershell
$base = 'https://cloud-images.ubuntu.com/wsl/releases/24.04/current'
$name = 'ubuntu-noble-wsl-amd64-wsl.rootfs.tar.gz'
$dir  = "$env:USERPROFILE\wsl"
New-Item -ItemType Directory -Force $dir | Out-Null
Invoke-WebRequest -UseBasicParsing "$base/$name" -OutFile "$dir\$name"
(Get-FileHash -Algorithm SHA256 "$dir\$name").Hash  # vs $base/SHA256SUMS
wsl --import Ubuntu-24.04 "$dir\Ubuntu-24.04" "$dir\$name" --version 2
```

An imported distribution has no first-run prompt, so create the
Linux user yourself as root and name it as the default in
`/etc/wsl.conf` (see "Hardening the distribution"). That user
needs `kvm` and `libvirt` and nothing more: package installs go
through `wsl -u root`, which only the owning Windows account can
run. Canonical's image also ships an `ubuntu` account in the
`sudo` group with its password locked; take it out of `sudo`
(`deluser ubuntu sudo`) so no unused account can gain root.

`wsl.exe` did not run from an SSH session at all in early Store
releases of WSL ("Store WSL isn't accessible from Session 0",
microsoft/WSL#9231). WSL 2.x runs it, called by its full path,
`C:\Program Files\WSL\wsl.exe`. Update with `wsl --update` before
anything else if `wsl --version` shows an older release.

## The guest bridge survives a distribution restart

This is the first failure specific to WSL, and its symptom points
away from its cause.

After the VM-host distribution has been stopped and started
again, `vagrant up` fails because the guest network is inactive,
and starting it by hand reports:

```
error: internal error: Network is already in use by interface virbr0
```

The network is inactive, yet libvirt says its bridge is in use.
Both statements are true. WSL runs a **single utility virtual
machine whose network namespace is shared by every
distribution**, and that namespace outlives `wsl --terminate`.
The bridge libvirt created therefore survives while libvirt's own
record of having created it does not, so the next libvirtd start
finds an interface it did not make and refuses to proceed.

A full `wsl --shutdown` clears it, because that stops the utility
machine as well. That is a workaround rather than a fix: WSL
stops idle distributions on its own, so the broken state returns
without anyone asking for it.

The fix is to delete any leftover bridge before libvirtd starts.
At that moment libvirtd is not running, so no `virbr*` interface
can be in legitimate use:

```bash
sudo tee /usr/local/sbin/wsl-clear-stale-virbr >/dev/null <<'EOF'
#!/bin/sh
set -e
ip -br link show type bridge 2>/dev/null | awk '{print $1}' | cut -d@ -f1 |
while read -r br; do
    case "$br" in
        virbr*) ip link delete "$br" 2>/dev/null || true ;;
    esac
done
exit 0
EOF
sudo chmod 0755 /usr/local/sbin/wsl-clear-stale-virbr

sudo mkdir -p /etc/systemd/system/libvirtd.service.d
sudo tee /etc/systemd/system/libvirtd.service.d/10-wsl-stale-bridge.conf \
  >/dev/null <<'EOF'
[Service]
ExecStartPre=-/usr/local/sbin/wsl-clear-stale-virbr
EOF
sudo systemctl daemon-reload
```

The leading `-` on `ExecStartPre` keeps a failure in the script
from blocking libvirtd itself.

Test it with `wsl --terminate <distro>` rather than
`wsl --shutdown`. Only the first reproduces the problem, and a
test that passes under the second proves nothing.

## Vagrant treats WSL as Windows

With interop disabled, every Vagrant command that loads a project
fails before any libvirt code runs:

```
Vagrant failed to initialize at a very early stage:
The executable 'cmd.exe' Vagrant is trying to run was not found
```

`vagrant --version` still works, which makes the failure look
intermittent. It is not. Vagrant asks each provider whether it is
usable in order to choose a default, and the Hyper-V provider
treats "running under WSL" as "running on Windows", so it shells
out to PowerShell for an administrator check
(`plugins/providers/hyperv/provider.rb` calling
`Platform.windows_admin?`). On a hardened distribution there is
no PowerShell to call.

Naming the provider means the probe never happens:

```bash
echo 'VAGRANT_DEFAULT_PROVIDER=libvirt' | sudo tee -a /etc/environment
```

`/etc/environment` rather than `~/.bashrc` or `~/.profile`,
and the reason is the same one behind the non-interactive `PATH`
trap in [vm-host-setup.md](vm-host-setup.md). bombyx runs
`ssh <host> "cd ... && vagrant ..."`, a shell that is neither
interactive nor a login shell and reads neither file. sshd applies
`/etc/environment` through PAM, which those commands do get.

**This setting is for the commands you type by hand.** bombyx
neither needs it nor uses it, and the rest of this section is
about why -- but make the edit anyway, because without it every
vagrant command you run yourself on this host fails with the
`cmd.exe` error above.

Every script bombyx sends begins by unsetting the five vagrant
variables that redirect a command, this one among them, because
a value on the VM host would otherwise point a `destroy` at the
wrong machine. bombyx then writes the
project's own `provider` back in front of each project
`vagrant` call except the teardown, so vagrant is named a
provider on all of them but `bombyx destroy`.

**`bombyx destroy` fails on this host (issue #111).** The
teardown is the one project call bombyx sends without a
provider, because naming one that a host cannot supply makes
vagrant refuse the destroy, and the directory removal runs only
after it. On a libvirt host that exemption keeps a misconfigured
project removable. Here it works the other way: the `unset`
clears the `/etc/environment` value and the teardown writes none
back, so vagrant loads with no provider named, falls back to
VirtualBox, and VirtualBox refuses under WSL:

```
Vagrant is unable to use the VirtualBox provider from the Windows Subsystem for
Linux without access to the Windows environment.
```

That was measured with bombyx 0.7.0 against a project whose
machine existed and was shut off, so on this host a refusal does
not mean there was no machine. bombyx stops at the failed step,
so the domain, its snapshot, the project directory and the two
generated files all stay behind, and no bombyx command can clear
them.

Clean up by hand in this order, and not the other way round.
The project directory is `<remote_root>/<project>` from your
`config.toml`, which is `~/vms/<project>` unless you changed
it:

```bash
ssh <host> "cd ~/vms/<project> && vagrant destroy -f"
ssh <host> "rm -rf ~/vms/<project>"   # only after the destroy
```

The destroy over `ssh` succeeds where bombyx's fails, and the
reason is the mechanism this section opened with: sshd applies
`/etc/environment` through PAM, so that command gets the
provider bombyx had cleared. Removing the directory first would
delete the Vagrantfile while the libvirt domain is still
defined, which leaves a machine running with nothing left to
point `vagrant` at.

`bombyx doctor` works on this host. It carries no provider at
all: its only vagrant call is `vagrant plugin list`, and on the
host above that call listed `vagrant-libvirt` with the variable
cleared, so every row passed. The command does not reach the
usability probe that breaks the teardown.

Confirm it the way bombyx will see it, not from a login shell:

```bash
ssh <host> 'echo "$VAGRANT_DEFAULT_PROVIDER"'
```

**The same vagrant command then works over SSH and fails from
`wsl.exe`, which is worth knowing before it confuses you.** PAM is
what applies `/etc/environment`, and only sshd goes through it.
A command run as `wsl.exe -d <distro> -- vagrant status` does not,
so the variable is unset, the Hyper-V probe runs, and it fails
with the `cmd.exe` error above -- on a host where bombyx is
working perfectly.

That matters when you drop into the distribution to debug
something by hand, because the failure looks like the fix never
took. Reproduce bombyx's environment rather than the convenient
one: go in over `ssh <host>`, or set the variable explicitly for
the command.

## WSL stops idle distributions, and running guests die with them

An agent VM is a QEMU process inside the distribution. When WSL
stops the distribution, that process is killed, and the evidence
is thin: the domain reports `shut off (unknown)`, the QEMU log
ends mid-startup with nothing about shutting down, and
`journalctl` shows a fresh systemd boot where the guest used to
be.

Two things make this hard to recognise. WSL does not start a
distribution when a connection arrives on a forwarded port, so a
perfectly healthy setup answers `Connection refused`. And
`uptime -s` inside WSL reports the **utility machine's** boot
time rather than the distribution's, so it cheerfully claims an
uptime spanning a restart that did happen. Trust
`journalctl -b` and look for a systemd startup sequence instead.

**WSL has two idle timers, and both have to be off.** Both live in
`.wslconfig` in the owning account's profile
(`%UserProfile%\.wslconfig`); the file is per Windows user.

- `instanceIdleTimeout`, under `[general]`, stops a distribution
  a set time after its last client leaves. The default is 15
  seconds. This is the timer that kills the guests.
- `vmIdleTimeout`, under `[wsl2]`, stops the whole WSL VM a set
  time after every distribution has stopped. The default is 60
  seconds.

Setting `vmIdleTimeout` alone changes nothing, because the
distribution timer fires first and the VM timer only starts
after it. `-1` disables each:

```ini
[general]
instanceIdleTimeout=-1

[wsl2]
vmIdleTimeout=-1
```

WSL reads the file when its VM starts, so the change applies from
the next start. Measured on the second arrangement: before the
change, the distribution restarted within about two minutes of
the last client leaving and the guest was shut off; after it, the
same boot survived three idle minutes.

Neither setting starts WSL after Windows restarts, and neither
needs to. Guests do not start on their own either, and the next
`bombyx up` starts the distribution by connecting to it.

Holding a client open also works --
`wsl -d <distro> --exec /usr/bin/sleep infinity` in a window left
open -- but it needs someone signed in as the owning account, so
it does not suit the second arrangement.

## Reaching the host without opening a port

The obvious arrangement is sshd listening on a spare port and an
SSH alias pointing at `127.0.0.1` through WSL's localhost
forwarding. It works, and there is a better option.

```
Host bombyx-wsl
    HostName bombyx-wsl
    User youruser
    IdentityFile ~/.ssh/bombyx-wsl
    IdentitiesOnly yes
    ProxyCommand wsl.exe -d <distro> -u root --exec /usr/sbin/sshd -i
```

`sshd -i` serves one session over stdin and stdout, the way inetd
did. Three things follow. `wsl.exe` starts the distribution if it
is not running, so a connection never fails merely because WSL
idled it out — which solves reachability, though not the guest
survival problem above. Nothing listens on a port, so the agent
guests cannot reach the port that controls them; they share this
machine's WSL network namespace, and a wildcard bind would put
sshd one hop from a VM assumed to be hostile. And bombyx sees an
ordinary SSH alias, so nothing in bombyx needs to know.

Two details. `HostName` is only a label for `known_hosts` here,
since ProxyCommand decides where the connection goes. And
disabling `ssh.service` removes `/run/sshd`, which the service
unit used to create, so `sshd -i` stops with "Missing privilege
separation directory" until a tmpfiles rule recreates it:

```bash
echo 'd /run/sshd 0755 root root -' | sudo tee /etc/tmpfiles.d/sshd.conf
sudo systemd-tmpfiles --create /etc/tmpfiles.d/sshd.conf
```

On Ubuntu 24.04 disable `ssh.socket` as well as `ssh.service`.
The socket unit is what listens on port 22, so disabling the
service alone leaves the port open. Check with `ss -ltn`.

### When bombyx runs on another machine

The `ProxyCommand` above runs `wsl.exe` on the workstation, so it
works only when the workstation is the Windows machine. From
another workstation, go through the Windows machine's own OpenSSH
server and have it run the same command:

```
Host wsl-win
    HostName <windows machine>
    User bombyx
    IdentityFile ~/.ssh/wsl-win
    IdentitiesOnly yes

Host bombyx-wsl
    HostName bombyx-wsl
    User bombyx
    IdentityFile ~/.ssh/bombyx-wsl
    IdentitiesOnly yes
    ProxyCommand ssh -T -o BatchMode=yes wsl-win
```

The first hop logs in as the dedicated standard account from
"Installing the distribution". On the Windows side, its key is
restricted to one command, so the proxy sends no command of its
own:

```
restrict,command="& 'C:\Program Files\WSL\wsl.exe' -d Ubuntu-24.04 -u root --exec /usr/sbin/sshd -i" ssh-ed25519 AAAA...
```

`restrict` turns off forwarding and the pty, and `command=`
replaces whatever the client asks to run. So the key can start
WSL's one-shot sshd and nothing else, and that sshd still demands
the second key. Measured on the verified machine: `whoami` sent
with the first key printed WSL sshd's protocol banner rather than
a user name, and a port forward was refused as "administratively
prohibited". Both keys need no passphrase, because bombyx connects
unattended; the restriction is what limits the first one.

The `& '...'` form is PowerShell's. Windows OpenSSH runs commands
through the shell named by `DefaultShell` under
`HKLM:\SOFTWARE\OpenSSH`, which was Windows PowerShell 5.1 on the
verified machine; adjust the command for `cmd` *(unverified)*.

Keep that key in a file only administrators can change, so the
account cannot lift its own restriction. In
`%ProgramData%\ssh\sshd_config`:

```
PasswordAuthentication no     # a global setting: above the first Match

Match User bombyx
       AuthorizedKeysFile __PROGRAMDATA__/ssh/bombyx_authorized_keys
```

The key file is owned by Administrators and writable only by
SYSTEM and Administrators. It also avoids creating
`C:\Users\bombyx\.ssh` by hand, which before the account's first
logon makes Windows give the account a second, suffixed profile
directory. Run `sshd -t` before restarting the service, so a
config error cannot take the server down.

Then narrow the firewall rule that installing OpenSSH Server
creates, which admits any address, to the machines that need it:

```powershell
Set-NetFirewallRule -Name OpenSSH-Server-In-TCP `
    -RemoteAddress <workstation>,<other machines>
```

The scope shuts the agent guests out as well, since their traffic
reaches Windows from WSL's address. Name every machine that
already uses the server; `sshd -t` catches a config error, not an
address left off this list.

Inside WSL, sshd gets its own lockdown in a file under
`/etc/ssh/sshd_config.d/`: `PasswordAuthentication no`,
`PermitRootLogin no` and `AllowUsers` naming the Linux user.

With the first key restricted, the workstation reaches WSL only
as the Linux user, which has no sudo. Root inside WSL, for a
package or the host firewall, goes through `wsl -u root` run as
the owning Windows account, for example on the Windows machine:

```powershell
runas /user:bombyx "wsl.exe -d Ubuntu-24.04 -u root --exec sh <script>"
```

## What this arrangement does not solve

The guest network exposure described under "Keeping agent VMs off
your home network" in [vm-host-firewall.md](vm-host-firewall.md) turns
out to be **smaller** on a WSL host than on a dedicated one, and
the reason is worth knowing rather than assuming either way.

**Read both measurements in this section as suspect**
*(unverified)*. The probe `docs/vm-host-firewall.md` published at
the time read from the socket after connecting, and that times
out on any port which waits for the client to speak first, so it
reported a blocked path for a port that had answered. Neither
run recorded which probe it used. If they used that one,
the error runs toward more exposure than this section describes
rather than less. Both results below are therefore open
questions, and the corrected helper is under "Checking that it
worked" in that document.

Measured on this setup: a guest could reach the internet and
resolve names, and could not open TCP to the router, to the
workstation's own LAN address, or to a Tailscale peer. Neither
could the WSL distribution itself, while its internet access
worked normally. So the block is WSL's own NAT and Hyper-V
firewall, one layer above libvirt, and it applies before any rule
you write.

Do not read that as "no exposure". It was four TCP probes, not a
proof; it says nothing about UDP or ICMP; and it depends on WSL's
default NAT mode. **Mirrored networking mode removes it**, since
the distribution then shares the Windows network namespace and
sits directly on your LAN.

`scripts/agent-vm-firewall.sh` has now been applied and persisted
on a WSL host, and it works: the rules load against `virbr1`, the
guest keeps internet and DNS, and the LAN and host-gateway paths
stay closed. Treat it as defence in depth here rather than the
primary barrier — which is a better position to be in than
relying on a NAT behaviour that a settings change would remove.

On the second arrangement the script was installed as a
root-owned `/usr/local/sbin/agent-vm-firewall`, then applied and
persisted, and probed from inside a guest with the corrected
probe: internet and DNS worked, the host gateway's sshd timed out
(exit 124, the input chain's drop), and the router, the Windows
machine's own sshd and the workstation were each refused (exit 1,
"Connection refused", the forward chain's reject). Reading the
rule counters needs root, so this rests on the replies rather than
on the counters.

One caveat carried over unchanged: `conntrack` is not installed
by default, so the script warns that connections opened before
the rules loaded are still allowed. Restarting the guests
achieves the same thing.

The systemd unit was verified by restarting it, which proves it
loads the rules file. Surviving a full restart of the
distribution is *(unverified)* -- confirming it means terminating
the distribution, which kills any running guest with it.

Guest disk images land in the distribution's virtual disk, which
grows and does not shrink when a VM is destroyed. Reclaiming that
space needs `wsl --manage <distro> --set-sparse true`, or an
export and re-import *(unverified)*.
