# Running the VM host in WSL2

This describes how to use a WSL2 distribution on your Windows
workstation as a bombyx VM host, and the four ways it behaves
differently from the dedicated Linux host that
[vm-host-setup.md](vm-host-setup.md) describes.

Read that page first. Everything in it applies here: the same
packages, the same Vagrant repository, the same provider plugin,
the same non-interactive `PATH` trap. This page covers only what
is different, and each difference is a failure you would
otherwise spend an afternoon diagnosing.

> **Verified end to end on 14 August 2026** against Windows 11
> (build 26200.9168), WSL 2.7.11 with kernel 6.18.33.2, Ubuntu
> 24.04.4, libvirt 10.0.0, QEMU 8.2.2, Vagrant 2.4.9 and
> vagrant-libvirt 0.12.2, on an Intel i7-1260P. Every command
> here was run, and a Debian 13 guest was booted, provisioned and
> compiled inside it. Steps marked *(unverified)* were not.

## Whether to do this at all

A WSL2 host gives up the property bombyx exists to provide, and
it is worth being precise about which one, because the loss is
smaller than it first appears and larger than it is comfortable
to admit.

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
containment is equally strong; the consequence of failure is not.

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

Confirm it the way bombyx will see it, not from a login shell:

```bash
ssh <host> 'echo "$VAGRANT_DEFAULT_PROVIDER"'
```

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

**`vmIdleTimeout` in `.wslconfig` does not prevent this.** It was
measured at 24 hours and the distribution was gone within 100
seconds of the last client detaching, with no warning that the
setting had been ignored. Do not rely on it.

What works is holding a WSL client open for as long as you want
guests to live:

```powershell
wsl -d <distro> --exec /usr/bin/sleep infinity
```

Run it in a window you leave open, or from a shortcut in the
Startup folder if you want it at logon.

**A warning about automating that.** A scheduled task created
with `schtasks /SC ONLOGON` that launches a hidden `.vbs` through
`wscript.exe` is, in shape, exactly how malware persists, and
Microsoft Defender flags it as `Trojan:Win32/Commando.A!ml` on
the command line alone. The script's contents are never the
issue. If you want this automatic, prefer a plain shortcut in the
Startup folder that runs `wsl.exe` directly: no scripting host,
no persistence-entry command line, and it is removed by deleting
one file.

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

## What this arrangement does not solve

The guest network exposure described under "Keeping agent VMs off
your home network" in [vm-host-setup.md](vm-host-setup.md)
applies here unchanged, and is sharper: the gateway one hop from
the guest is your own workstation. The nftables rules in
`scripts/agent-vm-firewall.sh` close the guest-to-LAN and
guest-to-host paths and have not been applied to a WSL host
*(unverified)*.

Guest disk images land in the distribution's virtual disk, which
grows and does not shrink when a VM is destroyed. Reclaiming that
space needs `wsl --manage <distro> --set-sparse true`, or an
export and re-import *(unverified)*.
