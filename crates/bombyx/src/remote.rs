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
//! defines [`RemoteCommand`] and [`PushArchive`], and `quote` holds
//! the POSIX quoting primitives -- pure functions with their own
//! dense test block and no dependency on [`Config`], which is why
//! they read as a separate unit. Both are re-exported, so
//! `bombyx::remote::shell_quote` is an unchanged path.

mod command;
pub mod probe;
mod quote;

pub use command::{PushArchive, RemoteCommand};
pub use quote::{quote_remote_path, shell_quote};

use std::path::Path;

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

/// Builds an `ssh` command running `vagrant` in `dir` on the
/// VM host.
#[must_use]
pub fn vagrant_in(cfg: &Config, dir: &str, args: &[&str]) -> RemoteCommand {
    let script = vagrant_script(cfg, dir, args);
    RemoteCommand::new("ssh", &[&cfg.host, &script])
}

/// Builds an `ssh` command running `vagrant` in the project
/// directory on the VM host.
#[must_use]
pub fn vagrant(cfg: &Config, args: &[&str]) -> RemoteCommand {
    vagrant_in(cfg, &cfg.remote_project_dir(), args)
}

/// Builds the commands that push a local directory's
/// **contents** into `remote_dir` on the VM host.
///
/// The repo stays the source of truth; the host receives a
/// copy before every boot so the two cannot drift.
///
/// This ships a tar archive rather than using `scp -r` or
/// `rsync`, for two reasons:
///
/// - `scp -r <dir> host:<dest>/` copies *into* an existing
///   destination, like `cp -r`. The first push creates
///   `<dest>/<dir>`; the second creates
///   `<dest>/<dir>/<dir>`. Extracting a tar over an
///   existing tree instead overwrites in place, so
///   repeated pushes are idempotent.
/// - `rsync` is not present on a stock Windows workstation,
///   which is where bombyx runs. `tar`, `scp` and `ssh` all
///   are.
///
/// `.vagrant/` holds the VM's identity on the host and is
/// excluded from the archive, so a developer who has ever run
/// `vagrant` locally cannot overwrite the host's copy and
/// orphan a running VM. `.git/` is excluded because there is
/// no reason to ship it.
///
/// The tradeoff of extract-in-place is that a file deleted
/// locally is not removed from the host; run `vagrant
/// destroy` and re-push if the remote tree needs pruning.
#[must_use]
pub fn push_dir(
    cfg: &Config,
    local_dir: &Path,
    remote_dir: &str,
    archive: &PushArchive,
) -> Vec<RemoteCommand> {
    let remote_archive = quote_remote_path(&format!("~/{}", archive.name));
    // Cleanup runs whether or not the extract succeeded: a
    // half-written archive left in the project directory
    // would be swept into the tree `vagrant up` runs in.
    let unpack = format!(
        "{{ cd {dir} && tar -xzf {a}; }}; rc=$?; rm -f {a}; exit $rc",
        dir = quote_remote_path(remote_dir),
        a = remote_archive,
    );
    let local = local_dir.to_string_lossy().into_owned();
    let dest = format!("{}:{}", cfg.host, archive.name);
    vec![
        // `-C <dir> .` archives the contents, not the
        // directory itself, so extraction lands files
        // directly in `remote_dir`.
        RemoteCommand::new(
            "tar",
            &[
                "-czf",
                &archive.name,
                "-C",
                &local,
                "--exclude=./.vagrant",
                "--exclude=./.git",
                ".",
            ],
        )
        .in_dir(&archive.dir),
        RemoteCommand::new("scp", &[&archive.name, &dest]).in_dir(&archive.dir),
        RemoteCommand::new("ssh", &[&cfg.host, &unpack]),
    ]
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
#[must_use]
pub fn destroy_vm_if_present(cfg: &Config, dir: &str) -> RemoteCommand {
    let script = format!(
        "cd {dir} && if [ -f Vagrantfile ]; then {cmd}; fi",
        dir = quote_remote_path(dir),
        cmd = vagrant_command(cfg, &["destroy", "-f"]),
    );
    RemoteCommand::new("ssh", &[&cfg.host, &script])
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
/// `-t` forces a TTY, which `vagrant ssh` needs when invoked
/// through a non-interactive SSH command.
#[must_use]
pub fn shell_into_vm(cfg: &Config) -> RemoteCommand {
    let script = vagrant_script(cfg, &cfg.remote_project_dir(), &["ssh"]);
    RemoteCommand::new("ssh", &["-t", &cfg.host, &script])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::name::ScratchName;
    use std::path::PathBuf;

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

    fn archive() -> PushArchive {
        PushArchive::new(Path::new("/work"), "42")
    }

    fn push() -> Vec<RemoteCommand> {
        push_dir(
            &cfg(),
            Path::new("/repo/vagrant"),
            "~/vms/myproject",
            &archive(),
        )
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
        let c = vagrant(&cfg(), &["up"]);
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
        let c = vagrant(&cfg(), &["up"]);
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
            destroy_vm_if_present(&cfg(), "~/vms/myproject").args[1].clone();
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
        let script = vagrant(&cfg, &["up"]).args[1].clone();
        assert!(
            script.contains("BOMBYX_VM_HOST='a b; rm -rf /'"),
            "{script}"
        );
    }

    #[test]
    fn builds_a_vagrant_command() {
        let c = vagrant(&cfg(), &["up"]);
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
        let c = vagrant(&cfg(), &["snapshot", "restore", "fresh-install"]);
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
    fn push_emits_exactly_three_steps_in_order() {
        let cmds = push();
        let programs: Vec<&str> =
            cmds.iter().map(|c| c.program.as_str()).collect();
        assert_eq!(programs, vec!["tar", "scp", "ssh"]);
    }

    #[test]
    fn push_archives_contents_not_the_directory() {
        // `-C <dir> .` is what makes the push idempotent:
        // archiving the directory itself would nest it one
        // level deeper on every push.
        assert_eq!(
            push()[0].args,
            vec![
                "-czf",
                ".bombyx-push-42.tar.gz",
                "-C",
                "/repo/vagrant",
                "--exclude=./.vagrant",
                "--exclude=./.git",
                "."
            ]
        );
    }

    #[test]
    fn push_excludes_the_hosts_vm_identity() {
        // Shipping a local `.vagrant/` overwrites the host's
        // machine id and orphans the running VM.
        assert!(
            push()[0].args.iter().any(|a| a == "--exclude=./.vagrant"),
            "the push must not carry a local .vagrant/"
        );
    }

    #[test]
    fn push_runs_tar_and_scp_in_the_archive_dir() {
        // Both must use the bare file name, or an absolute
        // Windows path makes scp read `C:` as a host name.
        let cmds = push();
        let dir = Some(PathBuf::from("/work"));
        assert_eq!(cmds[0].dir, dir);
        assert_eq!(cmds[1].dir, dir);
        assert_eq!(cmds[2].dir, None);
    }

    #[test]
    fn push_never_passes_a_drive_letter_to_scp() {
        let archive = PushArchive::new(
            Path::new(r"C:\Users\igor\AppData\Local\Temp"),
            "42",
        );
        let cmds = push_dir(
            &cfg(),
            Path::new("/repo/vagrant"),
            "~/vms/myproject",
            &archive,
        );
        for arg in &cmds[1].args {
            assert!(
                !arg.contains(r":\"),
                "scp argument {arg:?} carries a drive letter"
            );
        }
    }

    #[test]
    fn push_copies_the_archive_to_the_remote_home() {
        assert_eq!(
            push()[1].args,
            vec![".bombyx-push-42.tar.gz", "vmhost:.bombyx-push-42.tar.gz"]
        );
    }

    #[test]
    fn push_removes_the_archive_even_when_extraction_fails() {
        // `&&`-chaining the cleanup would leave a corrupt
        // archive inside the tree `vagrant up` runs in.
        assert_eq!(
            push()[2].args[1],
            "{ cd ~/'vms/myproject' && tar -xzf \
             ~/'.bombyx-push-42.tar.gz'; }; rc=$?; rm -f \
             ~/'.bombyx-push-42.tar.gz'; exit $rc"
        );
    }

    #[test]
    fn push_never_uses_scp_recursive() {
        // Regression guard: `scp -r` into an existing
        // destination nests the directory on every push.
        for c in &push() {
            assert!(
                !(c.program == "scp" && c.args.iter().any(|a| a == "-r")),
                "push must not use scp -r"
            );
        }
    }

    #[test]
    fn push_targets_the_dir_vagrant_runs_in() {
        // The Vagrantfile must land where `vagrant up` runs,
        // otherwise the boot fails with no Vagrantfile.
        let cfg = cfg();
        let dir = cfg.remote_project_dir();
        let cmds = push_dir(&cfg, Path::new("/repo/vagrant"), &dir, &archive());
        let quoted = quote_remote_path(&dir);
        assert!(cmds[2].args[1].contains(&format!("cd {quoted} &&")));
        assert!(
            vagrant(&cfg, &["up"]).args[1]
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
        let c = destroy_vm_if_present(&cfg(), "~/vms/myproject");
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
        let c = vagrant_in(&cfg(), "/srv/x", &["halt"]);
        let env = vm_env();
        assert_eq!(c.args[1], format!("cd '/srv/x' && {env} vagrant 'halt'"));
    }

    #[test]
    fn shell_into_vm_forces_a_tty() {
        let c = shell_into_vm(&cfg());
        let env = vm_env();
        assert_eq!(c.args[0], "-t");
        assert_eq!(c.args[1], "vmhost");
        assert_eq!(
            c.args[2],
            format!("cd ~/'vms/myproject' && {env} vagrant 'ssh'")
        );
    }
}
