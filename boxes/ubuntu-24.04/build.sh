#!/usr/bin/env bash
# Builds a libvirt vagrant box of Ubuntu 24.04 LTS from Canonical's own
# cloud image, checked against Canonical's signature, with every update
# Ubuntu offers installed.
#
# Usage:
#   boxes/ubuntu-24.04/build.sh [WORKDIR]
#   vagrant box add --name bombyx/ubuntu-24.04 \
#       WORKDIR/ubuntu-24.04.box
#
# WORKDIR defaults to ~/.cache/bombyx-box, which the Windows build
# shares. docs/ubuntu-guest-box.md records what the box holds and why.
#
# Not a thin wrapper over `cargo xtask`, which CLAUDE.md asks of a
# shell script. That rule keeps one fix from having to land in a bash
# twin and a PowerShell twin. This script runs only on a Linux libvirt
# host, so it has no twin; it drives curl, gpg, qemu, xorriso and tar.
#
# The only sources are Canonical, for the image and for the updates
# Ubuntu's archive supplies, and vagrant's own insecure public key,
# read from the vagrant install on this machine.

set -euo pipefail

# Canonical's directory for the current 24.04 release image. It moves
# to each new image, so a build takes whatever is current; the
# signature below is what makes a moving source safe.
readonly IMAGE_BASE='https://cloud-images.ubuntu.com/releases/24.04/release'
readonly IMAGE_NAME='ubuntu-24.04-server-cloudimg-amd64.img'

# The key that signs Canonical's cloud-image SHA256SUMS: "UEC Image
# Automatic Signing Key <cdimage@ubuntu.com>". Ubuntu ships the same key
# in its ubuntu-keyring package, as
# /usr/share/keyrings/ubuntu-cloudimage-keyring.gpg, which is how this
# fingerprint was confirmed from a second source. The key itself is
# fetched from Ubuntu's keyserver; only a signature by this exact
# fingerprint is accepted, so the keyserver need not be trusted.
readonly SIGNING_KEY_FPR='D2EB44626FDDC30B513D5BB71A5D6C4C7DB87C81'
readonly KEYSERVER_URL="https://keyserver.ubuntu.com/pks/lookup?\
op=get&options=mr&search=0x$SIGNING_KEY_FPR"

# The disk's virtual size. The image ships with a small root disk;
# cloud-init grows the partition to fill this on the build's first
# boot, and its file grows only as the guest writes.
readonly DISK_GB=64
# The update run and the shutdown; a build that runs past this has
# hung.
readonly BUILD_TIMEOUT_S=3600

here=$(cd "$(dirname "$0")" && pwd)
work=${1:-$HOME/.cache/bombyx-box}

say() { printf 'build: %s\n' "$*"; }
die() { printf 'build: %s\n' "$*" >&2; exit 1; }

for tool in curl gpg sha256sum qemu-system-x86_64 qemu-img xorriso tar \
    timeout; do
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
build=$(mktemp -d "$work/build.XXXXXX")
trap 'rm -rf "$build"' EXIT

fetch() {
    curl -fsSL --proto '=https' --proto-redir '=https' -o "$2" "$1"
}

# The checksums and their signature are fetched fresh on every build,
# because the directory they come from moves to each new image.
say "fetching Canonical's checksums and their signature"
fetch "$IMAGE_BASE/SHA256SUMS" "$build/SHA256SUMS"
fetch "$IMAGE_BASE/SHA256SUMS.gpg" "$build/SHA256SUMS.gpg"
fetch "$KEYSERVER_URL" "$build/signing-key.asc"

# gpg works in a home of its own inside the build folder, so the
# operator's keyring is neither read nor changed. `--status-fd` reports
# VALIDSIG with the fingerprint of the key that made a good signature;
# requiring the pinned fingerprint there refuses a signature by any
# other key the keyserver might have handed back.
mkdir -m 0700 "$build/gnupg"
gpg --homedir "$build/gnupg" --batch --quiet --import \
    "$build/signing-key.asc" 2>/dev/null ||
    die "the signing key from $KEYSERVER_URL could not be imported"
say "checking the signature on the checksums"
gpg --homedir "$build/gnupg" --batch --status-fd 1 \
    --verify "$build/SHA256SUMS.gpg" "$build/SHA256SUMS" \
    >"$build/verify.status" 2>/dev/null || true
