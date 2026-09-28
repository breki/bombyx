# Windows guest box

bombyx does not build Windows guests yet. This file records the
box the Windows-guest work is built and tested against, how it was
chosen, and what a run on a real VM host showed about it.

The work is tracked in GitHub issue #138 and split into three
parts. #135 chose this box and is closed by the change that added
this file. #136 adds a Windows branch to provisioning: the config
key, the Vagrantfile and PowerShell versions of the guest scripts.
#137 makes `shell`, the secrets refresh and the hook work on a
Windows guest. Both build on the findings below.

## What the guest is for

A project whose build needs Windows, such as one that targets .NET
Framework and builds with MSBuild. Such a guest needs .NET Framework
and the Visual Studio Build Tools; it does not need a desktop,
because bombyx reaches every guest over SSH.

## The box

`gusztavvargadr/windows-server-2022-standard-core`, version
2607.0.0, libvirt provider, amd64.

- **What it is.** Windows Server 2022 Standard **Evaluation**, build
  20348, with no desktop (Server Core). It is built by the
  publisher's Packer templates
  ([gusztavvargadr/packer](https://github.com/gusztavvargadr/packer))
  from Microsoft's `SERVER_EVAL` ISO.
- **Why this one.** It is the smallest of the libvirt Windows boxes
  measured below: a 5.4 GB download, against 9.0 GB for Server 2025
  Core and 18.4 GB for Windows 11 from the same publisher. Server
  2022 includes .NET Framework 4.8, and the Build Tools need no
  desktop.

Sizes were measured from the Vagrant registry's download URLs on
2026-09-28:

| Box | Download |
|-|-|
| `gusztavvargadr/windows-server-2022-standard-core` 2607.0.0 | 5.4 GB |
| `jborean93/WindowsServer2022` 1.2.0 | 5.9 GB |
| `peru/windows-server-2019-standard-x64-eval` 20240201.01 | 7.2 GB |
| `gusztavvargadr/windows-server-core` (2025) 2607.0.0 | 9.0 GB |
| `peru/windows-10-enterprise-x64-eval` 20240201.01 | 11.3 GB |
| `gusztavvargadr/windows-11` 2607.1.0 | 18.4 GB |

## Licence

The box is an evaluation, licensed for testing, not for routine
use.

- **It arrives unactivated.** Windows reported a `LicenseStatus` of
  2, the out-of-box grace period, with 10 days left (read from the
  `SoftwareLicensingProduct` CIM class). The 180-day evaluation
  starts only on activation, which contacts Microsoft's servers.
  What the guest does when the 10 days run out unactivated was not
  checked.
- **After activation** it runs 180 days, and a Server evaluation can
  be converted to a licensed edition with a product key.
- **For routine use** the guest needs a licence of its own: a
  Visual Studio standard (annual) subscription, which covers Windows
  for development and testing, a retail licence per VM, or a
  licensed host. The monthly Visual Studio subscriptions cover SQL
  Server alone for dev/test, not Windows. The subscription terms
  were read from Microsoft's
  [pricing page](https://visualstudio.microsoft.com/vs/pricing/) on
  2026-09-28; the retail and host options were not checked against
  Microsoft's terms. This is not legal advice.

## What the box ships

Read from the publisher's source at the version above. The run
below confirmed sshd, the key, the shell and the RDP forward; the
disk, the network card and the firmware check rest on the source
alone.

- **OpenSSH server, enabled.** The first-boot script
  (`src/windows/vagrant/Autounattend.ps1`) installs vagrant's
  insecure key for the `vagrant` account and sets `sshd` to start
  on boot. The insecure key is vagrant's well-known default key
  pair; vagrant swaps it for a generated one on the first boot.
  WinRM is enabled too.
- **The box's own Vagrantfile** (`src/windows/vagrant/qemu.Vagrantfile`)
  sets `config.vm.guest = :windows`, `config.vm.communicator =
  'winrm'` and `config.winssh.shell = 'powershell'`. A Vagrantfile
  that wants SSH overrides the communicator with `winssh`, which is
  vagrant's SSH communicator for Windows guests; the Linux `ssh`
  communicator is a different one.
- **Emulated hardware, so no extra drivers.** It uses a SATA disk
  and an e1000e network card, which Windows drives out of the box.
  The faster paravirtual devices, virtio, need drivers that Windows
  does not include.
- **UEFI firmware on the VM host.** The box boots with OVMF, the
  UEFI firmware for QEMU/KVM, from the `ovmf` package on Debian and
  Ubuntu. The box's Vagrantfile looks for it at
  `/usr/share/OVMF/OVMF_CODE_4M.fd` and
  `/usr/share/OVMF/x64/OVMF_CODE.4m.fd`, and raises an error when
  neither exists.
- **A forwarded RDP port.** The box's Vagrantfile forwards guest
  port 3389, remote desktop, to host port 53389.

## What a run on frosti showed

Run on 2026-09-28 against frosti (vagrant 2.4.9, vagrant-libvirt
0.12.2), with no bombyx involved. The Vagrantfile was:

```ruby
Vagrant.configure(2) do |config|
  config.vm.box = "gusztavvargadr/windows-server-2022-standard-core"
  config.vm.box_version = "2607.0.0"
  config.vm.communicator = "winssh"
  config.vm.synced_folder ".", "/vagrant", disabled: true
end
```

- **It boots and vagrant reaches it over SSH.** The first
  `vagrant up` took 883 s including the download; a later boot took
  40 s. vagrant replaced the insecure key, as it does for a Linux
  guest. WinRM was not used.
- **`vagrant ssh -c` runs Windows PowerShell 5.1** (5.1.20348.2110),
  because of the box's `config.winssh.shell = 'powershell'`.
- **A plain `ssh` command runs under `cmd.exe`.** sshd has no
  `DefaultShell` set under `HKLM:\SOFTWARE\OpenSSH`, so it falls back
  to `cmd.exe`: `%COMSPEC%` expanded and `$PSVersionTable` came back
  as literal text. A command that reaches the guest this way crosses
  the VM host's `sh` and then `cmd.exe` before PowerShell sees it.
  Passing it as `powershell -NoProfile -EncodedCommand <base64>`
  keeps both from reparsing it, because base64 holds no character
  either reads. `-EncodedCommand` takes base64 of UTF-16LE text
  only; base64 of UTF-8 fails to decode.
- **The default `/vagrant` synced folder fails.** For this Windows
  guest vagrant chose rsync (the log says `Rsyncing folder`), and
  the guest has no rsync. On a Linux guest vagrant-libvirt uses NFS
  instead, which is the case the test comment in
  `disables_the_default_synced_folder` (`vagrantfile.rs`)
  describes. bombyx's Vagrantfile disables the folder either way.
- **PowerShell writes progress records to stderr as CLIXML**
  (`#< CLIXML` and an `<Objs>` block) when modules load for the first
  time. bombyx prints a host's stderr as the reason for a failure, so
  a guest command must set `$ProgressPreference = 'SilentlyContinue'`
  first.
- **The forwarded RDP port listens on every address.** Host port
  53389 was bound on `0.0.0.0` and `[::]`, tunnelled to the guest's
  RDP port, so the guest's remote desktop was reachable from the VM
  host's network. bombyx's generated Vagrantfile must switch that
  forward off, and #136 tracks it.
- **Resources.** 2 GB of memory by default, 921 MB of it in use at
  idle; 8.6 GB used of a 125 GB disk.
- **Tools.** .NET Framework 4.8 is installed (release key 528449).
  git is not. `New-LocalUser` is available, so an account can be
  created from PowerShell. The `vagrant` account is an
  administrator.

## Not checked

- The Hyper-V variant of the box, and any VM host other than frosti.
- Activating the evaluation, what happens when the 10-day grace
  runs out, and how the 180 days behave across `destroy` and `up`.
- Which RDP credentials the box accepts.
- Memory the guest needs once git, the Build Tools and a real build
  are on it.
