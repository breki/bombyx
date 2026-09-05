//! The read-only commands `bombyx doctor` sends to the host.
//!
//! These sit apart from the rest of `remote` because they are
//! the opposite kind of command: everything else in that module
//! exists to *change* the host, and every probe here is
//! forbidden to. Keeping them in one file makes that rule
//! reviewable in one place -- and gives each probe room for the
//! paragraph explaining why it is shaped the way it is, which is
//! the part that keeps a later edit from quietly making it prove
//! less.
//!
//! The rule stated precisely: **no probe runs a command whose
//! purpose is to write**, which `doctor::mutating_token` checks
//! over these scripts. That is narrower than "doctor leaves the
//! host byte-identical", and the difference is real -- see
//! [`provider`], which runs `vagrant plugin list` and so may
//! create `~/.vagrant.d` on a host where vagrant has never run.
//! Nothing here creates, deletes or modifies anything bombyx
//! owns.
//!
//! The verdicts these scripts cannot express live in
//! `crate::doctor`.

use super::{RemoteCommand, quote_remote_path, shell_quote};
use crate::config::{Config, Transport};

/// Builds a non-interactive probe running `script`.
///
/// Each option closes a way a diagnostic can be worse than
/// useless:
///
/// - `BatchMode=yes` -- without it, a host that will not accept
///   the key waits for a password, so the probe hangs instead
///   of failing.
/// - `ConnectTimeout=10` -- `BatchMode` bounds interaction, not
///   duration. A host that blackholes TCP (a DROP rule, or a
///   dead address behind a live DNS record) would otherwise
///   block for the OS timeout, minutes, with no output at all.
///   It bounds the direct `connect()` and nothing more: it is
///   not inherited by a `ProxyCommand`/`ProxyJump`, and it does
///   not cover the banner exchange or authentication.
/// - `ServerAliveInterval=5` with `ServerAliveCountMax=3` --
///   which *does* bound a session that connects and then stalls,
///   proxied or not. Without it a hung sshd, or a jump host that
///   accepts the TCP connection and then goes quiet, hangs the
///   diagnostic indefinitely.
/// - `LogLevel=ERROR` -- suppresses banners and host-key
///   notices that would otherwise be the text reported as the
///   failure reason.
///
/// Setting them in one place is what makes the guarantee
/// structural rather than something each builder remembers.
fn probe(cfg: &Config, script: &str) -> RemoteCommand {
    // The same decision every other builder applies, read from
    // the same place -- not a second one. `probe` matches here
    // rather than calling `super::transport` because its `ssh`
    // arm carries five connection options the wrapper does not
    // add.
    //
    // Every variant named, so a third route is a compile error
    // here rather than one that quietly takes the `ssh` arm.
    match cfg.transport() {
        // Running here, none of the options below has anything
        // to configure: there is no connection to time out, no
        // session to keep alive and no banner to suppress. The
        // script is unchanged, so the shared wrapper builds it.
        Transport::Local => super::transport(cfg, script, super::Tty::NoPty),
        Transport::Ssh => RemoteCommand::new(
            "ssh",
            &[
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "LogLevel=ERROR",
                "-o",
                "ServerAliveInterval=5",
                "-o",
                "ServerAliveCountMax=3",
                cfg.host.as_str(),
                script,
            ],
        ),
    }
}

/// Builds the probe that proves the host is reachable and key
/// auth works without a prompt.
#[must_use]
pub fn reachable(cfg: &Config) -> RemoteCommand {
    probe(cfg, "true")
}

/// Builds the probe reporting where `tool` is on the host's
/// **non-interactive** `PATH`.
///
/// This is the probe the whole command exists for. The shell
/// that runs bombyx's scripts is neither an interactive nor a
/// login shell on either route -- `ssh <host> "<script>"`
/// starts one that is neither, and `sh -c "<script>"` is
/// neither by definition -- so it reads neither `~/.profile`
/// nor the interactive part of `~/.bashrc`. A
/// `vagrant` installed outside the default `PATH` is therefore
/// invisible to bombyx while working perfectly when the
/// operator logs in and types it -- and vagrant cannot report
/// that itself, because it is not running.
///
/// That trap belongs to the `ssh` route. Running here, `sh -c`
/// inherits the `PATH` bombyx was started with, which came
/// from the operator's own shell, so a `vagrant` they can type
/// is one bombyx can find. The probe still earns its place on
/// this route: it reports the `PATH` the scripts will actually
/// use, which is the question either way.
#[must_use]
pub fn command(cfg: &Config, tool: &str) -> RemoteCommand {
    probe(cfg, &format!("command -v {}", shell_quote(tool)))
}

