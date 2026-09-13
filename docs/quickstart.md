# bombyx quickstart

The short path: install bombyx, register a project, boot its VM,
work in it. [tutorial.md](tutorial.md) is the long version, which
builds a sample project from nothing and explains why each piece
is shaped the way it is. This page assumes you have a repository
already and want a VM for it.

You need a VM host with libvirt, Vagrant and the `vagrant-libvirt`
provider. That can be another machine or the one you are sitting
at. [vm-host-setup.md](vm-host-setup.md) covers preparing one.

## Install

No Rust toolchain needed -- the releases carry prebuilt binaries.

<!-- version: 0.5.0 -->

```bash
VERSION=0.5.0
BASE=https://github.com/breki/bombyx/releases/download/v$VERSION

cd /tmp
curl -LO "$BASE/bombyx-v$VERSION-x86_64-unknown-linux-gnu.tar.gz"
curl -LO "$BASE/SHA256SUMS"
```

Verify before unpacking:

```bash
sha256sum --check --ignore-missing SHA256SUMS
```

One `OK` line is what you want. `--ignore-missing` is needed
because `SHA256SUMS` covers every platform and you downloaded
one of them.

The archive extracts to a directory, not a bare binary:

```bash
tar xzf "bombyx-v$VERSION-x86_64-unknown-linux-gnu.tar.gz"
install -m755 \
    "bombyx-v$VERSION-x86_64-unknown-linux-gnu/bombyx" \
    ~/.local/bin/bombyx
bombyx --version
```

The directory also holds `LICENSE`, `README.md` and
`THIRD-PARTY-LICENSES`.

Other platforms: swap the target in the file name for
`x86_64-apple-darwin`, `aarch64-apple-darwin` or
`x86_64-pc-windows-msvc`. Windows also has a `.zip`.

**If the version is wrong**, something earlier on your `PATH` is
winning. A previous `cargo install` puts one in `~/.cargo/bin`;
`cargo uninstall bombyx` removes it.

After this first install, `bombyx self-update` does the same
download-and-verify for you.

## Register a project

bombyx reads one file, and it is yours -- not part of any
repository. Put it at `~/.config/bombyx/config.toml`
(`%APPDATA%\bombyx\config.toml` on Windows):

```toml
host = "vmhost"

[projects.myproject]
remote_root = "~/vms"

[projects.myproject.vm]
provider = "libvirt"
box = "generic/ubuntu2204"
cpus = 3
memory = 6144

[projects.myproject.source]
repo = "git@github.com:you/myproject.git"
ref = "main"
script = ".bombyx/provision.sh"
```

`host` is the machine the VMs run on, usually an alias from your
`~/.ssh/config`. Name the machine you are sitting at and bombyx
runs `vagrant` directly instead of over `ssh`.

The table key is the project name. Nothing inside repeats it,
and it is what you pass as `--project`.

**`remote_root` must sit above the two sub-tables.** TOML binds
a bare key to the header above it, so written below
`[...source]` it becomes `source.remote_root` and the whole file
is refused.

Sizing: the VM competes with whatever else runs on the host, and
that may be the machine you are typing on. Start small.

Changing these later is cheap. The libvirt provider compares the
configured values against the running machine when it starts one
and updates them, so `down` then `up` is enough -- the disk and
everything you installed survive. It has to be stopped and
started, though: `up` on a machine that is already running does
nothing.

Check the file parses:

```bash
bombyx list --offline
```

```
NAME       HOST    BOX                 CPUS   MEM
myproject  vmhost  generic/ubuntu2204     3  6144
```

`--offline` contacts no machine. An error here names the line to
fix.
