# Ubuntu guest box

This project uses a box only when it comes from the OS vendor, or is
built from the vendor's own image. That is a rule for the operator, not
a check: bombyx boots whatever box the config names. Canonical
publishes no Vagrant box for libvirt, so the recipe in
`boxes/ubuntu-24.04/` builds one from Canonical's cloud image. This
file records what that box holds and how it is built.

A box also has to resolve names, and a stale box can fail at that. A
guest's resolver, systemd-resolved, checks each DNS answer against a
built-in copy of the DNS root zone's signing key, the KSK. The root
switched to a new KSK on 2026-10-11. A systemd built before then knows
only the old key and rejects every answer, so every name lookup in the
guest fails, and the guest cannot even reach GitHub to clone. Ubuntu
24.04 carries the new key from `systemd 255.4-1ubuntu8.18`. That is
why bombyx does not use `generic/ubuntu2204`: its publisher, a third
party, has shipped no version of it since early 2024. GitHub issues
#193 and #194 hold the investigation.

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
`qemu-img`, `xorriso`, `tar`, `timeout` and `flock` on the VM host,
and the operator in the `kvm` group. It also needs vagrant's insecure
public key: every vagrant install ships the same key pair, so that
vagrant can log in to a box it has never seen, and on a VM's first
`up` vagrant replaces it with a key made for that VM. The key is
public by design and is not a secret to protect. `build.sh` finds it
in HashiCorp's vagrant package, under `/opt/vagrant/embedded`; with
vagrant from another source, set `VAGRANT_PUB` to the key's path.

The build works in `~/.cache/bombyx-box`, or in the folder given as
its one argument. It keeps the image there and downloads it again only
when Canonical publishes a new one.

It runs unattended. A build on frosti, the maintainers' VM host, on
2026-10-20, took under two minutes: the 621 MB image downloaded in
about 15 seconds, the update boot took about a minute, and packing
took the rest. The box came out at 2.0 GB. An older image has more
updates to install, so expect a build to take longer as the image
ages.

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
  when gpg reports a good signature by the key with fingerprint
  `D2EB 4462 6FDD C30B 513D 5BB7 1A5D 6C4C 7DB8 7C81`, Canonical's
  "UEC Image Automatic Signing Key", and reports that key as neither
  revoked nor expired. It then checks the image's SHA-256 against that
  file. The fingerprint is pinned in `build.sh` and was confirmed from
  a second source: Ubuntu's own `ubuntu-keyring` package ships the same
  key, as `/usr/share/keyrings/ubuntu-cloudimage-keyring.gpg`.
- **What the keyserver is trusted for.** The key itself is fetched
  from `keyserver.ubuntu.com`. Only a signature by the pinned
  fingerprint counts, so the keyserver cannot substitute another key.
  It is trusted for one thing: if Canonical revokes the key, the build
  learns of it only when the keyserver serves the revocation.
- **Why 24.04 and not 26.04.** Both carry the new root key: 24.04 in
  `systemd 255.4-1ubuntu8.18`, 26.04 in `systemd 259`. 26.04 replaces
  `sudo` with `sudo-rs` and GNU coreutils with Rust's uutils by
  default. bombyx's guest scripts, `account.sh` and `bootstrap.sh` in
  `crates/bombyx/templates/`, which create the agent's account and
  clone the project, have never run against either. 24.04 keeps the
  GNU tools those scripts already run on.
- **Why the build installs updates.** The image is a snapshot. Without
  the update, every VM would start as far behind as the image is old.

## Cloud images and cloud-init

A cloud image is a ready-installed disk with no installer and no login
account. On its first boot a program inside it, cloud-init, configures
it from instructions supplied from outside, through what cloud-init
calls a data source. The NoCloud data source reads those instructions
from any volume labelled `CIDATA`, so a small ISO with that label is
enough to tell the image what to do. The recipe uses that once, during
the build, to run its own script inside the image; after that,
cloud-init has no more work and is switched off.

## How the build works

`build.sh` copies the image into a private build folder, checks that
copy against the signed checksum, grows its disk to 64 GB, and boots
it once under `qemu` with the `CIDATA` seed volume attached. The copy
that was checked is the one that boots, so a change to the cached
image during a build cannot reach the box. A lock in the work folder
lets only one Ubuntu build run there at a time.

The seed volume holds `user-data`, which tells cloud-init to run
`stage.sh` from the same volume and then power off, and vagrant's
public key. `stage.sh` runs as root inside that VM. It:

1. installs every update, keeping the image's own configuration files;
2. creates the `vagrant` account with the insecure key and
   passwordless `sudo`, which vagrant expects of a box;
3. replaces cloud-init's network file, which matches the build VM's
   MAC address, with one that asks for DHCP on any wired card, so a VM
   with another MAC still gets an address;
4. turns cloud-init off, so it does not search for a data source on
   every boot;
5. removes the SSH host keys and installs a unit that makes new ones on
   a VM's first boot, ordered before `ssh.service`, so no two VMs share
   keys;
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

On frosti, on 2026-10-20, the recipe as committed built a box whose
signature and checksum checked out and whose `stage.sh` reported
`BOMBYX-DONE`. VMs made from builds of this recipe were checked as
follows:

- each booted, got an address over DHCP, and resolved names, with
  `systemd 255.4-1ubuntu8.18`;
- bombyx's guest scripts ran: the egress rule loaded, the guest fetched
  GitHub's published ssh host keys, and the project cloned over ssh
  with a deploy key;
- cloud-init reported itself disabled;
- the SSH host key and `/etc/machine-id` were written seconds after the
  VM's first boot, after its build had finished, so the VM made its
  own;
- the host-key unit ran during boot and finished before `ssh.service`
  started, and the boot logged no ordering cycle.

## Not checked

- Two VMs from one box side by side. That each gets its own host keys
  and machine id rests on the timestamps above, from one VM at a time.
- A build on a VM host other than frosti.
- A host without HashiCorp's vagrant package, where `VAGRANT_PUB` is
  needed.
