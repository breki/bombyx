# Setting up a VM host

This describes how to prepare a machine so that bombyx can
run agent VMs on it. The commands are written for Ubuntu
24.04 LTS. If you are on a different distribution, read the
whole page anyway and then see **Other distributions** at the
end, which explains what changes.

> **This is a written record, not a script, and that is
> deliberate.**
>
> Package names and repositories change between releases.
> Ubuntu 24.04, for example, removed the `vagrant` package
> completely and renamed `qemu-kvm`. When a document goes out
> of date you can see that it has, and you adapt. When a
> script goes out of date it fails half-way through, as root,
> having already changed some things and not others.
>
> Setting up the host is also not bombyx's job. bombyx
> composes `ssh`, `scp`, `tar` and `vagrant`, and nothing
> more. See "wrap, don't reimplement" in `README.md`.
>
> Steps 1 and 2 were verified on Ubuntu 24.04.4 with Vagrant
> 2.4.9 in August 2026. Steps marked *(unverified)* have not
> yet been run from start to finish.

## What has to be true

There are two separate sets of requirements. It is worth
keeping them apart, because they fail in different ways and
you fix them in different places.

### What bombyx itself needs

This list is short and it rarely changes. It is also exactly
what `bombyx doctor` will check, once that exists.

| Where | Requirement |
|-------|-------------|
| Host | `sshd` running, key auth working *non-interactively* |
| Host | A POSIX login shell (`bash` or `sh`, not `csh` or `fish`) |
| Host | `tar` available on `PATH` |
| Host | `vagrant` available on the **non-interactive** `PATH` |
| Host | `remote_root` (default `~/vms`) writable |
| Local | `ssh`, `scp` and `tar` |

### What the host needs in order to run VMs at all

This list is longer, and it changes more often between
distributions and releases. None of it is bombyx's concern.
If something here is wrong, `vagrant` is the thing that will
tell you so.

- Hardware virtualization support in the CPU, and a
  `/dev/kvm` device.
- QEMU and libvirt installed, with your user able to reach
  libvirt without `sudo`.
- Vagrant's `libvirt` provider plugin.
- Enough RAM and disk space for the VMs you plan to boot.

As a rough guide, a machine with 8 cores, 24 GiB of RAM and
about 300 GB of free disk comfortably hosts several agent VMs
at once.

## Check the machine before you install anything

All of these commands only read information. None of them
change the system.

```bash
grep -oE 'vmx|svm' /proc/cpuinfo | sort -u   # no output = VT off in BIOS
systemd-detect-virt                          # "none" = bare metal
ls -l /dev/kvm                               # missing = module not loaded
free -g && df -h /
```

`vmx` means an Intel CPU with VT-x, and `svm` means an AMD CPU
with AMD-V. If neither appears, hardware virtualization is
almost certainly switched off in the BIOS.

You can run VMs on a host that is itself a VM, but it will be
noticeably slower. A physical machine is the better choice for
a host that boots VMs on demand.

## Installing on Ubuntu 24.04

### Step 1: QEMU and libvirt

Note that the package is `qemu-system-x86`. The older name,
`qemu-kvm`, no longer exists in 24.04.

```bash
sudo apt update
sudo apt install -y qemu-system-x86 libvirt-daemon-system \
                    libvirt-clients virtinst
sudo usermod -aG libvirt,kvm "$USER"
```

That last command adds your user to two groups. Membership of
`libvirt` lets you create and control VMs without `sudo`.
Membership of `kvm` gives you access to `/dev/kvm`, which is
what makes the VMs run at close to full speed instead of
being emulated in software.

**The `-a` matters, and leaving it out is genuinely
dangerous.** With `-a`, the command adds these two groups to
the ones you are already in. Without it, `-G` *replaces* your
entire list of extra groups, so you would lose `sudo` and
anything else you had. On a machine you only reach over SSH,
that can lock you out of administering it.

Group membership is only read when a session starts, so log
out and log back in before you expect it to work. After that,
bombyx opens a new connection for every command, so it will
pick up the new groups on its own.

Two details that save time when something does not work:

- Ubuntu's `libvirt-daemon-system` package may have already
  added you to the `libvirt` group during installation. Run
  `id -nG` to see. Do not assume the `usermod` above is what
  put you there.
- Of the two groups, `libvirt` is the one that actually
  matters here. With the default `qemu:///system` connection,
  libvirtd starts QEMU as root, so your own access to
  `/dev/kvm` is not needed. Being in `kvm` is a sensible
  precaution rather than a requirement, and it only becomes
  necessary if you use `qemu:///session` or run QEMU
  directly.

### Step 2: Vagrant

**Ubuntu 24.04 does not package Vagrant at all.** Running
`apt install vagrant` reports that there is no installation
candidate, even with the `universe` component enabled. You
therefore need HashiCorp's own repository.

```bash
wget -O- https://apt.releases.hashicorp.com/gpg | \
  sudo gpg --dearmor -o /usr/share/keyrings/hashicorp-archive-keyring.gpg
echo "deb [arch=amd64 signed-by=/usr/share/keyrings/hashicorp-archive-keyring.gpg] \
https://apt.releases.hashicorp.com noble main" | \
  sudo tee /etc/apt/sources.list.d/hashicorp.list
sudo apt update && sudo apt install -y vagrant
```

This installs Vagrant under `/opt/vagrant` and puts a small
wrapper script at `/usr/bin/vagrant`. It is a shell script
rather than a symlink, but what matters is only that
`/usr/bin` is on the default `PATH`, which is why bombyx can
find it at all. The section on the non-interactive `PATH`
below explains why that matters so much.

### Step 3: The libvirt provider plugin

