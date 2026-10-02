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

## KVM shadow-MMU escapes: Januscape and Zapscape, July and August 2026

**What happened.** Two use-after-free bugs in KVM's shadow MMU,
the code that tracks a nested guest's page tables, let code
inside a guest corrupt host kernel memory. Januscape matched a
reused shadow page by its address and ignored the page's type;
Zapscape let page reclaim free a root page the fault handler was
still using. Both need root inside the guest and a guest that
runs a VM of its own (nested virtualization). Zapscape on Intel
also needs 5-level EPT exposed to the guest. A public proof of
concept for each crashes or takes over the host.

**Source.**
[CVE-2026-53359](https://www.cve.org/CVERecord?id=CVE-2026-53359),
<https://thehackernews.com/2026/07/16-year-old-linux-kvm-flaw-lets-guest.html>,
July 2026;
[CVE-2026-64561](https://www.cve.org/CVERecord?id=CVE-2026-64561),
<https://thehackernews.com/2026/08/new-zapscape-kvm-flaw-could-let.html>,
August 2026.

**What bombyx does about it.** Nothing yet. A kernel update closes
each bug: Januscape is fixed in Ubuntu 24.04, and Zapscape is not
as of 2026-10-02.

**Where bombyx is exposed.** The agent has root in the guest
through passwordless `sudo`, and bombyx's default CPU mode,
`host-passthrough`, passes the host CPU's virtualization flag to
the guest whenever the host's KVM allows nesting. So a default
bombyx guest probably meets both conditions; we have not
confirmed the flag from inside a guest. Issue #170 tracks hiding
that flag by default.

## Comment and Control: prompt injection through GitHub, April 2026

**What happened.** A PR title, an issue body or a hidden HTML
comment carried instructions that hijacked coding agents working
in CI: Claude Code, Gemini CLI, Copilot's agent and Codex. The
agents read secrets from their environment and sent them out
through channels they were allowed to use: PR and issue comments,
commits, `git push`, and in one case Hugging Face download
counters on a pre-approved domain, one character at a time.

**Source.**
[CVE-2026-54316](https://www.cve.org/CVERecord?id=CVE-2026-54316)
(Claude Code) and
[CVE-2026-12537](https://www.cve.org/CVERecord?id=CVE-2026-12537)
(Gemini CLI),
<https://labs.cloudsecurityalliance.org/research/csa-research-note-ai-coding-agent-ci-prompt-injection-202608/>,
first disclosed April 2026.

**What bombyx does about it.** Nothing it can do. The VM keeps the
workstation's secrets out of reach, but the agent in the guest
reads issues and repository text, and it holds a git credential
and the secrets file by design.

**Where bombyx is exposed.** Everything the guest holds, sent out
through a channel the guest must keep. `git push` is how work
leaves the VM, so no firewall can close it, and an egress
allowlist such as the one #130 proposes still leaks through each
domain it allows. The token's scope sets the damage
(`docs/trust-boundary.md`, "The guest needs a credential"), and
#150 would keep the credential itself out of the guest.

## Mini Shai-Hulud worms, April to August 2026

**What happened.** A family of npm and PyPI worms spread through
hijacked maintainer accounts and CI pipelines. A poisoned version
runs a `preinstall` hook, or runs on import, and collects SSH
keys, git and cloud tokens, `*.kdbx` password databases and AI
tool configuration. It republishes every package the stolen token
can publish. For persistence it writes `.claude/settings.json`
hooks and `.vscode/tasks.json` folder-open tasks into the project,
where they survive uninstalling the package. One wave, through
TanStack, led to about 170 private repositories being copied from
a single infected laptop.

**Source.**
[CVE-2026-45321](https://www.cve.org/CVERecord?id=CVE-2026-45321),
<https://snyk.io/blog/tanstack-npm-packages-compromised/>,
<https://www.aikido.dev/blog/keyv-and-friends-compromised-in-npm-supply-chain-attack>
and
<https://thehackernews.com/2026/09/crowdsec-says-tanstack-npm-attack-led.html>,
April to September 2026.

**What bombyx does about it.** This is the attack the README's
"Why" section describes. An install run by the agent happens in
the guest, so the worm finds the guest's credentials and none of
the workstation's.

**Where bombyx is exposed.** Two routes:

- The guest's git credential and secrets file, which the worm
  reads like any other file in the guest. #150 tracks keeping the
  credential out of the guest.
- The persistence files. They land in the working tree, so an
  agent's `git push` can carry them to the workstation, where
  Claude Code or VS Code runs them when the checkout is opened.
  Issue #171 tracks that route.