/// Builds the probe that makes the host shell *run* a POSIX
/// construct.
///
/// bombyx sends POSIX `sh` scripts, so a `csh` or `fish` login
/// shell mangles them. Asking the shell to *perform* a POSIX
/// test and print a token means the shell that actually
/// interprets bombyx's commands is the one answering. Printing
/// `$SHELL` instead would report an environment variable a
/// wrapper can set, and would pass on whatever it said -- which
/// is exactly how an earlier version of this probe passed on a
/// `fish` shell.
///
/// The verdict lives in
/// `crate::doctor::probes::posix_shell_verdict`, because the
/// token has to be *checked*, not merely printed.
#[must_use]
pub fn posix_shell(cfg: &Config) -> RemoteCommand {
    probe(cfg, "x=1; if [ \"$x\" = 1 ]; then printf 'posix\\n'; fi")
}

/// Builds the probe that checks bombyx could write into `dir`.
///
/// Read-only on purpose: `up` would create the directory with
/// `mkdir -p`, but a diagnostic that changes state is not a
/// diagnostic. So it walks up to the nearest existing ancestor
/// and tests that instead, which is what `mkdir -p` needs
/// anyway.
///
/// Every failure names itself on stderr, so missing, unwritable,
/// not-searchable, no-ancestor and exists-but-not-a-directory
/// stay distinguishable rather than collapsing into one message
/// the operator cannot act on.
///
/// Two details are there because a simpler check passed on a host
/// where `mkdir -p` fails:
///
/// - The walk tests `-e || -L`, not `-e` alone. A dangling
///   symlink fails `-e`, so testing only that would step *past*
///   it to a writable parent and report success -- while
///   `mkdir -p` goes on to fail with `File exists`.
/// - It requires `-x` as well as `-w`. Creating an entry in a
///   directory needs write *and* search permission, so a
///   directory whose execute bit is clear (`drw-------`) is
///   writable by `test` and unusable by `mkdir`.
///
/// # What a pass does not prove
///
/// `test -w` and `test -x` call `access(2)`, which reports
/// success for **root on almost any path**. Against a root SSH
/// login this probe is therefore advisory rather than
/// conclusive. It also checks only the nearest existing ancestor,
/// not every component `mkdir -p` would create beneath it --
/// though those do not exist yet, so there is nothing to test.
#[must_use]
pub fn dir_writable(cfg: &Config, dir: &str) -> RemoteCommand {
    let script = format!(
        "d={dir}; p=$d; \
         while [ ! -e \"$p\" ] && [ ! -L \"$p\" ]; do \
         q=$(dirname \"$p\"); [ \"$q\" = \"$p\" ] && break; p=$q; done; \
         if [ ! -e \"$p\" ] && [ ! -L \"$p\" ]; then \
         echo \"no existing ancestor of $d\" >&2; exit 1; fi; \
         if [ ! -d \"$p\" ]; then \
         echo \"$p exists but is not a directory\" >&2; exit 1; fi; \
         if [ ! -w \"$p\" ]; then \
         echo \"$p is not writable\" >&2; exit 1; fi; \
         if [ ! -x \"$p\" ]; then \
         echo \"$p is writable but not searchable\" >&2; exit 1; fi; \
         if [ \"$p\" = \"$d\" ]; then echo \"$d\"; \
         else echo \"$p (will create $d)\"; fi",
        dir = quote_remote_path(dir),
    );
    probe(cfg, &script)
}