grep -q "^\[GNUPG:\] VALIDSIG $SIGNING_KEY_FPR " "$build/verify.status" ||
    die "SHA256SUMS is not signed by $SIGNING_KEY_FPR; refusing it"

want=$(grep " \*$IMAGE_NAME\$" "$build/SHA256SUMS" | cut -d' ' -f1)
[ -n "$want" ] || die "SHA256SUMS lists no $IMAGE_NAME"

# The image is kept between builds and fetched again only when the
# signed checksum names a different one.
image="$work/$IMAGE_NAME"
if [ ! -f "$image" ] ||
    [ "$(sha256sum "$image" | cut -d' ' -f1)" != "$want" ]; then
    say "downloading $IMAGE_NAME from Canonical"
    fetch "$IMAGE_BASE/$IMAGE_NAME" "$image.part"
    mv "$image.part" "$image"
fi
say "checking the image's SHA-256"
actual=$(sha256sum "$image" | cut -d' ' -f1)
[ "$actual" = "$want" ] ||
    die "$image has SHA-256 $actual, and the signed checksum is $want"

# The seed volume. cloud-init's NoCloud source reads user-data and
# meta-data from any volume labelled CIDATA, so the ISO is attached as
# a plain read-only disk: a virtio disk cannot be a CD drive. stage.sh
# and the key ride on the same volume, and user-data runs stage.sh from
# it.
mkdir "$build/seed"
cp "$here/user-data" "$here/stage.sh" "$build/seed/"
cp "$vagrant_pub" "$build/seed/vagrant.pub"
printf 'instance-id: bombyx-box-build\nlocal-hostname: ubuntu\n' \
    >"$build/seed/meta-data"
xorriso -as mkisofs -quiet -V CIDATA -J -r -o "$build/seed.iso" \
    "$build/seed"
rm -rf "$build/seed"

# A copy, so the downloaded image stays as Canonical shipped it.
cp "$image" "$build/disk.qcow2"
qemu-img resize -q "$build/disk.qcow2" "${DISK_GB}G"

# Virtio disk and network, which is what vagrant-libvirt gives a VM by
# default, so the box boots on the same hardware it was built on. qemu
# exits when stage.sh's work is done and cloud-init powers the VM off.
# The monitor socket is for taking a `screendump` of a stuck build. It
# sits in the build folder, which mktemp made private, because the
# monitor controls the VM. No VNC display: one on a TCP port would let
# any account on the VM host reach the build VM's console.
say "updating Ubuntu; this takes a few minutes (log: $work/serial.log)"
set +e
timeout "$BUILD_TIMEOUT_S" qemu-system-x86_64 \
    -machine q35,accel=kvm -cpu host -smp 2 -m 2048 \
    -drive "file=$build/disk.qcow2,if=virtio,format=qcow2,discard=unmap" \
    -drive "file=$build/seed.iso,if=virtio,format=raw,readonly=on" \
    -netdev user,id=net -device virtio-net-pci,netdev=net \
    -serial "file:$work/serial.log" \
    -display none \
    -monitor "unix:$build/monitor.sock,server,nowait"
status=$?
set -e
[ "$status" -ne 124 ] ||
    die "the build ran past ${BUILD_TIMEOUT_S} s; see $work/serial.log"
[ "$status" -eq 0 ] || die "qemu exited with status $status"
if grep -q '^BOMBYX-FAILED' "$work/serial.log"; then
    die "$(grep '^BOMBYX-FAILED' "$work/serial.log" | tail -n 1)"
fi
grep -q '^BOMBYX-DONE' "$work/serial.log" ||
    die "the VM shut down before stage.sh finished; see $work/serial.log"

say "packaging the box"
# The copy drops the clusters the guest freed, which stage.sh trimmed.
qemu-img convert -O qcow2 "$build/disk.qcow2" "$build/box.img"
rm "$build/disk.qcow2"
printf '{"provider":"libvirt","format":"qcow2","virtual_size":%d}\n' \
    "$DISK_GB" >"$build/metadata.json"
cp "$here/Vagrantfile" "$build/Vagrantfile"
box="$work/ubuntu-24.04.box"
tar -C "$build" -cf "$box.part" metadata.json Vagrantfile box.img
mv "$box.part" "$box"
say "built $box from image SHA-256 $actual"
say "box SHA-256 $(sha256sum "$box" | cut -d' ' -f1)"
say "add it with: vagrant box add --name bombyx/ubuntu-24.04 $box"
