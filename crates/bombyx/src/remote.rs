//! Building the commands that drive Vagrant on the VM host.
//!
//! Every operation is a plain `ssh`, `scp` or `tar`
//! invocation. Nothing here runs a process: these functions
//! return the argv to run, which keeps the interesting logic
//! (quoting, paths, command composition) unit-testable
//! without a VM host.
//!
//! This module is the builders and the VM-host identity constants.
//! Two neighbours hold the pieces they are built from: `command`
//! defines [`RemoteCommand`], and `quote` holds
//! the POSIX quoting primitives -- pure functions with their own
//! dense test block and no dependency on [`Config`], which is why
//! they read as a separate unit. Both are re-exported, so
//! `bombyx::remote::shell_quote` is an unchanged path.

mod command;
pub mod probe;
mod quote;
mod write;

pub use command::RemoteCommand;
pub use quote::{quote_remote_path, shell_quote};
pub use write::write_file;

use crate::config::Config;

/// Environment variable carrying the VM host's SSH alias into
/// the `vagrant` process on the host.
///
/// The alias as bombyx knows it -- `frosti`, `my-vmhost` -- which
/// is the name the operator recognises, since they chose it.
///
/// # Why this exists
///
/// A VM booted by bombyx cannot work out which machine it is
/// running on. There is no synced folder to read, `hostname`
/// inside the guest answers with the guest's own name, and
/// libvirt does not pass the host's name in at all: the guest's
/// SMBIOS/DMI describes the *emulated* machine, so
/// `/sys/class/dmi/id/sys_vendor` reads `QEMU` and
/// `product_name` names a QEMU machine type. Measured inside a
/// live guest as the unprivileged user -- those files are
/// readable and simply hold nothing about the host, while the
/// root-only ones (`product_serial`) carry no host name either.
/// There is nothing to read at any privilege level.
///
/// So this and [`VM_HOSTNAME_ENV`] travel on the `vagrant`
/// invocation instead.
///
/// # The project's half
///
/// The variables reach the `vagrant` process on the host and no
/// further: **Vagrant does not export its own environment into a
/// guest.** A provisioner runs inside the guest under the guest's
/// environment, so anything from the host has to be handed over
/// deliberately. The `Vagrantfile` is Ruby running on the host,
/// so it can read these and pass them to a shell provisioner
/// through its `env:` option -- see the "Telling the VM which
/// host it runs on" section of `README.md`.
pub const VM_HOST_ENV: &str = "BOMBYX_VM_HOST";

/// Environment variable carrying what the VM host calls itself.
///
/// The machine's own short name, which need not match
/// [`VM_HOST_ENV`]: an alias in `~/.ssh/config` can be anything,
/// and often is. Both are passed so a guest can show the alias
/// and still tell that the two disagree.
///
/// The two *can* legitimately agree. A WSL2 distribution that has
/// not been given a name of its own reports the Windows machine's
/// name, so on that kind of host this value may equal the
/// workstation's -- expected, not a sign of the wrong-side
/// expansion that `vm_host_env` guards against. Not a doc link:
/// that function is private, and rustdoc rejects a public page
/// pointing at a private item.
pub const VM_HOSTNAME_ENV: &str = "BOMBYX_VM_HOSTNAME";

/// The environment prefix that tells the guest which machine it
/// is running on.
///
/// See [`VM_HOST_ENV`] for why the guest cannot find this out for
/// itself, and what the project's `Vagrantfile` has to do with the
/// values.
///
/// **`$(hostname -s)` is left unexpanded on purpose.** bombyx
/// spawns `ssh` directly rather than through a shell, so the
/// substitution reaches the host's shell and answers with the
/// host's name. Expanded on this side it would answer with the
/// *workstation's* name -- a plausible-looking wrong answer, which
/// is the failure mode worth guarding. It is unquoted because a
/// shell assignment does not field-split its value, so the quotes
/// would only add noise to the dry-run output.
///
/// A host with no `hostname` command leaves the variable empty
/// rather than failing the boot. Reporting an unknown host name is
/// a smaller problem than refusing to start a VM over a status
/// line.
fn vm_host_env(cfg: &Config) -> String {
    format!(
        "{VM_HOST_ENV}={host} {VM_HOSTNAME_ENV}=$(hostname -s)",
        host = shell_quote(&cfg.host),
    )
}

