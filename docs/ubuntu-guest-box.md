# Ubuntu guest box

This project uses a box only when it comes from the OS vendor, or is
built from the vendor's own image. That is a rule for the operator, not
a check: bombyx boots whatever box the config names. Canonical
publishes no libvirt Vagrant box for current Ubuntu, so the recipe in
`boxes/ubuntu-24.04/` builds one from Canonical's cloud image. This
file records what that box holds and how it is built.

The work is GitHub issue #194. It replaces `generic/ubuntu2204`, a
box published by a third party with no new version since early 2024,
whose systemd lacks the DNS root's new signing key; issue #193 records
how that broke every name lookup in its guests.

## Building and adding the box

On the VM host, from a clone of this repository:

```sh
boxes/ubuntu-24.04/build.sh
vagrant box add --name bombyx/ubuntu-24.04 \
    ~/.cache/bombyx-box/ubuntu-24.04.box
```

A project then names `box = "bombyx/ubuntu-24.04"`. No `guest` key is
needed, because Linux is the default.

The build needs `curl`, `gpg`, `sha256sum`, `qemu-system-x86_64`,
`qemu-img`, `xorriso`, `tar` and `timeout` on the VM host, the operator
in the `kvm` group, and vagrant's insecure public key. `build.sh` finds
that key in HashiCorp's vagrant package, under `/opt/vagrant/embedded`;
with vagrant from another source, set `VAGRANT_PUB` to the key's path.
The build works in `~/.cache/bombyx-box`, or in the folder given as
its one argument. It keeps the image there and downloads it again only
when Canonical publishes a new one.

It runs unattended. The first build on frosti, the maintainers' VM
host, on 2026-10-20, took under two minutes: the 621 MB image
downloaded in about 15 seconds, the update boot took about a minute,
and packing took the rest. The box came out at 2.0 GB. A month-old
image has more updates to install, so expect a later build to take
longer.

## What the box is

Ubuntu 24.04 LTS, the server cloud image, updated to the day it is
built. Canonical supports 24.04 until 2029-05-31.

- **Where it comes from.** The image, its `SHA256SUMS` and the
  signature `SHA256SUMS.gpg` are Canonical's, fetched over HTTPS from
  `cloud-images.ubuntu.com/releases/24.04/release/`. The updates come
  from Ubuntu's archive, through `apt`. vagrant's insecure public key
  is read from the vagrant install on the VM host. Nothing else enters
  the box.
- **How the image is checked.** `build.sh` accepts `SHA256SUMS` only
  when it carries a good signature by the key with fingerprint
  `D2EB 4462 6FDD C30B 513D 5BB7 1A5D 6C4C 7DB8 7C81`, Canonical's
  "UEC Image Automatic Signing Key". It then checks the image's SHA-256
  against that file. The fingerprint is pinned in `build.sh` and was
  confirmed from a second source: Ubuntu's own `ubuntu-keyring` package
  ships the same key, as
  `/usr/share/keyrings/ubuntu-cloudimage-keyring.gpg`. The key itself
  is fetched from `keyserver.ubuntu.com`, but only a signature by that
  exact fingerprint counts, so the keyserver need not be trusted.
- **Why 24.04 and not 26.04.** Both carry the root key fix: 24.04 in
  `systemd 255.4-1ubuntu8.18`, 26.04 in `systemd 259`. 26.04 replaces
  `sudo` with `sudo-rs` and GNU coreutils with Rust's uutils by
  default, and `account.sh` and `bootstrap.sh` have never run against
  either. 24.04 keeps the GNU tools those scripts already run on.
- **Why the build installs updates.** The image is a snapshot. Without
  the update, every VM would start as far behind as the image is old.

## How the build works

`build.sh` makes a copy of the image, grows its disk to 64 GB, and
boots it once under `qemu` with a small seed volume attached. The seed
volume is an ISO labelled `CIDATA`, which cloud-init's NoCloud source
reads. It holds `user-data`, which tells cloud-init to run `stage.sh`
from the same volume and then power off, and vagrant's public key.

`stage.sh` runs as root inside that VM. It:

1. installs every update, keeping the image's own configuration files;
2. creates the `vagrant` account with the insecure key and
   passwordless `sudo`, which vagrant expects of a box;
3. replaces cloud-init's network file, which matches the build VM's
   MAC address, with one that asks for DHCP on any wired card, so a VM
   with another MAC still gets an address;
4. turns cloud-init off, so it does not search for a data source on
   every boot;
5. removes the SSH host keys and installs a unit that makes new ones on
   a VM's first boot, so no two VMs share keys;
6. empties `/etc/machine-id`, so each VM gets its own, and with it its
   own DHCP lease;
7. clears the package cache and trims free blocks, so the box packs
   small.

Its last line on the serial console is `BOMBYX-DONE`, or
`BOMBYX-FAILED` with the line that failed. qemu exits the same way in
both cases, because cloud-init powers the VM off either way, so
`build.sh` reads that line to tell them apart. It then converts the
disk and packs it with `metadata.json` and the box's `Vagrantfile`.

## Checked on a real VM host

On frosti, on 2026-10-20, a VM made from the first build booted, got
an address over DHCP, and resolved names: its `systemd` is
`255.4-1ubuntu8.18`, which carries the root key fix. bombyx's guest
scripts ran on it: the egress rule loaded, the guest fetched GitHub's
published ssh host keys, and the project cloned over ssh with a deploy
key. cloud-init reported itself disabled. The SSH host key and
`/etc/machine-id` were written seconds after that VM's first boot,
which is after the build finished, so the VM made its own.

## Not checked

- Two VMs from one box side by side. That each gets its own host keys
  and machine id rests on the timestamps above, from one VM.
- A build on a VM host other than frosti.
- A host without HashiCorp's vagrant package, where `VAGRANT_PUB` is
  needed.
