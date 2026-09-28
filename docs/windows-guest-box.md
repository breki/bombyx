# Windows guest box

bombyx uses a Windows box only when it is built from an official
Microsoft source. Microsoft publishes no Vagrant box or libvirt image
for Windows Server, so bombyx builds its own from Microsoft's
evaluation ISO, with the recipe in `boxes/windows-server-2025/`. This
file records what that box holds, how it is built, and what runs on a
real VM host showed about it.

The Windows-guest work is tracked in GitHub issue #138. #136 added the
`guest = "windows"` key, #141 ports the guest scripts to PowerShell,
#144 built this box, and #137 makes `shell`, the secrets refresh and
the hook work on a Windows guest.

## What the guest is for

A project whose build needs Windows, such as one that targets .NET
Framework and builds with MSBuild. Such a guest needs .NET Framework
and the Visual Studio Build Tools. It does not need a desktop, because
bombyx reaches every guest over SSH.

## Building and adding the box

On the VM host, from a clone of this repository:

```sh
boxes/windows-server-2025/build.sh
vagrant box add --name bombyx/windows-server-2025 \
    ~/.cache/bombyx-box/windows-server-2025.box
```

A project then names `box = "bombyx/windows-server-2025"` with
`guest = "windows"`. The build needs `qemu-system-x86_64`, `qemu-img`,
`xorriso`, `curl` and vagrant on the VM host, and the operator in the
`kvm` group. It keeps the ISO in `~/.cache/bombyx-box`, so a second
build downloads nothing. It runs unattended; one build on frosti took
about 80 minutes, most of it installing updates.

## What the box is

Windows Server 2025 Standard **Evaluation**, Server Core (no desktop),
patched to the month it is built. The build on 2026-09-28 reached
build 26100.33438, the September 2026 cumulative update.

- **Where it comes from.** The ISO is Microsoft's own, fetched over
  HTTPS from the link on Microsoft's [Evaluation Center page][eval25],
  which redirects to build 26100.32230, Microsoft's January 2026
  refresh. Microsoft publishes no hash for it, and no independent
  record of this file's hash was found, so `build.sh` pins the SHA-256
  of the first download and refuses a file that differs. The updates
  come from Windows Update, through Windows' own update service.
  vagrant's insecure public key is read from the vagrant install on
  the VM host. Nothing else enters the box.
- **Why Server 2025.** Microsoft's Server 2022 evaluation ISO was
  never refreshed and still carries the March 2022 build, four and a
  half years of fixes behind. The 2025 ISO was refreshed in January
  2026, so the build has months of updates to install rather than
  years. Server 2025 also ships the OpenSSH server installed.
- **Why the build installs updates.** A box as the ISO ships it would
  start every VM months behind on security fixes.

[eval25]: https://www.microsoft.com/en-us/evalcenter/download-windows-server-2025

## How the build works

`build.sh` installs Windows into a qcow2 disk with qemu, from the ISO
and a small config CD it makes with `xorriso`, then packages the disk
as a libvirt `.box`.

1. **Setup** reads `Autounattend.xml` from the config CD. It installs
   the ISO's first image onto one MBR partition and creates a
   `vagrant` administrator. `first-logon.ps1` checks the image is
   `ServerStandardEval`, `Server Core`, and fails the build otherwise.
2. **The first logon** runs `first-logon.ps1`, which registers
   `stage.ps1` as a startup task running as SYSTEM and restarts. A
   startup task, because updates restart Windows several times and a
   first-logon command runs once, and because Windows' update API
   refuses a remote session.
3. **`stage.ps1`** installs every update Windows Update offers,
   drivers excepted, and restarts, until none are left. The 2026-09-28
   build took four rounds: the September cumulative update, the .NET
   Framework update and a Defender update, then a restart Windows made
   itself, then a Defender platform update, then none.
4. **Then it sets up sshd, cleans up and generalizes**: it frees the
   superseded update files, trims the disk, and runs sysprep, which
   shuts the VM down. Generalizing gives each VM its own identity,
   host keys and evaluation grace from its first boot.

Each script reports its steps on the VM's first serial port, which
`build.sh` records in `~/.cache/bombyx-box/serial.log`. The last line
is `BOMBYX-DONE` or `BOMBYX-FAILED: <why>`.

