# Red Team Findings -- Deferred backlog

Security (Red Team) review findings. Newest first.


---

### rt-2026-09-02-documented-config-is-rejected

**Category:** Documentation that does not work

`README.md` and `docs/tutorial.md` both show a sample `bombyx.toml`
with `remote_root` written *after* the `[source]` table. TOML binds a
bare key to the table above it, so it parses as `source.remote_root`
and the config is refused. Reproduced by copying the tutorial's block
verbatim: "unknown field `remote_root`, expected one of `repo`,
`ref`, `script`". `bombyx.toml.sample` is broken differently -- it
still carries `vagrant_dir` and has no `[vm]` or `[source]` at all,
so copying it fails to load too.

The fix is to move `remote_root` above `[vm]` in both samples and
rewrite the sample file. `CLAUDE.md` asks for a test using the
document's own example, which would stop this recurring.

### rt-2026-09-02-doctor-fails-on-hyperv-projects

**Category:** A gating check that gates the wrong thing

`doctor::host_probes` always adds the `libvirt provider` probe,
whatever `[vm] provider` says. On a `provider = "hyperv"` project
against a host without `vagrant-libvirt`, `doctor` prints a FAIL row
and exits 1 while every VM command works.

This is the same class as the `tar` row removed in 92c2e74, and the
sentence added in 43e29ce -- "a red report always means `up` is in
trouble" -- is false while this probe is unconditional.

### rt-2026-09-02-home-does-not-isolate-ssh-config

**Category:** A comment asserting a property the platform does not give

`doctor_fails_and_says_which_check_failed` sets `HOME` and
`USERPROFILE` to the fixture and claims that stops `ssh` reading the
operator's `~/.ssh/config`. OpenSSH on Unix takes the home directory
from the passwd entry, not from `$HOME`. Measured: with `HOME`
pointed at a fixture whose `ssh_config` rewrites an alias,
`ssh -G <alias>` ignores it. The isolation works only on the Windows
port, so the test still inherits a `Host *` `ProxyCommand` on Linux
and macOS.

A stub `ssh` first on `PATH` is the lever that works on both.

### rt-2026-09-02-three-comments-still-describe-the-push

**Category:** Stale prose about a removed capability

`remote/probe.rs` says `up` "creates the remote directory and ships a
tarball before `vagrant` fails"; `config/root.rs` justifies its depth
rule with `up` extracting "a tarball into `/etc`"; `remote/quote.rs`
explains a quoting rule with `scp` writing to the real home
directory. All three state current behaviour and all three are
false. Three sweeps missed them.

### rt-2026-08-31-chmod-symlink-race

**Category:** TOCTOU / privilege escalation (guest)

`bootstrap.sh` resolves the configured provisioning script with
`readlink -f`, checks the result is inside the clone, then
`chmod +x`es it and `exec`s it as root. The `chown -R` a few
lines earlier gives the agent's unprivileged user ownership of
every file in the clone, so on a re-provision of a *running* VM
that user can unlink the resolved path and put a symlink there
between the resolve and the `chmod`. `chmod` follows symlinks
even on an already-resolved path, so root sets the execute bit
on a file of the attacker's choosing.

Deferred deliberately, with the trade written into the script
beside the `chmod` rather than left for a reader to work out.
Closing it costs either the shebang (drop the `chmod`, run the
script through a named interpreter, so a Python or Ruby
provisioning script stops working) or the readability of the one
file meant to be read straight through (open the file, operate
on `/dev/fd/N`).

What it yields is the execute bit alone -- not content, not a
write -- which is worth nothing on most targets and is a
privilege escalation only on a file the user can already write
to. It also requires code already executing in the VM as that
user, timing a provision.

The `exec` has the same exposure and it does not matter: the
user owns the script already, so it can write its own content
into the file rather than race. Only the `chmod` reaches a file
outside that ownership.

Found by the fresh-reader review (FR-7), 2026-08-31. Neither the
Red Team nor Artisan pass covered it.
