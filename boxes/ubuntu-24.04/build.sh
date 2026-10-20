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
# fingerprint is accepted, so the keyserver need not be trusted to hand
# back the right key. It is trusted for one thing: a revocation of this
# key reaches the build only if the keyserver serves it.
readonly SIGNING_KEY_FPR='D2EB44626FDDC30B513D5BB71A5D6C4C7DB87C81'
# The same key's long ID, the last 16 hex digits of the fingerprint,
# which is what gpg's GOODSIG status line names.
readonly SIGNING_KEY_ID=${SIGNING_KEY_FPR: -16}
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
    timeout flock; do
    command -v "$tool" >/dev/null || die "$tool is not installed"
done
[ -r /dev/kvm ] && [ -w /dev/kvm ] ||
    die "/dev/kvm is not usable; join the kvm group"

# vagrant's insecure public key, from the newest vagrant gem installed.
# `|| true` keeps a missing /opt/vagrant from failing the assignment,
# which under `set -e` would end the script before the message below
# could name VAGRANT_PUB.
vagrant_pub=${VAGRANT_PUB:-$( { find /opt/vagrant/embedded/gems/gems \
    -path '*/vagrant-*/keys/vagrant.pub' 2>/dev/null || true; } |
    sort -V | tail -n 1)}
[ -f "$vagrant_pub" ] ||
    die "vagrant's insecure key was not found; set VAGRANT_PUB to its path"

mkdir -p "$work"
# One Ubuntu build at a time per work folder. Two would write the same
# image, log and box files there, and one could boot an image the
# other had just replaced.
#
# `exec 9>file` opens the lock file on file descriptor 9 and keeps it
# open for the rest of the script. `flock -n 9` locks it, or fails at
# once when another build holds the lock. The lock lasts while any
# process holds descriptor 9 open, so it ends when this script exits,
# provided no child still holds the descriptor; the qemu line below
# closes it for qemu with `9>&-`.
exec 9>"$work/ubuntu-24.04.lock"
flock -n 9 || die "another Ubuntu box build is running in $work"
# Named after the box, because the Windows build shares the default
# work folder and keeps its own serial.log there.
log="$work/ubuntu-24.04-serial.log"
build=$(mktemp -d "$work/build.XXXXXX")
trap 'rm -rf "$build"' EXIT

# Fetches $1 into $2 over HTTPS only, redirects included. Any further
# arguments go to curl: the small files pass `-sS`, which prints errors
# and nothing else, and the image passes `--progress-bar`, because a
# silent download of several hundred megabytes reads as a hang.
fetch() {
    local url=$1 out=$2
    shift 2
    curl -fL --proto '=https' --proto-redir '=https' "$@" -o "$out" "$url"
}

# The checksums and their signature are fetched fresh on every build,
# because the directory they come from moves to each new image.
say "fetching Canonical's checksums and their signature"
fetch "$IMAGE_BASE/SHA256SUMS" "$build/SHA256SUMS" -sS
fetch "$IMAGE_BASE/SHA256SUMS.gpg" "$build/SHA256SUMS.gpg" -sS
fetch "$KEYSERVER_URL" "$build/signing-key.asc" -sS

# gpg works in a home of its own inside the build folder, so the
# operator's keyring is neither read nor changed. Two of the lines
# `--status-fd` writes must both appear:
#
#   GOODSIG <long id>   a good signature by a key that is neither
#                       revoked nor expired. gpg writes REVKEYSIG or
#                       EXPKEYSIG instead for such a key, yet still
#                       writes VALIDSIG and exits 0, so VALIDSIG alone
#                       would accept a key Canonical has withdrawn.
#   VALIDSIG <fpr>      the full fingerprint of the signing key, which
#                       is what is pinned; GOODSIG carries only the
#                       long ID.
#
# gpg's exit status is not used, because it is 0 in the revoked and
# expired cases too.
#
# gpg's own messages are kept in the build folder and the last of them
# printed on failure, because one refusal covers several causes -- a
# truncated download, a bad signature, another key -- and they read
# alike from the outside.
mkdir -m 0700 "$build/gnupg"
gpg --homedir "$build/gnupg" --batch --quiet --import \
    "$build/signing-key.asc" 2>"$build/import.err" ||
    die "the signing key from $KEYSERVER_URL could not be imported:" \
        "$(tail -n 3 "$build/import.err")"
say "checking the signature on the checksums"
gpg --homedir "$build/gnupg" --batch --status-fd 1 \
    --verify "$build/SHA256SUMS.gpg" "$build/SHA256SUMS" \
    >"$build/verify.status" 2>"$build/verify.err" || true
{ grep -q "^\[GNUPG:\] GOODSIG $SIGNING_KEY_ID " "$build/verify.status" &&
    grep -q "^\[GNUPG:\] VALIDSIG $SIGNING_KEY_FPR " "$build/verify.status"
} || die "SHA256SUMS has no good signature by $SIGNING_KEY_FPR;" \
        "refusing it. gpg said: $(tail -n 3 "$build/verify.err")"