/// Builds the probe that checks Vagrant has the **libvirt**
/// plugin.
///
/// It asks about libvirt whatever the project's `provider` says,
/// so `doctor` sends it only for a libvirt project and reports a
/// skip row for any other -- `doctor::probes` holds that.
///
/// A missing provider is the one host-provisioning gap worth
/// probing: everything bombyx itself needs can be present, so
/// `up` creates the remote directory and ships a tarball before
/// `vagrant` fails. Other provisioning concerns -- `/dev/kvm`,
/// libvirtd, storage pools -- cost nothing before failing and
/// stay in `docs/vm-host-setup.md`.
///
/// # The one probe that is not strictly read-only
///
/// Every other probe runs a shell builtin or a query. This one
/// runs `vagrant`, and `vagrant plugin list` is not read-only: on
/// a host where vagrant has never run as that user it creates
/// `~/.vagrant.d` and the files under it. `doctor` therefore
/// changes nothing *of bombyx's own* and may still initialise
/// vagrant's home directory, which the module doc and the README
/// both say plainly rather than claiming more.
///
/// `VAGRANT_CHECKPOINT_DISABLE=1` is set for a second reason.
/// Without it vagrant contacts HashiCorp's checkpoint service and
/// caches the answer under `~/.vagrant.d/data`, so the probe both
/// writes more and can block: the ssh keepalive options bound a
/// dead *network*, not a slow remote command, and a firewalled
/// checkpoint endpoint would hang the probe for vagrant's own
/// timeout with no ceiling anywhere in `doctor`.
///
/// Three details each stop a specific wrong answer:
///
/// - `vagrant plugin list` exits zero and prints "No plugins
///   installed." when there are none, so its exit status alone
///   is worthless. A bare pipeline would go the other way and
///   discard vagrant's status entirely, so the output is
///   captured first and vagrant's own failure propagated.
/// - Each of the two failures names itself. Left implicit, both
///   arrive as a silent non-zero exit and the report falls back
///   to "not found" -- so "vagrant is not on the PATH" and "the
///   plugin is missing" read identically, and the operator
///   installs the wrong thing.
/// - The pattern is anchored. An unanchored `libvirt` also
///   matches `vagrant-libvirt-qemu` or a local fork.
///
/// The matched line is left unsilenced so it becomes the
/// report's detail.
///
/// On the failed-to-run path the label is printed *before*
/// vagrant's own output rather than spliced into it. Since
/// `doctor::classify` reports the last stderr line, that makes
/// vagrant's own last line the reason -- typically
/// `vagrant: command not found`, which is a better diagnosis
/// than any wording here -- while the label still stands in when
/// vagrant said nothing at all. Verified against a host with a
/// stripped `PATH`: composing the two into one line with a
/// `tail` subshell instead reported an empty reason, because a
/// `PATH` broken enough to hide `vagrant` can hide `tail` too.
///
/// **Deliberately without either environment prefix.**
/// `remote::vagrant_script` puts the two VM-host identity
/// variables on every other project call, and the provider
/// selection on `vagrant up` alone. This probe gets neither:
/// `vagrant plugin list` evaluates no `Vagrantfile`, so nothing
/// here could read the identity, and it creates no machine, so
/// it asks vagrant for no provider. That holds wherever the
/// command starts, which matters because the two routes stand
/// in different directories: over `ssh` the login directory,
/// and running here whatever directory bombyx was started in.
/// Adding the variables would suggest doctor is part of that
/// arrangement when it is not.
#[must_use]
pub fn provider(cfg: &Config) -> RemoteCommand {
    probe(
        cfg,
        "out=$(VAGRANT_CHECKPOINT_DISABLE=1 vagrant plugin list 2>&1) || { \
         printf 'vagrant plugin list failed\\n%s\\n' \"$out\" >&2; \
         exit 1; }; \
         printf '%s\\n' \"$out\" | grep '^vagrant-libvirt[ (]' || { \
         echo 'vagrant-libvirt plugin not installed' >&2; exit 1; }",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config::for_tests()
    }

    /// Every probe command a live run sends.
    ///
    /// Derived from `doctor::host_probes` rather than listed here.
    /// The listed version claimed in its own comment to prevent
    /// "naming five and missing the sixth" while being exactly
    /// such a list: a new builder would have been skipped by every
    /// test below with nothing failing. Deriving it also means
    /// these assertions cover the commands that are really sent.
    fn all(cfg: &Config) -> Vec<RemoteCommand> {
        crate::doctor::probe_commands(&crate::doctor::host_probes(cfg))
    }

    #[test]
    fn every_probe_sets_the_safety_options() {
        // What each option prevents: see probe()'s doc. Asserted
        // per probe, because setting them in one helper is only a
        // guarantee while every probe goes through that helper.
        for p in all(&cfg()) {
            assert_eq!(p.program, "ssh");
            for opt in [
                "BatchMode=yes",
                "ConnectTimeout=10",
                "LogLevel=ERROR",
                "ServerAliveInterval=5",
                "ServerAliveCountMax=3",
            ] {
                assert!(
                    p.args.iter().any(|a| a == opt),
                    "probe must set {opt}: {:?}",
                    p.args
                );
            }
        }
    }

    #[test]
    fn reachable_runs_true_on_the_host() {
        assert_eq!(reachable(&cfg()).args.last().unwrap(), "true");
    }

    #[test]
    fn command_asks_the_non_interactive_shell() {
        // `command -v` over a plain ssh call is the whole
        // point: it reports the PATH bombyx actually gets.
        let c = command(&cfg(), "vagrant");
        assert_eq!(c.args.last().unwrap(), "command -v 'vagrant'");
    }

    #[test]
    fn posix_shell_makes_the_shell_prove_itself() {
        // Why naming the shell is not enough: see posix_shell().
        let script = posix_shell(&cfg()).args.last().unwrap().clone();
        assert!(script.contains("printf 'posix"), "{script}");
        assert!(!script.contains("$SHELL"), "{script}");
    }

    #[test]
    fn dir_writable_walks_up_and_never_creates() {
        let c = dir_writable(&cfg(), "~/vms/myproject");
        let script = c.args.last().unwrap();
        assert!(script.starts_with("d=~/'vms/myproject';"), "{script}");
        // Each failure names itself, so five distinct host
        // states stop collapsing into one "not found".
        for named in [
            "no existing ancestor",
            "is not a directory",
            "is not writable",
        ] {
            assert!(script.contains(named), "{named}: {script}");
        }
    }

    #[test]
    fn dir_writable_does_not_step_past_a_dangling_symlink() {
        // `-e` is false for a dangling symlink, so a walk
        // testing only that would carry on to a writable parent
        // and report success -- while `mkdir -p` fails with
        // `File exists`.
        let c = dir_writable(&cfg(), "~/vms/myproject");
        let script = c.args.last().unwrap();
        assert!(
            script.contains("[ ! -e \"$p\" ] && [ ! -L \"$p\" ]"),
            "{script}"
        );
    }

    #[test]
    fn provider_distinguishes_a_missing_vagrant_from_a_missing_plugin() {
        let script = provider(&cfg()).args.last().unwrap().clone();
        // `vagrant plugin list` exits 0 and prints "No plugins
        // installed." when there are none, so its status alone
        // proves nothing -- but a bare pipeline would discard
        // vagrant's own failure, so the output is captured and
        // the status propagated.
        assert!(script.contains("vagrant plugin list failed"), "{script}");
        // The label precedes vagrant's output, so the reported
        // reason is vagrant's own last line. Folding them into
        // one line needed a `tail` subshell, and a PATH broken
        // enough to hide `vagrant` hides `tail` too -- which on a
        // real host produced an empty reason.
        assert!(!script.contains("tail"), "{script}");
        // And the two failures must not read alike: without its
        // own message the missing plugin arrives as a silent
        // non-zero exit and renders as "not found", the same
        // text a missing vagrant produces.
        assert!(script.contains("plugin not installed"), "{script}");
        // Anchored: an unanchored `libvirt` also matches
        // `vagrant-libvirt-qemu` or a local fork.
        assert!(script.contains("grep '^vagrant-libvirt"), "{script}");
    }
}