```bash
sudo apt install -y libvirt-dev ruby-dev gcc make pkg-config
vagrant plugin install vagrant-libvirt
```

Only the build dependencies need `sudo`. Vagrant keeps plugins
per user under `~/.vagrant.d`, so the second command is run as
yourself.

This plugin compiles native extensions against the Ruby that
ships inside Vagrant, which is the part most likely to fail.
On Ubuntu 24.04 with Vagrant 2.4.9 it built and linked without
any help, installing `vagrant-libvirt 0.12.2` in about a
minute. If the build cannot link on your machine, point it at
Vagrant's own libraries and try again:

```bash
CONFIGURE_ARGS='with-ldflags=-L/opt/vagrant/embedded/lib' \
  vagrant plugin install vagrant-libvirt
```

### The storage pool creates itself, but does not autostart

A fresh libvirt install has no storage pools at all, which
looks alarming if you check before booting anything:

```bash
virsh -c qemu:///system pool-list --all    # empty on a new host
```

You do not need to create one. vagrant-libvirt defines and
starts a pool named `default` the first time it boots a VM,
putting disk images in `/var/lib/libvirt/images`.

It does leave that pool with **autostart disabled**, which is
worth knowing because the consequence is delayed. Check with:

```bash
virsh -c qemu:///system pool-list --all
# Name      State    Autostart
# default   active   no          <-- will not come back after a reboot
```

After the host reboots, an inactive pool makes `vagrant up`
fail in a way that has nothing obviously to do with rebooting.
Turn it on once:

```bash
virsh -c qemu:///system pool-autostart default
```

## Checking that it worked

Run these from your workstation rather than on the host
itself. That is the point of them: they test the same path
that bombyx uses, instead of the more forgiving one you get
when you log in and type commands by hand.

```bash
ssh <host> true                 # the alias and key auth work
ssh <host> vagrant --version    # the important one
ssh <host> 'command -v tar'
ssh <host> 'vagrant plugin list'                    # see below
ssh <host> 'virsh -c qemu:///system net-list --all' # see below
```

If `vagrant --version` prints a version number over a plain,
non-interactive `ssh` call like that, bombyx has everything it
needs.

Two of those commands need care, because the obvious way to
write them tests the wrong thing.

**Always name the libvirt URI.** A bare `virsh list --all` run
as a non-root user connects to `qemu:///session`, a per-user
libvirt instance that is always reachable. It therefore passes
whether or not you have any access to the system daemon, which
is the one vagrant-libvirt actually uses. Check `virsh uri` if
you want to see which one you are getting. Passing
`-c qemu:///system` explicitly is what proves your `libvirt`
group membership works.

**Vagrant plugins are per-user.** Installing the provider with
`sudo vagrant plugin install` puts it in `/root/.vagrant.d`,
where the account bombyx connects as cannot see it.
`vagrant plugin list` then reports "No plugins installed" even
though the install appeared to succeed. Only the `apt install`
of the build dependencies needs `sudo`; the
`vagrant plugin install` must not have it.

## Why the non-interactive PATH causes trouble

This is the failure worth understanding properly, because
nothing in the system will report it for you.

bombyx runs commands of the form
`ssh <host> "cd ... && vagrant ..."`. A command passed to
`ssh` like this runs in a shell that is neither interactive
nor a login shell. Such a shell does not read `~/.profile`,
and it does not read the interactive part of `~/.bashrc`
either. Instead, Ubuntu gives it a fixed default `PATH`:

```
/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/usr/games:/usr/local/games:/snap/bin
```

A Vagrant installed in `/usr/bin`, which is what HashiCorp's
`.deb` gives you, is therefore visible. So is one installed
via snap, in `/snap/bin`. But a Vagrant in `~/.local/bin`, in
`~/bin`, or only in `/opt/vagrant/bin` is invisible.

The symptom is confusing, which is why it is worth knowing in
advance. Vagrant works perfectly when you SSH in and type
`vagrant` yourself, because that is an interactive login shell
with a fuller `PATH`. bombyx, using the same account on the
same machine, gets `bash: vagrant: command not found`. Worse,
it gets it in the middle of a push, after it has already
created the remote directory and copied a tarball across.

You can see the difference directly:

```bash
ssh <host> 'echo $PATH'                # what bombyx sees
ssh <host> 'bash -lc "echo \$PATH"'    # what you see on login
```

Vagrant cannot warn you about this itself, for the simple
reason that it is not running. To fix it, either install
Vagrant somewhere already on that default `PATH`, or add a
symlink into `/usr/local/bin`.

## Configuration for each project

Setting up the host is something you do once per machine.
After that, every project you want a VM for needs two things
of its own, kept in that project's own repository: a
`bombyx.toml` file and a `vagrant/` directory containing a
Vagrantfile.

bombyx does not ship either of them, because the project
repository is meant to be the source of truth for how its VM
is built. See `bombyx.toml.sample`, and the **Configure**
section of `README.md`.

## Other distributions

The two requirement lists near the top of this page are the
same everywhere. Only the package names and the installation
commands change.

On Fedora, the equivalent packages are `qemu-kvm`, `libvirt`
and `virt-install`, and Vagrant comes from HashiCorp's `dnf`
repository. On Arch, they are `qemu-full`, `libvirt` and
`vagrant`, and Vagrant *is* available from the official
repositories, so no extra repository is needed.

Whatever the distribution, two things are worth checking
carefully, because they are where the surprises tend to be:

1. Whether Vagrant is packaged at all, and if so, where its
   binary ends up. What matters is whether that location is
   on the non-interactive `PATH` described above.
2. Which group grants access to libvirt without `sudo`. It is
   called `libvirt` on some distributions, `libvirtd` or
   `kvm` on others.
