# Windows guest box

This project uses a Windows box only when it is built from an
official Microsoft source. That is a rule for the operator, not a
check: bombyx boots whatever box the config names. Microsoft publishes
no Vagrant box or libvirt image for Windows Server, so the recipe in
`boxes/windows-server-2025/` builds one from Microsoft's evaluation
ISO. This file records what that box holds, how it is built, and what
runs on a real VM host showed about it.

The Windows-guest work is tracked in GitHub issue #138, in parts that
are each an issue of their own. Issue #136 added the
`guest = "windows"` key. Issue #141 ports the guest scripts to
PowerShell: `account.ps1` and `bootstrap.ps1` create the agent's
account, place its secrets, clone the project and run its script.
Issue #144 is this box. Issue #137 makes `shell`, the secrets refresh
and the hook work on a Windows guest.

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
`guest = "windows"`. `up` then boots the VM, creates the agent's
account, installs git, clones the project and runs its `.ps1` script,
through `account.ps1` and `bootstrap.ps1` in
`crates/bombyx/templates/`.

The build needs `qemu-system-x86_64`, `qemu-img`, `xorriso`, `curl`,
`sha256sum` and `tar` on the VM host, the operator in the `kvm` group,
and vagrant's insecure public key. `build.sh` finds that key in
HashiCorp's vagrant package, under `/opt/vagrant/embedded`; with
vagrant from another source, set `VAGRANT_PUB` to the key's path. The
build works in `~/.cache/bombyx-box`, or in the folder given as its one
argument, and keeps the ISO there, so a second build downloads
nothing.

It runs unattended. A build on frosti, the maintainers' VM host, took
about 80 minutes, most of it installing updates, so the time changes
with how many updates are due that month.

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
   first-logon command runs once. The updates are not driven over ssh
   from `build.sh` either, because Windows' update API refuses a
   remote session.
3. **`stage.ps1`** installs every update Windows Update offers except
   drivers, and restarts, until none are left. The 2026-09-28
   build took four rounds: the September cumulative update, the .NET
   Framework update and a Defender update, then a restart Windows made
   itself, then a Defender platform update, then none.
4. **Then it sets up sshd, cleans up and generalizes**: it frees the
   superseded update files, trims the disk, and runs
   `sysprep /generalize`, which shuts the VM down. Generalizing strips
   what makes the install one particular machine: its security
   identifier, its computer name and its evaluation clock. The next
   boot, which is each VM's first, runs setup again as a new machine,
   and `unattend-oobe.xml` answers that setup's questions. `stage.ps1`
   deletes sshd's host keys first, so each VM generates its own too.

Each script reports its steps on the VM's first serial port, which
`build.sh` records in `serial.log` in its work folder. A progress line
starts with `bombyx: `. Two markers start a line with no prefix:
`BOMBYX-DONE`, written just before sysprep runs, and
`BOMBYX-FAILED: <why>`, written by any failed step, sysprep included.
So a sysprep failure leaves a `BOMBYX-FAILED` line after
`BOMBYX-DONE`. `build.sh` fails on any `BOMBYX-FAILED` line, and on a
log with no `BOMBYX-DONE`, which means the VM shut down early or the
time limit ran out.

A stuck build can be looked at through qemu's monitor socket,
`monitor.sock` in the `build.*` folder inside the work folder. The
monitor command `screendump <file>.ppm` writes a picture of the VM's
screen. Connecting to the socket needs a tool that speaks to a Unix
socket, such as `nc -U`; the exact flags were not checked.

The VM runs on emulated hardware: a q35 machine with a SATA disk and
an e1000e network card, which Windows drives without extra drivers,
and BIOS boot, so the VM host needs no UEFI firmware. The box's own
Vagrantfile asks libvirt for the same hardware.

## What the box ships

Read from a VM made from the box, on 2026-09-29:

- **sshd, running at boot**, OpenSSH 9.5, with vagrant's insecure key
  for the `vagrant` account. vagrant adds a generated key at a VM's
  first boot and uses that one, but the insecure keys stay accepted
  (#183), which is why `account.ps1` filters them out of the keys it
  gives the agent.
- **Password logins refused** (`PasswordAuthentication no`).
- **The firewall admits TCP only to sshd.** Windows' firewall keeps
  separate rules for three network profiles, Domain, Private and
  Public, and it puts the libvirt network in Public. SMB (445) and RPC
  (135) listen, but no enabled inbound rule admits them on that
  profile. The only other TCP allow rule, for Delivery Optimization,
  Windows' peer-to-peer download of updates, has nothing listening
  behind it. So over the network the well-known `vagrant` password
  meets sshd alone, which refuses it. This was read from the
  firewall's rules; no login was tried from another VM.
- **Host keys generated per VM** at its first sshd start, RSA, ECDSA
  and Ed25519 only; the DSA key is not offered.
- **The administrators block in `sshd_config` commented out.** With
  it, sshd reads every administrator's keys from one shared file,
  `C:\ProgramData\ssh\administrators_authorized_keys`. Without it,
  sshd reads the account's own `.ssh\authorized_keys`, which is where
  vagrant writes the key it swaps in, and where `account.ps1` writes
  the key it uses to log in as the agent. The stock config's
  `AllowGroups administrators "openssh users"` stays, so only members
  of those two groups may log in over SSH; the agent account that
  `account.ps1` creates is an administrator.
- **sshd's firewall rule open on every network profile.** Server 2025
  ships the rule limited to the Private profile, and the libvirt
  network is Public, where the port would stay closed and vagrant
  would time out.
- **Defender on**, real-time protection enabled, no exclusions. **UAC
  on.** Remote desktop off.
- **The built-in Administrator disabled.** Before sysprep,
  `stage.ps1` gives it a fresh random password, made in the guest and
  written nowhere, so any copy of the install's password left in the
  image is stale, and then disables it. `stage.ps1` also removes each
  answer file under `C:\Windows\Panther` that holds a password. An
  answer file that Windows lets `stage.ps1` read but not delete is
  named in `serial.log` and left in place; it holds the stale
  Administrator password and vagrant's well-known one. `vagrant` is
  the way in.
- **WinRM stopped and disabled**, and its firewall rules off, because
  bombyx reaches the guest over SSH alone.
- **Updates set to download only**, Windows' default, as SConfig,
  Server Core's text-mode settings menu, reports it. A VM downloads
  new updates but installs none by itself.
- **Nothing installed** beyond Windows: no programs in the uninstall
  list, and no service outside Windows' own folders. The kernel,
  `lsass`, `winlogon`, `sshd` and `powershell` all carry a valid
  `Microsoft Windows` signature.
- **git is not installed**; `account.ps1` installs it.

## Licence

The box is an evaluation, licensed for testing, not for routine use.

- **Each VM starts its own grace**, because sysprep resets it. On a
  VM made from the box, `account.ps1` reported at provisioning that
  the evaluation was not activated, with about 9 days left.
- **Unactivated, it shuts down after 10 days.** Microsoft's
  Evaluation Center page says: "Evaluation versions of Windows Server
  must be activated over the internet in the first 10 days to avoid
  automatic shutdown."
- **It may activate itself.** Another VM made from the box reported a
  `LicenseStatus` of 1, licensed, with 180 days left, when it was
  audited some minutes after its first boot. Nothing ran `slmgr` or
  entered a key: Windows' own automatic activation reached Microsoft
  through libvirt's NAT. How soon it does so was not measured. bombyx
  does not activate the guest itself; `account.ps1` prints the days
  left while a guest is unactivated.
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
- **A plain `ssh` command runs under sshd's default shell**, which is
  `cmd.exe` unless `HKLM:\SOFTWARE\OpenSSH\DefaultShell` names
  another. Which one this box uses was not checked. `account.ps1`'s
  hand-over command holds no character that `cmd.exe` or PowerShell
  reads, so it works under either.
- **PowerShell writes progress records to stderr as CLIXML**
  (`#< CLIXML` and an `<Objs>` block) when modules load for the first
  time. bombyx prints a host's stderr as the reason for a failure, so a
  guest command sets `$ProgressPreference = 'SilentlyContinue'` first.
- **git clones with MinGit's own ssh, not Windows'.** Driven by git
  over pipes from the hand-over's session, which has no console,
  Windows' `System32\OpenSSH\ssh.exe` authenticates, sends
  `git-upload-pack` and then moves no data, so the clone hangs.
  MinGit's `usr\bin\ssh.exe` completes. The hand-over itself still
  uses Windows' client, where no git pipe is involved.
- **Windows PowerShell 5.1 mangles two kinds of native argument.** It
  passes an argument holding a double quote without escaping it, and
  drops an empty-string argument altogether. `bootstrap.ps1` writes
  the ssh command with an escaped space instead of quotes, and passes
  `'""'` where git needs an empty value.
- **vagrant rewrites every `'` in a `vagrant ssh -c` command** as
  `'\''`, for PowerShell too (vagrant 2.4.9's `ssh_run.rb`). In `sh`
  that closes the quote, adds an escaped `'` and reopens it.
  PowerShell has no backslash escape, so it reads a closed string, a
  stray `\` and a new string, and the command breaks. bombyx sends
  each guest command as base64, run by `Invoke-Expression`, which
  holds no `'`.
- **A guest command line has a length limit.** vagrant encodes the
  command as `-encodedCommand`'s UTF-16 base64, which multiplies it
  by about 2.7, and sshd runs it through `cmd.exe`, whose documented
  limit is 8191 characters; where the limit bites was not measured.
  A command of about 20500 characters fails with `exec request failed
  on channel 0`. So bombyx drops a script's comments before encoding
  it, and encodes it as UTF-8 for `Invoke-Expression` rather than as
  UTF-16. `the_longest_windows_refresh_command_fits_the_guest_command_line`
  keeps the secrets refresh call, at the longest names the config
  accepts, under 7800. `bombyx shell` sends no script this way: it
  logs in with plain `ssh`, and its PowerShell is one
  `Set-Location`.
- **A session with a terminal always returns exit status 0.** Windows'
  sshd reports 0 for it, whatever the session exited with: measured
  for `vagrant ssh -c 'exit 3' -- -t`, for a loopback login inside it,
  and on 2026-10-06 for the direct login `bombyx shell` makes, `ssh -F
  <vagrant ssh-config> -t -l agent guest`, with a session ending
  `exit 3`. Without a terminal the status comes back: 3 on that same
  direct login. So a
  `bombyx shell` session on a Windows guest ends 0 however it ended,
  and only a failure before the session, or a dropped connection,
  ends non-zero.
- **Windows PowerShell 5.1 passes `""` for `$null`** to a .NET
  method's `string` parameter, so `[IO.File]::Replace` refuses its
  backup path as "not of a legal form". `refresh.ps1` passes
  `[NullString]::Value` instead.
- **`ssh.exe` joins its command's arguments with spaces**, dropping
  the quotes PowerShell put around one, so a path with a space
  reaches the far side split. `refresh.ps1` sends the agent's side
  as one `-EncodedCommand` instead.
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
