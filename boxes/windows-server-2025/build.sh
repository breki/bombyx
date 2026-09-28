#!/usr/bin/env bash
# Builds a libvirt vagrant box of Windows Server 2025 Standard
# Evaluation, Server Core, from Microsoft's own evaluation ISO, with
# every update Windows Update offers installed.
#
# Usage:
#   boxes/windows-server-2025/build.sh [WORKDIR]
#   vagrant box add --name bombyx/windows-server-2025 \
#       WORKDIR/windows-server-2025.box
#
# WORKDIR defaults to ~/.cache/bombyx-box. The ISO is kept there, so a
# second build downloads nothing. docs/windows-guest-box.md records
# what the box holds and why.
#
# Not a thin wrapper over `cargo xtask`, which CLAUDE.md asks of a
# shell script. That rule keeps one fix from having to land in a bash
# twin and a PowerShell twin. This script runs only on a Linux libvirt
# host, so it has no twin; it drives qemu, xorriso and tar.
#
# The only sources are Microsoft, for the ISO and for the updates
# Windows Update supplies, and vagrant's own insecure public key, read
# from the vagrant install on this machine.

set -euo pipefail

# Microsoft's English link for the ISO, from its Evaluation Center page.
# It redirects to build 26100.32230, Microsoft's January 2026 refresh.
# Microsoft publishes no hash, and no independent record of this file's
# hash was found, so the pin is the SHA-256 of our first HTTPS download
# from Microsoft, on 2026-09-28. A changed ISO fails here until someone
# re-pins it.
readonly ISO_URL='https://go.microsoft.com/fwlink/?linkid=2345730&clcid=0x409&culture=en-us&country=us'
readonly ISO_SHA256='7b052573ba7894c9924e3e87ba732ccd354d18cb75a883efa9b900ea125bfd51'
readonly ISO_NAME='26100.32230.SERVER_EVAL_x64FRE_en-us.iso'

# The disk's virtual size. Its file grows only as Windows writes.
readonly DISK_GB=128
# The install, the update rounds and sysprep; a build that runs past
# this has hung.
readonly BUILD_TIMEOUT_S=14400

here=$(cd "$(dirname "$0")" && pwd)
work=${1:-$HOME/.cache/bombyx-box}

say() { printf 'build: %s\n' "$*"; }
die() { printf 'build: %s\n' "$*" >&2; exit 1; }

for tool in curl sha256sum qemu-system-x86_64 qemu-img xorriso tar; do
    command -v "$tool" >/dev/null || die "$tool is not installed"
done
[ -r /dev/kvm ] && [ -w /dev/kvm ] ||
    die "/dev/kvm is not usable; join the kvm group"

# vagrant's insecure public key, from the newest vagrant gem installed.
vagrant_pub=${VAGRANT_PUB:-$(find /opt/vagrant/embedded/gems/gems \
    -path '*/vagrant-*/keys/vagrant.pub' 2>/dev/null | sort -V | tail -n 1)}
[ -f "$vagrant_pub" ] ||
    die "vagrant's insecure key was not found; set VAGRANT_PUB to its path"

mkdir -p "$work"
iso="$work/$ISO_NAME"
if [ ! -f "$iso" ]; then
    say "downloading $ISO_NAME from Microsoft"
    curl -fL --proto '=https' --proto-redir '=https' -o "$iso.part" "$ISO_URL"
    mv "$iso.part" "$iso"
fi
say "checking the ISO's SHA-256"
actual=$(sha256sum "$iso" | cut -d' ' -f1)
[ "$actual" = "$ISO_SHA256" ] ||
    die "$iso has SHA-256 $actual, and the pin is $ISO_SHA256; refusing it"

build=$(mktemp -d "$work/build.XXXXXX")
trap 'rm -rf "$build"' EXIT

# The config CD: both answer files, both build scripts and the key.
# The Administrator password is random and written nowhere else, so
# nobody holds it; the vagrant account is the way in.
password="$(head -c 18 /dev/urandom | base64 | tr -d '/+=')aA1!"
mkdir "$build/cfg"
for answer in Autounattend.xml unattend-oobe.xml; do
    text=$(<"$here/$answer")
    text=${text//@ADMIN_PASSWORD@/$password}
    printf '%s\n' "$text" >"$build/cfg/$answer"
done
cp "$here/first-logon.ps1" "$here/stage.ps1" "$build/cfg/"
cp "$vagrant_pub" "$build/cfg/vagrant.pub"
xorriso -as mkisofs -quiet -V BOMBYXCFG -J -r -o "$build/cfg.iso" "$build/cfg"
rm -rf "$build/cfg"

qemu-img create -q -f qcow2 "$build/disk.qcow2" "${DISK_GB}G"

# The same hardware the box's Vagrantfile asks libvirt for: q35, a SATA
# disk and an e1000e card. BIOS boot, so no UEFI firmware is needed on
# the VM host. The empty disk falls through to the install CD, and
# later boots start from the disk. qemu exits when sysprep shuts the VM
# down. The VNC display and the monitor socket are local only, for
# watching a build or taking a `screendump` of a stuck one.
say "installing Windows; this takes a while (log: $work/serial.log)"
set +e
timeout "$BUILD_TIMEOUT_S" qemu-system-x86_64 \
    -machine q35,accel=kvm -cpu host -smp 2 -m 4096 \
    -rtc base=utc \
    -drive "file=$build/disk.qcow2,if=none,id=disk,format=qcow2,discard=unmap" \
    -device ide-hd,drive=disk,bus=ide.0 \
    -drive "file=$iso,if=none,id=install,media=cdrom,readonly=on" \
    -device ide-cd,drive=install,bus=ide.1 \
    -drive "file=$build/cfg.iso,if=none,id=cfg,media=cdrom,readonly=on" \
    -device ide-cd,drive=cfg,bus=ide.2 \
    -boot order=cd \
    -netdev user,id=net -device e1000e,netdev=net \
    -serial "file:$work/serial.log" \
    -display none -vnc 127.0.0.1:59 \
    -monitor "unix:$work/monitor.sock,server,nowait"
status=$?
set -e
[ "$status" -ne 124 ] ||
    die "the install ran past ${BUILD_TIMEOUT_S} s; see $work/serial.log"
[ "$status" -eq 0 ] || die "qemu exited with status $status"
if grep -q 'BOMBYX-FAILED' "$work/serial.log"; then
    die "$(grep 'BOMBYX-FAILED' "$work/serial.log" | tail -n 1)"
fi
grep -q 'BOMBYX-DONE' "$work/serial.log" ||
    die "the VM shut down before stage.ps1 finished; see $work/serial.log"

say "packaging the box"
# The copy drops the clusters Windows never wrote.
qemu-img convert -O qcow2 "$build/disk.qcow2" "$build/box.img"
rm "$build/disk.qcow2"
printf '{"provider":"libvirt","format":"qcow2","virtual_size":%d}\n' \
    "$DISK_GB" >"$build/metadata.json"
cp "$here/Vagrantfile" "$build/Vagrantfile"
box="$work/windows-server-2025.box"
tar -C "$build" -cf "$box.part" metadata.json Vagrantfile box.img
mv "$box.part" "$box"
say "built $box"
say "SHA-256 $(sha256sum "$box" | cut -d' ' -f1)"
say "add it with: vagrant box add --name bombyx/windows-server-2025 $box"
