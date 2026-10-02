# Incidents

This file lists real security incidents in AI-agent isolation and
in agents running with broad access. Each entry says how the
attack worked, what bombyx does about it, and where bombyx is
exposed to the same mechanism. Other docs link to an entry here
rather than retelling it.

## How an entry is written

Each entry has four fields:

- **What happened**: the mechanism, in two or three sentences.
- **Source**: a link and its date.
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
VM with the project shared in. It kept a live host-guest file
share (virtio-fs) and a guest-to-host socket relay. Code inside the
sandbox escaped both by planting symlinks, which gave it read and
write on host files as the hypervisor user.

**Source.** CVE-2026-77179 and CVE-2026-79994, September 2026.

**What bombyx does about it.** bombyx runs no host-guest share.
The guest clones the repository itself, and the generated
Vagrantfile disables Vagrant's default shared folder
(`crates/bombyx/src/vagrantfile.rs`, the `synced_folder` line).
`docs/trust-boundary.md` under "Two ways to satisfy the
constraint" explains why nothing outside the guest holds project
code.

**Where bombyx is exposed.** The mechanism is the host opening a
path or file whose content the guest controls. Without a share,
two routes remain:

- Provisioning output crosses from the guest to the workstation's
  terminal. `doctor` sanitizes what the VM host prints and `up`
  does not; issue #80 tracks that.
- Anything else the VM host reads that the guest can influence.
  Not checked; `host-reads-guest-paths` in `docs/todo.md` tracks
  it.

## Meta Muse zero-day, September 2026

**What happened.** Muse is a desktop agent for macOS that users
grant broad access: files, mail, calendar and the microphone. An
undocumented setting, `endo_voyager_dictation_endpoint`, named the
server that receives dictation audio, and any unprivileged local
process could rewrite it. An attacker could point it at a proxy
that kept the audio and auth tokens and passed traffic on to Meta.
Prompt injection could then make the trusted agent copy out files.
The attacker steers the agent instead of shipping malware that
needs its own access.

**Source.**
<https://www.infoq.com/news/2026/09/meta-muse-zeroday/>,
September 2026.

**What bombyx does about it.** The agent runs in a VM, so steering
it reaches only what the guest holds, and nothing on the
workstation. The README's "Why" section makes that case.

**Where bombyx is exposed.** The guest is the agent's own machine,
so an injected agent can change any setting inside it:

- bombyx writes no API-endpoint or proxy setting into the guest.
  It writes the git remote and the credential helper into the
  clone's config, which the agent's account owns, so the agent can
  point the push credential at any host. That adds nothing beyond
  reading the credential directly, which the agent can always do
  (`docs/trust-boundary.md`, "The agent can read the credential").
  Issue #150 would keep the credential out of the guest.
- The guest keeps outbound internet access, so it can send what it
  holds anywhere. Issue #130 would enforce an egress allowlist at
  the router.