/// Builds the `vagrant` command itself: the identity prefix, the
/// program, and its quoted arguments.
///
/// Split out from [`vagrant_script`] so the two shapes bombyx
/// emits cannot disagree about the prefix. `destroy` needs the
/// command nested inside a `if [ -f Vagrantfile ]` guard rather
/// than after a bare `cd`, and when it built its own string it
/// silently ran `vagrant` with neither variable set -- while the
/// doc comment here claimed every invocation carried them.
fn vagrant_command(cfg: &Config, args: &[&str]) -> String {
    let mut cmd = format!("{} vagrant", vm_host_env(cfg));
    for arg in args {
        cmd.push(' ');
        cmd.push_str(&shell_quote(arg));
    }
    cmd
}

/// Builds the remote script that enters `dir` and runs
/// `vagrant` with `args`.
///
/// Every vagrant invocation that runs **inside a project
/// directory** carries [`vm_host_env`], not just the ones that
/// provision. `halt` and `status` have no use for the values, and
/// setting them in one place is what keeps the action that *does*
/// need them from being the one that was forgotten.
///
/// The one exemption is `doctor`, whose probes
/// ([`probe::provider`]) run in the SSH login directory rather
/// than a project directory. They inspect the host's own vagrant
/// installation and evaluate no `Vagrantfile`, so there is nothing
/// there to read the variables.
fn vagrant_script(cfg: &Config, dir: &str, args: &[&str]) -> String {
    format!(
        "cd {dir} && {cmd}",
        dir = quote_remote_path(dir),
        cmd = vagrant_command(cfg, args),
    )
}

/// Whether `ssh` should allocate a remote pseudo-terminal (`-t`).
///
/// It decides more than interactivity, which is why it is a
/// parameter rather than a detail of the one command that obviously
/// needs it. Without a PTY the remote program's stdout is a pipe, so
/// the remote tty layer never translates `\n` to `\r\n`, and every
/// line arrives with a bare line feed. Measured against a real host:
/// `vagrant status` returned 206 bytes containing six line feeds and
/// **no** carriage returns, which a Windows console renders as a
/// staircase -- each line starting at the column where the last one
/// ended.
///
/// So the choice is not cosmetic, and it is not free either.
///
/// [`Tty::Allocate`] buys readable line endings, and lets the remote
/// colourize. It costs three things: the remote's stderr is merged
/// into stdout, the local terminal goes into raw mode, and it needs a
/// local terminal to allocate against at all -- without one, ssh
/// prints `Pseudo-terminal will not be allocated because stdin is
/// not a terminal.` and carries on with no PTY, which is measured
/// and is why the caller inspects stdin.
///
/// It also carries `-o LogLevel=ERROR`, and that is not tidying.
/// A tty session makes ssh print `Connection to <host> closed.` to
/// stderr when it ends, so without this every `status`, `up` and
/// teardown would gain a spurious trailing line. Measured: the
/// message appears at the default level and not at `ERROR`, while a
/// genuine failure still reports identically -- an unresolvable host
/// gives the same message and the same 255 either way. `QUIET`
/// suppresses the line too and was rejected: it would also swallow
/// real diagnostics from a tool whose failures matter.
///
/// [`Tty::NoPty`] keeps the bytes exactly as the remote wrote them,
/// which is what a pipe, a redirect or a parsed probe needs.
///
/// `doctor`'s probes are the case that must stay [`Tty::NoPty`]:
/// their output is compared and sanitized, and a PTY would fold
/// control characters and CRs into the text being checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tty {
    /// Pass `-t`, so the remote gets a pseudo-terminal.
    Allocate,
    /// No `-t`; the remote writes to a pipe.
    ///
    /// Spelled `NoPty` rather than `None` so a `match` arm cannot be
    /// misread as an absent [`Option`].
    NoPty,
}

impl Tty {
    /// The choice for a run whose streams are (or are not)
    /// terminals.
    ///
    /// **Both have to be terminals**, and each for its own reason.
    /// `ssh -t` needs a local terminal to allocate against, and
    /// merely warns and carries on without one. And the reason to
    /// want a PTY is that the remote tty then translates `\n` to
    /// `\r\n`, which only helps when the output is going to a
    /// terminal -- piped or redirected, the bytes must stay exactly
    /// as the remote wrote them, or a captured log gains carriage
    /// returns and, since the remote colourizes under a PTY, escape
    /// sequences too.
    ///
    /// A pure function of two booleans so the rule is testable; the
    /// binary supplies them from `IsTerminal`.
    #[must_use]
    pub fn for_streams(stdin_tty: bool, stdout_tty: bool) -> Self {
        if stdin_tty && stdout_tty {
            Self::Allocate
        } else {
            Self::NoPty
        }
    }
}

