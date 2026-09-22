# Running bombyx against your own machine

Your workstation can serve as its own VM host. On Linux, bombyx
runs `vagrant` here through `sh -c` rather than reaching a second
machine over `ssh`. This page is the whole procedure and its
caveats; it is the local-route alternative to Parts 1 and 2 of
[the tutorial](tutorial.md), which set up a separate VM host.

*Verified on a Linux workstation (Ubuntu, vagrant 2.4.9,
vagrant-libvirt 0.12.2) on 2026-09-05: every VM command ran
through `sh -c`, the two generated files arrived intact, and a
guest booted on this route and provisioned to completion.*

You should read this page before you begin Part 2 of
[the tutorial](tutorial.md), or else come back and redo Part 2
afterwards, because it replaces the SSH alias you wrote in Part 1
together with the `host` line that names it. The checks that
accompany them -- `ssh vmhost true` in Part 1, and `ssh vmhost
vagrant --version` in Part 2 -- do not apply to you either, since
bombyx will not be using `ssh` at all.

The one prerequisite that decides whether you can use this route
is a Linux workstation. bombyx's VM host must run libvirt, and
libvirt runs on neither Windows nor macOS. If your workstation is
Windows, your options are a Linux VM or a WSL2 distribution with
nested virtualization acting as the host -- see
[vm-host-wsl2.md](vm-host-wsl2.md), which is verified end to end --
and in either case bombyx reaches it over `ssh`. It refuses the
local route on Windows outright, so there is nothing there to
configure wrongly.

With that settled, the procedure is simple: write your own
machine's name as `host` and bombyx does the rest. As it reads
`config.toml` it compares `host` against this machine's name, and
when the two match it runs each command here through `sh -c`
instead of handing it to `ssh`. There is no SSH server to install,
no key to authorize to your own account, and no loopback alias to
write.

```toml
host = "nimbus"     # this machine, so no ssh hop
```

Write the name exactly. The comparison ignores case, and nothing
else, so `host` must be what your machine calls itself, character
for character. Run `hostname` and copy what it prints. A domain
counts: on a machine answering `nimbus.lan`, `host = "nimbus"`
gives you the SSH route instead.

That strictness is deliberate. A bare label is easy to share --
plenty of machines are called `ubuntu`, `vagrant` or `build01` --
and it is the domain that says which one you mean. Matching on the
label alone would risk two unpleasant outcomes: bombyx starting a
guest on your workstation while you believe it is on the isolated
host, and teardown later deleting the workstation's directory.
Getting it wrong in the other direction merely gives you the SSH
route, which you will notice at once.

Note, too, that bombyx never reads your `~/.ssh/config`. It checks
the name you wrote against the machine's own name, and no more.
Usually this is exactly what you want: write `host = "selfhost"`
with `selfhost` aliased to `127.0.0.1` and you get the SSH route,
because you asked for it by name. The exception is worth keeping
in mind -- if an SSH alias happens to have exactly your machine's
name but points elsewhere, bombyx matches on the name and takes
the local route regardless. Write that alias as `you@name` and the
SSH route is forced, since the `you@` makes the two names differ.

On Windows the local route is never taken, whatever the names may
say. A Windows machine cannot run libvirt, so the local route
there could only ever be a mistake -- and a quiet one at that,
since Git for Windows supplies an `sh` for it to run.

You can always tell which route is in force. bombyx prints a line
on stderr whenever it is running here, and `bombyx doctor` reads
differently in two respects. In the first row, bombyx prints `sh`
rather than `ssh`, because that is the program it will actually
start; and two host rows come back as skips rather than passes --
`ssh`, which is not used, and `login shell`, because bombyx starts
`sh` itself rather than asking your login shell to interpret
anything. (The `doctor` transcript in [the tutorial](tutorial.md),
under **When something goes wrong**, is an `ssh`-route run, and so
shows neither.) That notice is worth reading rather than tuning
out: **Before you start** in [the tutorial](tutorial.md) describes
what you give up by placing the guest on the same machine you work
on, and the local route is what makes that arrangement easy to
reach by accident.

Everything else about bombyx stays the same. It still writes the
generated files and still runs `vagrant`, and the script it builds
is identical on both routes, since `sh -c` is the very same POSIX
shell that `ssh` would have started on a remote host.

On the `ssh` route the host's login shell must be POSIX, because
bombyx sends `mkdir -p` and `cat > file` for the far side to
interpret. On Linux this is already the case. On Windows, OpenSSH
Server starts `cmd.exe`, those commands fail, and the fix is the
`DefaultShell` registry value; this is what `doctor`'s `login
shell` row checks. The local route asks nothing of your login
shell, since bombyx starts `sh` itself.

There is one more thing to say about Windows, now that the
paragraph above has sent you there. Hyper-V is the other way to
run VMs on Windows, and bombyx accepts it as a `provider` value --
`libvirt` and `hyperv` are the two it takes, and VirtualBox is not
among them. Hyper-V does not, however, give you the local route,
and it carries a caveat of its own: its provider needs an elevated
shell, which an SSH session does not have. Because bombyx passes
the provider straight through to vagrant, setting `hyperv` where
it is unavailable fails the boot rather than quietly falling back
to libvirt. This protection applies only before the VM exists.
Once vagrant has created a machine it records the provider and
reads that record back later, so switching providers afterwards
requires a `bombyx destroy` first. `bombyx destroy` passes no
provider at all, and can therefore remove the directory even after
a boot has failed on a provider mismatch. That refusal was tested
on a Linux host and works there; whether a Windows VM host then
boots the machine is *(unverified)*, since nobody has yet run
bombyx against one.