The VM runs on emulated hardware: a q35 machine with a SATA disk and
an e1000e network card, which Windows drives without extra drivers,
and BIOS boot, so the VM host needs no UEFI firmware. The box's own
Vagrantfile asks libvirt for the same hardware.

## What the box ships

Read from a VM made from the box, on 2026-09-28:

- **sshd, running at boot**, OpenSSH 9.5, with vagrant's insecure key
  for the `vagrant` account. vagrant swaps it for a generated key at a
  VM's first boot.
- **Password logins refused** (`PasswordAuthentication no`), so the
  well-known `vagrant` password opens only the console.
- **Host keys generated per VM** at its first sshd start, RSA, ECDSA
  and Ed25519 only; the DSA key is not offered.
- **The administrators block in `sshd_config` commented out**, so
  sshd reads an administrator's keys from the account's own
  `.ssh\authorized_keys`, which is where vagrant's key swap and #141's
  hand-over key write. The stock config's
  `AllowGroups administrators "openssh users"` stays.
- **sshd's firewall rule open on every network profile.** Server 2025
  ships the rule limited to the Private profile, and Windows puts a
  VM's libvirt network in the Public one, where the port would stay
  closed and vagrant would time out.
- **Defender on**, real-time protection enabled, no exclusions. **UAC
  on.** Remote desktop off.
- **The built-in Administrator disabled**, with a random password
  nobody holds; `vagrant` is the way in.
- **Updates set to download only**, Windows' default, as SConfig
  reports it. A VM downloads new updates but installs none by itself.
- **Nothing installed** beyond Windows: no programs in the uninstall
  list, and no service outside Windows' own folders. The kernel,
  `lsass`, `winlogon`, `sshd` and `powershell` all carry a valid
  `Microsoft Windows` signature.
- **git is not installed**; #141's `account.ps1` installs it.

## Licence

The box is an evaluation, licensed for testing, not for routine use.

- **Each VM starts its own grace**, because sysprep resets it. On a
  VM made from the box, #141's `account.ps1` reported at provisioning
  that the evaluation was not activated, with about 9 days left.
- **Unactivated, it shuts down after 10 days.** Microsoft's
  Evaluation Center page says: "Evaluation versions of Windows Server
  must be activated over the internet in the first 10 days to avoid
  automatic shutdown."
- **It may activate itself.** Another VM made from the box reported a
  `LicenseStatus` of 1, licensed, with 180 days left, when it was
  audited some minutes after its first boot. Nothing ran `slmgr` or
  entered a key: Windows' own automatic activation reached Microsoft
  through libvirt's NAT. How soon it does so was not measured. bombyx
  does not activate the guest itself; #141's `account.ps1` prints the
  days left while a guest is unactivated.
- **After activation** it runs 180 days, and a Server evaluation can
  be converted to a licensed edition with a product key.
- **For routine use** the guest needs a licence of its own: a Visual
  Studio standard (annual) subscription, which covers Windows for
  development and testing, a retail licence per VM, or a licensed
  host. The monthly Visual Studio subscriptions cover SQL Server alone
  for dev/test, not Windows. The subscription terms were read from
  Microsoft's [pricing page](https://visualstudio.microsoft.com/vs/pricing/)
  on 2026-09-28; the retail and host options were not checked against
  Microsoft's terms. This is not legal advice.

## Facts that shape bombyx's Windows code

- **`vagrant ssh -c` runs Windows PowerShell 5.1**, because the box's
  Vagrantfile sets `config.winssh.shell = "powershell"`.
- **PowerShell writes progress records to stderr as CLIXML**
  (`#< CLIXML` and an `<Objs>` block) when modules load for the first
  time. bombyx prints a host's stderr as the reason for a failure, so a
  guest command sets `$ProgressPreference = 'SilentlyContinue'` first.
- **vagrant expands no `~` on a Windows guest**, so an upload's
  destination is relative to the login home.
- **The default `/vagrant` synced folder fails** on a Windows guest,
  because vagrant chooses rsync and the guest has none. bombyx's
  Vagrantfile disables the folder.

## Not checked

- The Hyper-V provider, and any VM host other than frosti.
- A build in any language but English (US).
- Memory the guest needs once git, the Build Tools and a real build
  are on it.