/// Builds an `ssh` command running `vagrant` in `dir` on the
/// VM host.
///
/// See [`Tty`] for what the last argument costs and buys.
#[must_use]
pub fn vagrant_in(
    cfg: &Config,
    dir: &str,
    args: &[&str],
    tty: Tty,
) -> RemoteCommand {
    let script = vagrant_script(cfg, dir, args);
    // `-t` ahead of the destination: ssh takes options before the
    // host, and everything after the host is the remote command.
    match tty {
        Tty::Allocate => RemoteCommand::new(
            "ssh",
            &["-t", "-o", "LogLevel=ERROR", &cfg.host, &script],
        ),
        Tty::NoPty => RemoteCommand::new("ssh", &[&cfg.host, &script]),
    }
}

/// Builds an `ssh` command running `vagrant` in the project
/// directory on the VM host.
#[must_use]
pub fn vagrant(cfg: &Config, args: &[&str], tty: Tty) -> RemoteCommand {
    vagrant_in(cfg, &cfg.remote_project_dir(), args, tty)
}

/// Builds the `ssh` command that creates `dir` on the VM
/// host if it does not yet exist.
#[must_use]
pub fn ensure_dir(cfg: &Config, dir: &str) -> RemoteCommand {
    let script = format!("mkdir -p {}", quote_remote_path(dir));
    RemoteCommand::new("ssh", &[&cfg.host, &script])
}

/// Builds the `ssh` command that destroys the VM defined in
/// `dir`, doing nothing when there is no Vagrantfile there.
///
/// The guard makes teardown idempotent. A bare
/// `vagrant destroy -f` exits non-zero in a directory with no
/// Vagrantfile, which an interrupted first push leaves behind,
/// and that failure would stop the removal step that follows.
///
/// The command comes from the same private `vagrant_command`
/// helper the other builders use, so it carries the same identity
/// prefix as every other invocation. It matters
/// here more than it looks: teardown still *evaluates* the
/// project's `Vagrantfile`, so one reading
/// `ENV.fetch("BOMBYX_VM_HOST")` without a default would raise on
/// `destroy` after working on `up` -- and since execution stops at
/// the first failing step, the directory removal that follows
/// would never run.
/// Takes a [`Tty`] like the other vagrant builders, and for the
/// same reason: `vagrant destroy -f` prints several lines of
/// progress, so without one `destroy` and `discard` staircase on
/// the console this parameter exists to fix. It was the sibling
/// left out when `Tty` was introduced -- the reachable-primitive
/// case `CLAUDE.md` warns about, found by review rather than by a
/// test.
#[must_use]
pub fn destroy_vm_if_present(
    cfg: &Config,
    dir: &str,
    tty: Tty,
) -> RemoteCommand {
    let script = format!(
        "cd {dir} && if [ -f Vagrantfile ]; then {cmd}; fi",
        dir = quote_remote_path(dir),
        cmd = vagrant_command(cfg, &["destroy", "-f"]),
    );
    match tty {
        Tty::Allocate => RemoteCommand::new(
            "ssh",
            &["-t", "-o", "LogLevel=ERROR", &cfg.host, &script],
        ),
        Tty::NoPty => RemoteCommand::new("ssh", &[&cfg.host, &script]),
    }
}

/// Builds the `ssh` command that recursively removes `dir` on
/// the VM host.
///
/// This is the widest-reaching command bombyx emits: its blast
/// radius is bounded by a path rather than by Vagrant's notion
/// of a machine. Nothing is checked here, deliberately --
/// `Config::validate` rejects a `remote_root` that is
/// unrooted, contains a `.` or `..` segment, or is too shallow,
/// so every path derived from a loaded `Config` is already at
/// least two real segments deep. Validating once at the layer
/// that owns `remote_root` is what keeps the write path
/// (`mkdir`, `tar -xzf`) and this removal path agreeing about
/// which roots are usable.
///
/// The `debug_assert` catches a caller that builds a path some
/// other way; it is not the safety mechanism.
#[must_use]
pub fn remove_dir(cfg: &Config, dir: &str) -> RemoteCommand {
    debug_assert!(
        crate::config::path_segments(dir).len() >= 2,
        "remove_dir given a path shallower than Config permits: {dir:?}"
    );
    let script = format!("rm -rf {}", quote_remote_path(dir));
    RemoteCommand::new("ssh", &[&cfg.host, &script])
}

