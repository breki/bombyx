# Incidents

This file lists real security incidents that bear on running
untrusted code, an AI agent in particular, away from the machines
it could harm. Each entry says how the attack worked, what bombyx
does about it, and where bombyx is exposed to the same mechanism.
Other docs link to an entry here rather than retelling it.

## How an entry is written

Each entry has four fields:

- **What happened**: the mechanism, in one short paragraph.
- **Source**: a link and its date. For a CVE, link its record on
  cve.org.
- **What bombyx does about it**: the boundary that contains the
  attack, or the open issue or todo that would. If the answer is
  nothing, the entry says so.
- **Where bombyx is exposed**: the part of bombyx the same
  mechanism would hit, or why there is none. This field asks how
  the attack transfers to bombyx's design, not only whether bombyx
  blocks the incident as reported. If nobody has checked, the
  entry says "not checked".

Every exposure is tracked. If an entry names an exposure that no
issue or todo covers, we file one as we write the entry, and the
entry links to it. So a gap never lives only in this file.

## Docker Sandboxes escapes, September 2026

**What happened.** Docker Sandboxes runs a coding agent in a small
VM with the project shared in. It kept two live channels to the
host: a file share (virtio-fs, which lets the guest read and write
a host directory) and a socket relay, which forwards the guest's
connections to sockets on the host. Code inside the sandbox
escaped both by planting symlinks that the host side followed.
That gave it read and write on host files, as the host account
that runs the sandbox's VM.

**Source.**
[CVE-2026-77179](https://www.cve.org/CVERecord?id=CVE-2026-77179)
and
[CVE-2026-79994](https://www.cve.org/CVERecord?id=CVE-2026-79994),
September 2026.

**What bombyx does about it.** bombyx runs no host-guest share.
The guest clones the repository itself, and the generated
Vagrantfile disables Vagrant's default shared folder
(`crates/bombyx/src/vagrantfile.rs`, the `synced_folder` line).
`docs/trust-boundary.md` under "Two ways to satisfy the
constraint" explains why nothing outside the guest holds project
code.

**Where bombyx is exposed.** The general mechanism is the host
acting on content the guest controls. Without a share, two routes
remain:

- The provisioner's output crosses from the guest to the
  operator's terminal. Control characters in that output can
  repaint lines the operator has already read. `up` prints the
  `secrets_refreshed` hook's output with control characters shown
  as `?` (`docs/trust-boundary.md`, "The secrets hook runs branch
  code on every refresh"), but it prints the provisioner's output
  unfiltered. Issue #80 tracks that.
- Anything else the VM host reads that the guest can influence.
  Not checked; `host-reads-guest-paths` in `docs/todo.md` tracks
  it.

## Meta Muse zero-day, September 2026

**What happened.** Muse is a desktop agent for macOS, and users
grant it broad access: files, mail, calendar and the microphone.
An undocumented setting, `endo_voyager_dictation_endpoint`, named
the server that receives dictation audio, and any unprivileged
local process could rewrite it. An attacker could point it at a
proxy that kept the audio and auth tokens and passed the traffic
on to Meta. The article also reports a second attack: prompt
injection that makes the trusted agent copy out documents.

**Source.**
<https://www.infoq.com/news/2026/09/meta-muse-zeroday/>,
September 2026.

**What bombyx does about it.** The agent runs in a VM, so an
attacker who steers it reaches only what the guest holds, and
nothing on the workstation. The README's "Why" section makes that
case.

**Where bombyx is exposed.** The guest is the agent's own machine,
so an injected agent can change any setting inside it:

- bombyx writes no API-endpoint or proxy setting into the guest.
  It writes the git remote and a credential helper into the
  clone's config, and the agent's account owns that config. The
  helper is `git-credential-store`, which hands the token only to
  the git host it was stored for. So a rewritten remote does not
  send the token elsewhere. The agent can read the token file
  directly, though (`docs/trust-boundary.md`, "The agent can read
  the credential"). Issue #150 would keep the credential out of
  the guest.
- The guest keeps outbound internet access, so it can send what it
  holds anywhere. The VM host's firewall (`host-network-isolation`
  in `docs/todo.md`) blocks the router and the VM host itself, not
  the internet. Issue #130 would put agent VMs on a VLAN of their
  own, with an egress allowlist at the router.

## Ubuntu AF_UNIX kernel escape, September 2026

**What happened.** The kernel's garbage collector for AF_UNIX
sockets races with the code that queues new references to a
socket, so it can free memory that something still points to.
Unprivileged code can use that freed memory to become root.
Docker's and Kubernetes' default seccomp profiles allow AF_UNIX
sockets, so code in a container can use it to become root on the
host, because a container shares the host's kernel. The upstream
fix is in kernels 7.1.10 and 7.2. Ubuntu's tracker lists its 24.04
and 26.04 kernels as vulnerable with no fix shipped, and a public
exploit was released on 22 and 23 September 2026.

**Source.**
[CVE-2026-80521](https://www.cve.org/CVERecord?id=CVE-2026-80521),
<https://thehackernews.com/2026/09/exploit-released-for-unpatched-ubuntu.html>
and <https://ubuntu.com/security/CVE-2026-80521>, September 2026.

**What bombyx does about it.** A bombyx guest is a VM with its own
kernel, so the exploit makes the agent root only inside its own
guest. The agent already has that through passwordless `sudo`, so
the exploit gives it nothing new.

**Where bombyx is exposed.** The exploit becomes the second step
after a hypervisor escape, which `docs/trust-boundary.md` accepts
as reaching the VM host. QEMU runs as the unprivileged
`libvirt-qemu` user (measured on a WSL2 VM host and on a local
host), so an escape alone lands as that user, and this bug turns
it into root on the VM host. When the VM host is the workstation,
that root also reaches every secret the workstation holds. Only a
kernel update closes this, and nothing in bombyx reports the VM
host's kernel or a pending reboot; `doctor-host-kernel-state` in
`docs/todo.md` tracks that.
