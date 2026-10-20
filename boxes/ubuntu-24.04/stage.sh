#!/usr/bin/env bash
# Runs once, as root, inside the build VM that boxes/ubuntu-24.04/
# build.sh boots, started by cloud-init from the seed volume mounted at
# /mnt/seed. It updates Ubuntu, adds the vagrant account, and removes
# what would otherwise make every VM made from the box a copy of this
# one.
#
# The last line it writes to the serial console is BOMBYX-DONE, or
# BOMBYX-FAILED with the line that failed. build.sh reads that line,
# because qemu exits the same way whether this script succeeded or not:
# cloud-init powers the VM off either way.

set -euo pipefail

readonly CONSOLE=/dev/ttyS0
readonly SEED=/mnt/seed

trap 'echo "BOMBYX-FAILED: stage.sh line $LINENO" >"$CONSOLE"' ERR

# Every update in Ubuntu's archive. `--force-confold` keeps the
# image's configuration files where a package ships a new one, so the
# run never stops to ask.
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get -y -o Dpkg::Options::=--force-confold full-upgrade
apt-get -y autoremove --purge

# The account vagrant logs in with: its insecure key, and passwordless
# sudo, which is what vagrant expects of a box. vagrant replaces the key
# with one of its own on the first `up` of each VM. The password stays
# locked, so the key is the only way in.
useradd --create-home --shell /bin/bash vagrant
install -d -m 0700 -o vagrant -g vagrant /home/vagrant/.ssh
install -m 0600 -o vagrant -g vagrant "$SEED/vagrant.pub" \
    /home/vagrant/.ssh/authorized_keys
printf 'vagrant ALL=(ALL) NOPASSWD:ALL\n' >/etc/sudoers.d/vagrant.tmp
visudo -cqf /etc/sudoers.d/vagrant.tmp
install -m 0440 /etc/sudoers.d/vagrant.tmp /etc/sudoers.d/vagrant
rm /etc/sudoers.d/vagrant.tmp

# NETWORK. cloud-init wrote a netplan file that matches this VM's
# network card by its MAC address. A VM made from the box has another
# MAC, so that file would leave it with no network. This one matches
# any wired card by name and asks for an address over DHCP, which is
# what vagrant-libvirt's network hands out.
rm -f /etc/netplan/50-cloud-init.yaml
cat >/etc/netplan/01-bombyx.yaml <<'EOF'
network:
  version: 2
  ethernets:
    wired:
      match:
        name: "en*"
      dhcp4: true
EOF
chmod 0600 /etc/netplan/01-bombyx.yaml

# cloud-init is done once the box is built. Left on, it would look for
# a data source on every boot of every VM made from the box, and could
# write a network config of its own again.
touch /etc/cloud/cloud-init.disabled
cloud-init clean --logs

# SSH HOST KEYS. Every VM made from the box would otherwise share this
# build's keys. They are removed here, and a unit of bombyx's makes new
# ones on a boot that finds none, before sshd starts. Ubuntu's own
# openssh package makes none at boot; cloud-init did that, and it is now
# off.
rm -f /etc/ssh/ssh_host_*
cat >/etc/systemd/system/bombyx-ssh-hostkeys.service <<'EOF'
[Unit]
Description=Make SSH host keys on a VM's first boot
ConditionPathExists=!/etc/ssh/ssh_host_ed25519_key
Before=ssh.service ssh.socket

[Service]
Type=oneshot
ExecStart=/usr/bin/ssh-keygen -A

[Install]
WantedBy=multi-user.target ssh.service ssh.socket
EOF
systemctl enable bombyx-ssh-hostkeys.service

# MACHINE ID. systemd derives the DHCP client identifier from it, so two
# VMs sharing one would ask the DHCP server for the same lease. An empty
# file makes systemd write a new one on each VM's first boot.
truncate -s 0 /etc/machine-id
rm -f /var/lib/dbus/machine-id

# Leave less to pack: the downloaded packages, the logs of this run,
# and the free blocks, which `fstrim` hands back to the qcow2 disk so
# build.sh's `qemu-img convert` can drop them. The package lists stay,
# so a project's `apt-get install` finds packages without an update
# first.
apt-get clean
journalctl --rotate --vacuum-time=1s >/dev/null 2>&1 || true
fstrim -av >/dev/null
sync

echo BOMBYX-DONE >"$CONSOLE"