/// Builds the `ssh` command that opens an interactive shell
/// inside the project's VM.
///
/// Always [`Tty::Allocate`], and unconditionally so: `vagrant ssh`
/// needs a TTY when invoked through a non-interactive SSH command,
/// and an interactive shell without one is unusable whatever the
/// local stdio looks like. Every other vagrant call decides per run.
#[must_use]
pub fn shell_into_vm(cfg: &Config) -> RemoteCommand {
    vagrant_in(cfg, &cfg.remote_project_dir(), &["ssh"], Tty::Allocate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::name::ScratchName;

    /// The ssh options that precede the destination, in order.
    ///
    /// Asserted as a whole rather than by index. A test pinning
    /// `args[1]` as the host passes an argv where a new option
    /// has pushed the host somewhere else: the index still holds
    /// a string, just the wrong one. Comparing the whole list is
    /// what notices.
    fn opts_before_host(c: &RemoteCommand) -> Vec<String> {
        c.args
            .iter()
            .take_while(|a| *a != "vmhost")
            .cloned()
            .collect()
    }

    /// The remote script, whatever precedes it.
    fn remote_script(c: &RemoteCommand) -> String {
        c.args.last().expect("a remote command").clone()
    }

    #[test]
    fn a_tty_run_asks_for_a_pty_and_silences_the_closing_notice() {
        // Order matters to ssh: options come before the destination,
        // and everything after it is the remote command, so `-t`
        // landing after the host would be handed to the remote
        // shell instead.
        //
        // `LogLevel=ERROR` is measured, not decoration: a tty
        // session makes ssh print `Connection to <host> closed.` to
        // stderr, which would end every status and up with a
        // spurious line. A genuine failure still reports at this
        // level.
        let c = vagrant(&cfg(), &["status"], Tty::Allocate);
        assert_eq!(c.program, "ssh");
        assert_eq!(opts_before_host(&c), vec!["-t", "-o", "LogLevel=ERROR"]);
        assert!(remote_script(&c).contains("vagrant 'status'"));
    }

    #[test]
    fn no_tty_passes_no_options_at_all() {
        // The default for a pipe or a redirect: the remote's bytes
        // arrive unchanged, which is what a captured log needs, and
        // ssh emits no pseudo-terminal warning.
        let c = vagrant(&cfg(), &["status"], Tty::NoPty);
        assert!(opts_before_host(&c).is_empty(), "{:?}", c.args);
        assert_eq!(c.args.len(), 2);
    }

    #[test]
    fn the_tty_choice_does_not_disturb_the_remote_script() {
        // Only the argv ahead of the host differs. If the script
        // itself changed with the tty, the printed plan and the
        // executed one would describe different work.
        let with = vagrant(&cfg(), &["status"], Tty::Allocate);
        let without = vagrant(&cfg(), &["status"], Tty::NoPty);
        assert_eq!(remote_script(&with), remote_script(&without));
    }

    #[test]
    fn an_interactive_shell_always_gets_a_tty() {
        // Unconditional here, unlike every other vagrant call:
        // `vagrant ssh` needs a TTY through a non-interactive SSH
        // command, and a shell without one is unusable whatever the
        // local stdio looks like.
        let c = shell_into_vm(&cfg());
        assert_eq!(opts_before_host(&c), vec!["-t", "-o", "LogLevel=ERROR"]);
        assert_eq!(
            remote_script(&c),
            remote_script(&vagrant(&cfg(), &["ssh"], Tty::NoPty))
        );
    }

    #[test]
    fn teardown_takes_a_tty_like_every_other_vagrant_call() {
        // The sibling that was left out when `Tty` was introduced.
        // `vagrant destroy -f` streams progress, so it staircased on
        // the very console this parameter exists to fix.
        let with = destroy_vm_if_present(&cfg(), "~/vms/p", Tty::Allocate);
        assert_eq!(opts_before_host(&with), vec!["-t", "-o", "LogLevel=ERROR"]);
        let without = destroy_vm_if_present(&cfg(), "~/vms/p", Tty::NoPty);
        assert!(opts_before_host(&without).is_empty());
        assert_eq!(remote_script(&with), remote_script(&without));
    }

    #[test]
    fn the_stream_rule_needs_both_streams() {
        // stdin, because ssh needs a local terminal to allocate
        // against and merely warns without one; stdout, because the
        // translation only helps output that reaches a terminal.
        assert_eq!(Tty::for_streams(true, true), Tty::Allocate);
        assert_eq!(Tty::for_streams(true, false), Tty::NoPty);
        assert_eq!(Tty::for_streams(false, true), Tty::NoPty);
        assert_eq!(Tty::for_streams(false, false), Tty::NoPty);
    }
    fn cfg() -> Config {
        Config::for_tests()
    }

    /// The identity prefix every vagrant script carries.
    ///
    /// `vagrant_carries_the_vm_host_identity` spells it out in
    /// full. The tests below reference this instead, because their
    /// subject is the directory and the arguments; repeating the
    /// prefix in each of them would push every assertion past the
    /// line limit and give it several places to drift.
    ///
    /// Built from the exported constants rather than hardcoding
    /// their values. Hardcoded, renaming either constant would
    /// leave this module green while bombyx exported a different
    /// variable name -- which is the one failure these assertions
    /// exist to catch.
    fn vm_env() -> String {
        format!("{VM_HOST_ENV}='vmhost' {VM_HOSTNAME_ENV}=$(hostname -s)")
    }

    #[test]
    fn vagrant_carries_the_vm_host_identity() {
        // The guest cannot work out which machine it runs on:
        // there is no synced folder, `hostname` inside the VM
        // answers with the guest's own name, and libvirt puts
        // nothing about the host anywhere a non-root process can
        // read. So the two names ride in on the one command that
        // crosses the boundary, and the guest's provisioning
        // writes them down.
        let c = vagrant(&cfg(), &["up"], Tty::NoPty);
        assert_eq!(
            c.args[1],
            "cd ~/'vms/myproject' && BOMBYX_VM_HOST='vmhost' \
             BOMBYX_VM_HOSTNAME=$(hostname -s) vagrant 'up'"
        );
    }

    #[test]
    fn the_hostname_is_evaluated_on_the_far_side() {
        // `$(...)` in a remote command is the wrong-side
        // expansion trap: expanded here it would report the
        // *workstation's* name, which is plausible enough that
        // nobody would question it. bombyx spawns `ssh` directly
        // rather than through a shell, so the substitution
        // reaches the host verbatim -- and the dry run has to
        // show it escaped, or a pasted line would answer with
        // the wrong machine.
        let c = vagrant(&cfg(), &["up"], Tty::NoPty);
        assert!(c.args[1].contains("$(hostname -s)"), "{}", c.args[1]);
        assert!(c.to_string().contains(r"\$(hostname -s)"), "{c}");
    }

    #[test]
    fn teardown_carries_the_identity_too() {
        // The regression this test exists for: `destroy` builds
        // its command inside an `if [ -f Vagrantfile ]` guard
        // rather than after a bare `cd`, so it once assembled its
        // own string and ran `vagrant` with neither variable set --
        // while the doc comment claimed every invocation carried
        // them.
        //
        // It matters most here. Teardown still evaluates the
        // project's Vagrantfile, so one reading the variable
        // without a default would raise on `destroy` after working
        // on `up`, and the directory removal that follows would
        // never run.
        //
        // Exhaustiveness across actions is asserted in `plan`,
        // which can enumerate them; this pins the one builder that
        // does not go through `vagrant_script`.
        let script =
            destroy_vm_if_present(&cfg(), "~/vms/myproject", Tty::NoPty).args
                [1]
            .clone();
        assert!(script.contains(&vm_env()), "{script}");
    }

    #[test]
    fn the_vm_host_alias_is_quoted_in_the_script() {
        // Second line of defence: `Config::validate` rejects
        // these characters. The alias is interpolated into a
        // remote script here, so it is quoted like every other
        // interpolated value rather than trusted because another
        // module checked it.
        let mut cfg = cfg();
        cfg.host = "a b; rm -rf /".to_owned();
        let script = vagrant(&cfg, &["up"], Tty::NoPty).args[1].clone();
        assert!(
            script.contains("BOMBYX_VM_HOST='a b; rm -rf /'"),
            "{script}"
        );
    }

    #[test]
    fn builds_a_vagrant_command() {
        let c = vagrant(&cfg(), &["up"], Tty::NoPty);
        let env = vm_env();
        assert_eq!(c.program, "ssh");
        assert_eq!(c.args[0], "vmhost");
        assert_eq!(
            c.args[1],
            format!("cd ~/'vms/myproject' && {env} vagrant 'up'")
        );
    }

    #[test]
    fn builds_a_vagrant_command_with_several_args() {
        let c = vagrant(
            &cfg(),
            &["snapshot", "restore", "fresh-install"],
            Tty::NoPty,
        );
        let env = vm_env();
        assert_eq!(
            c.args[1],
            format!(
                "cd ~/'vms/myproject' && {env} vagrant 'snapshot' \
                 'restore' 'fresh-install'"
            )
        );
    }

    #[test]
    fn builds_a_scratch_command() {
        let cfg = cfg();
        let name = ScratchName::parse("pr-1234").unwrap();
        let c = vagrant_in(
            &cfg,
            &cfg.remote_scratch_dir(&name),
            &["destroy", "-f"],
            Tty::NoPty,
        );
        let env = vm_env();
        assert_eq!(
            c.args[1],
            format!(
                "cd ~/'vms/scratch/myproject/pr-1234' && {env} \
                 vagrant 'destroy' '-f'"
            )
        );
    }

    #[test]
    fn vagrant_runs_in_the_project_dir() {
        // `vagrant up` reads the Vagrantfile from the directory
        // it runs in, so the command has to cd there first.
        let cfg = cfg();
        let quoted = quote_remote_path(&cfg.remote_project_dir());
        assert!(
            vagrant(&cfg, &["up"], Tty::NoPty).args[1]
                .starts_with(&format!("cd {quoted} &&"))
        );
    }

    #[test]
    fn ensure_dir_keeps_the_tilde_expandable() {
        let c = ensure_dir(&cfg(), "~/vms/scratch/pr-1");
        assert_eq!(c.args[1], "mkdir -p ~/'vms/scratch/pr-1'");
    }

    #[test]
    fn ensure_dir_quotes_an_absolute_dir() {
        let c = ensure_dir(&cfg(), "/srv/vms/p");
        assert_eq!(c.args[1], "mkdir -p '/srv/vms/p'");
    }

    #[test]
    fn remove_dir_quotes_the_path_and_keeps_the_tilde() {
        let c = remove_dir(&cfg(), "~/vms/myproject");
        assert_eq!(c.program, "ssh");
        assert_eq!(c.args[0], "vmhost");
        assert_eq!(c.args[1], "rm -rf ~/'vms/myproject'");
    }

    #[test]
    fn remove_dir_removes_an_absolute_path() {
        let c = remove_dir(&cfg(), "/srv/vms/myproject");
        assert_eq!(c.args[1], "rm -rf '/srv/vms/myproject'");
    }

    #[test]
    fn remove_dir_quotes_injection_in_the_path() {
        // Config rejects these characters, so this is the
        // second line of defence rather than the first.
        let c = remove_dir(&cfg(), "~/vms/a b; rm /");
        assert_eq!(c.args[1], "rm -rf ~/'vms/a b; rm /'");
    }

    #[test]
    fn destroy_tolerates_a_directory_with_no_vagrantfile() {
        // An interrupted first push leaves the directory made
        // but empty. A bare `vagrant destroy -f` fails there,
        // and would stop the removal that follows.
        let c = destroy_vm_if_present(&cfg(), "~/vms/myproject", Tty::NoPty);
        assert_eq!(
            c.args[1],
            format!(
                "cd ~/'vms/myproject' && if [ -f Vagrantfile ]; then \
                 {} vagrant 'destroy' '-f'; fi",
                vm_env()
            )
        );
    }

    #[test]
    fn vagrant_in_runs_in_the_given_dir() {
        let c = vagrant_in(&cfg(), "/srv/x", &["halt"], Tty::NoPty);
        let env = vm_env();
        assert_eq!(c.args[1], format!("cd '/srv/x' && {env} vagrant 'halt'"));
    }
}
