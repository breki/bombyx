# Windows guest box

bombyx does not build Windows guests yet. This file records the
box the Windows-guest work (GitHub issue #138) is built and tested
against, how it was chosen, and what a run on a real VM host showed
about it. Issues #136 and #137 build on these findings.

## The box

`gusztavvargadr/windows-server-2022-standard-core`, version
2607.0.0, libvirt provider, amd64.

- **What it is.** Windows Server 2022 Standard **Evaluation**, build
  20348, with no desktop (Server Core). It is built by the
  publisher's Packer templates
  ([gusztavvargadr/packer](https://github.com/gusztavvargadr/packer))
  from Microsoft's `SERVER_EVAL` ISO.
- **Why this one.** It is the smallest current Windows box the
  Vagrant registry lists for libvirt: a 5.4 GB download, against
  9.0 GB for Server 2025 Core and 18.4 GB for Windows 11 from the
  same publisher. An agent VM is reached over SSH, so a desktop buys
  nothing. Server 2022 includes .NET Framework 4.8. Visual Studio's
  IDE needs a desktop, so a Server Core guest builds with the
  command-line Build Tools.
- **Licence.** A 180-day evaluation, licensed for testing, and
  convertible to a licensed edition with a key. Issue #135 holds the
  licensing detail for routine use.

Sizes were measured from the registry's download URLs on
2026-09-28:

| Box | Download |
|-|-|
| `gusztavvargadr/windows-server-2022-standard-core` 2607.0.0 | 5.4 GB |
| `jborean93/WindowsServer2022` 1.2.0 | 5.9 GB |
| `peru/windows-server-2019-standard-x64-eval` 20240201.01 | 7.2 GB |
| `gusztavvargadr/windows-server-core` (2025) 2607.0.0 | 9.0 GB |
| `peru/windows-10-enterprise-x64-eval` 20240201.01 | 11.3 GB |
| `gusztavvargadr/windows-11` 2607.1.0 | 18.4 GB |

## What the box ships

Read from the publisher's source at the version above. The run
below confirmed sshd, the key, the shell and the RDP forward; the
disk, the network card and the OVMF check rest on the source alone.

- **OpenSSH server, enabled.** The first-boot script
  (`src/windows/vagrant/Autounattend.ps1`) installs vagrant's
  public key for the `vagrant` account and sets `sshd` to start on
  boot. WinRM is enabled too.
- **The box's own Vagrantfile** (`src/windows/vagrant/qemu.Vagrantfile`)
  sets `config.vm.guest = :windows`, `config.vm.communicator =
  'winrm'` and `config.winssh.shell = 'powershell'`. A Vagrantfile
  that wants SSH overrides the communicator with `winssh`, which is
  vagrant's SSH communicator for Windows guests; the Linux `ssh`
  communicator is a different one.
- **No virtio drivers needed.** It uses a SATA disk and an e1000e
  network card.
- **UEFI.** It boots with OVMF and raises an error when the VM host
  has none at `/usr/share/OVMF/OVMF_CODE_4M.fd` or
  `/usr/share/OVMF/x64/OVMF_CODE.4m.fd`.
- **A forwarded RDP port.** The box's Vagrantfile forwards guest
  port 3389 to host port 53389.

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
  40 s. vagrant replaced the box's insecure key with a generated one,
  as it does for a Linux guest. WinRM was not used.
- **`vagrant ssh -c` runs Windows PowerShell 5.1** (5.1.20348.2110),
  because of the box's `config.winssh.shell = 'powershell'`.
- **A plain `ssh` command runs under `cmd.exe`.** sshd has no
  `DefaultShell` set under `HKLM:\SOFTWARE\OpenSSH`, so it falls back
  to `cmd.exe`: `%COMSPEC%` expanded and `$PSVersionTable` came back
  as literal text. A command meant for PowerShell has to be passed
  as `powershell -NoProfile -EncodedCommand <base64 UTF-16LE>`, which
  no shell in between can reparse.
- **The default `/vagrant` synced folder fails**, because it uses
  rsync and the guest has none. bombyx's Vagrantfile already
  disables it (`vagrantfile.rs`, `disables_the_default_synced_folder`).
- **PowerShell writes progress records to stderr as CLIXML**
  (`#< CLIXML` and an `<Objs>` block) when modules load for the first
  time. bombyx prints a host's stderr as the reason for a failure, so
  a guest command must set `$ProgressPreference = 'SilentlyContinue'`
  first.
- **The forwarded RDP port listens on every address.** Host port
  53389 was bound on `0.0.0.0` and `[::]`, tunnelled to the guest's
  RDP port, so the guest's remote desktop was reachable from the VM
  host's network. Which password the RDP login accepts was not
  checked.
- **The evaluation arrives unactivated.** `LicenseStatus` was 2 (the
  out-of-box grace period) with 10 days left, so the 180 days start
  only on activation, which contacts Microsoft. Activation was not
  tried.
- **Resources.** 2 GB of memory by default, 921 MB of it in use at
  idle; 8.6 GB used of a 125 GB disk.
- **Tools.** .NET Framework 4.8 is installed (release key 528449).
  git is not. `New-LocalUser` is available, so an account can be
  created from PowerShell. The `vagrant` account is an
  administrator.

## Not checked

- The Hyper-V variant of the box, and any VM host other than frosti.
- Activating the evaluation, and how the 180-day period behaves
  across `destroy` and `up`.
- Which RDP credentials the box accepts.
- Memory the guest needs once git, the Build Tools and a real build
  are on it.