# `awk` compares the file-name field exactly and prints nothing when no
# line matches. A `grep` here would fail the assignment under `set -e`
# before the message below could say which file was missing.
want=$(awk -v name="*$IMAGE_NAME" '$2 == name { print $1 }' \
    "$build/SHA256SUMS")
[ -n "$want" ] || die "SHA256SUMS lists no $IMAGE_NAME"

# The image is kept between builds and fetched again only when the
# signed checksum names a different one. What is checked is a copy in
# the private build folder, and that copy is what boots, so the bytes
# checked are the bytes used even if the cached file changes meanwhile.
# The cached file itself stays as Canonical shipped it.
image="$work/$IMAGE_NAME"
disk="$build/disk.qcow2"
actual=
if [ -f "$image" ]; then
    cp "$image" "$disk"
    actual=$(sha256sum "$disk" | cut -d' ' -f1)
fi
if [ "$actual" != "$want" ]; then
    say "downloading $IMAGE_NAME from Canonical"
    fetch "$IMAGE_BASE/$IMAGE_NAME" "$image.part" --progress-bar
    mv "$image.part" "$image"
    cp "$image" "$disk"
    say "checking the image's SHA-256"
    actual=$(sha256sum "$disk" | cut -d' ' -f1)
fi
[ "$actual" = "$want" ] ||
    die "$image has SHA-256 $actual, and the signed checksum is $want"

# The seed volume. cloud-init's NoCloud source finds user-data and
# meta-data by the volume's label, CIDATA, whatever kind of device
# holds it, so the ISO works attached as an ordinary read-only disk.
# It is attached as a virtio disk, on the same bus as the boot disk; a
# CD drive would need an IDE or SATA controller added to the VM.
# stage.sh and the key ride on the same volume, and user-data runs
# stage.sh from it.
mkdir "$build/seed"
cp "$here/user-data" "$here/stage.sh" "$build/seed/"
cp "$vagrant_pub" "$build/seed/vagrant.pub"
printf 'instance-id: bombyx-box-build\nlocal-hostname: ubuntu\n' \
    >"$build/seed/meta-data"
xorriso -as mkisofs -quiet -V CIDATA -J -r -o "$build/seed.iso" \
    "$build/seed"
rm -rf "$build/seed"

qemu-img resize -q "$disk" "${DISK_GB}G"

# Virtio disk and network, which is what vagrant-libvirt gives a VM by
# default, so the box boots on the same hardware it was built on. qemu
# exits when stage.sh's work is done and cloud-init powers the VM off.
# The monitor socket is for taking a `screendump` of a stuck build. It
# sits in the build folder, which mktemp made private, because the
# monitor controls the VM. No VNC display: one on a TCP port would let
# any account on the VM host reach the build VM's console.
#
# `--foreground` keeps qemu in the terminal's process group. Without
# it, `timeout` moves qemu into a group of its own, and a Ctrl-C at the
# terminal never reaches qemu: the build runs on until the time limit.
# The option's cost is that on a timeout it kills only the command it
# started, not that command's children; qemu starts none, so nothing
# is left running. `9>&-` closes the lock's descriptor for qemu, so a
# build killed while qemu still runs does not leave the work folder
# locked.
say "updating Ubuntu; this takes a few minutes (log: $log)"
set +e
timeout --foreground "$BUILD_TIMEOUT_S" qemu-system-x86_64 \
    -machine q35,accel=kvm -cpu host -smp 2 -m 2048 \
    -drive "file=$disk,if=virtio,format=qcow2,discard=unmap" \
    -drive "file=$build/seed.iso,if=virtio,format=raw,readonly=on" \
    -netdev user,id=net -device virtio-net-pci,netdev=net \
    -serial "file:$log" \
    -display none \
    -monitor "unix:$build/monitor.sock,server,nowait" 9>&-
status=$?
set -e
[ "$status" -ne 124 ] ||
    die "the build ran past ${BUILD_TIMEOUT_S} s; see $log"
[ "$status" -eq 0 ] || die "qemu exited with status $status"
if grep -q '^BOMBYX-FAILED' "$log"; then
    die "$(grep '^BOMBYX-FAILED' "$log" | tail -n 1)"
fi
grep -q '^BOMBYX-DONE' "$log" ||
    die "the VM shut down without stage.sh reporting success; see $log"

say "packaging the box"
# The copy drops the clusters the guest freed, which stage.sh trimmed.
qemu-img convert -O qcow2 "$disk" "$build/box.img"
rm "$disk"
printf '{"provider":"libvirt","format":"qcow2","virtual_size":%d}\n' \
    "$DISK_GB" >"$build/metadata.json"
cp "$here/Vagrantfile" "$build/Vagrantfile"
box="$work/ubuntu-24.04.box"
tar -C "$build" -cf "$box.part" metadata.json Vagrantfile box.img
mv "$box.part" "$box"
say "built $box from image SHA-256 $actual"
say "box SHA-256 $(sha256sum "$box" | cut -d' ' -f1)"
say "add it with: vagrant box add --name bombyx/ubuntu-24.04 $box"
