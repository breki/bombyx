//! Writing the files bombyx generates onto the VM host.
//!
//! Every file bombyx stages on the VM host goes through here.
//! `crate::plan::plan` decides which ones: the Vagrantfile, the
//! bootstrap script and the account script on every run, the
//! project's secrets when the config names an `env_file` or
//! `vault.entries`, a git credential when it names a `repo_token`,
//! and the deploy key when it names one. Those are the only project
//! files any machine outside the guest holds. Not one of them comes
//! from the project's repository -- bombyx generates the three
//! scripts and the credential, and the secrets and the key come
//! from the operator's own workstation: the files the config
//! names, or the vault it names. So a project cannot supply any of
//! them however it arranges its own directory. See `docs/trust-boundary.md`.
//!
//! A write in the plan does not always land. Under `up` and
//! `scratch` the plan holds each secret's write, but the VM host's
//! shell skips it when vagrant has already provisioned the machine;
//! `SecretStaging` says when. So a `--dry-run` shows every write
//! that might happen, not every file that will be written.
//!
//! The command that carries a file is as short as it looks:
//!
//! ```sh
//! cat > somefile
//! ```
//!
//! The file itself is not in that command. It goes down the pipe
//! bombyx opens to the child process, which is what
//! `RemoteCommand::with_stdin` asks for and
//! `run::Resolver::execute` supplies.
//!
//! One mechanism serves both routes, and each route reaches the
//! `cat` differently. Running here, `sh -c` is the child, so the
//! pipe goes straight to the shell that runs the `cat`. Over
//! SSH, `ssh` is the child; it forwards whatever it reads on its
//! own standard input to the far side, and the remote `cat`
//! receives it there. Neither route needs a second builder.
//!
//! **Why the file is not an argument.** On any Unix machine,
//! every logged-in account can list the commands other accounts
//! are running, arguments included -- that is what `ps` prints.
//! A file passed as an argument is therefore readable by anyone
//! with a login on the VM host while the write runs, and on the
//! workstation too when the host is remote. A pipe between two
//! processes appears in no such listing. The generated
//! Vagrantfile carries every value from the config's `[env]`
//! table, so this is not hypothetical.
//!
//! Nothing has to be escaped either, which is the second
//! benefit. Bytes on a pipe are bytes: no shell looks inside
//! them for a `$` to substitute or an end-word to stop at.

use super::{Config, RemoteCommand, quote_remote_path};

/// The mode the generated files are left at: readable and
/// writable by their owner, and by nobody else.
///
/// The Vagrantfile carries every value from the project's
/// `[env]` table, and a VM host is a machine other people have
/// accounts on. `077` is the matching umask.
const FILE_MODE: &str = "600";

/// Builds the command that writes `contents` into the file
/// `name`, in the directory `dir`, on the VM host.
///
/// Nothing runs here. Like everything in `remote`, this only
/// builds the command; `run::Resolver` is what starts it. That
/// split is what lets the interesting part be unit-tested
/// without a VM host anywhere near it.
///
/// Used for the generated files, whose length a dry run may print.
/// The staged secrets go through `write_secret` and
/// `write_secret_of_hidden_size`. [`write_file_of_hidden_size`]
/// below is the variant whose length a dry run may not print, and
/// it says why.
///
/// `contents` is bytes rather than text, because one of these
/// files is the project's secrets and a password need not be
/// UTF-8. Any bytes at all are legal: they travel on the
/// command's standard input rather than in its arguments, so no
/// caller has to have checked or escaped them first.
#[must_use]
pub fn write_file(
    cfg: &Config,
    dir: &str,
    name: &str,
    contents: &[u8],
) -> RemoteCommand {
    let script = write_script(dir, name);
    super::transport(cfg, &script, super::Tty::NoPty).with_stdin(contents)
}

/// The shell that writes the file `name` in `dir` from its input.
///
/// `umask` sets the mode a *newly created* file gets, so the
/// contents never exist at a readable mode even briefly. It leaves
/// an existing file alone, and a re-provision writes over one --
/// `cat >` truncates and does not touch the mode -- so `chmod` is
/// what corrects a file an earlier run left readable. The `&&`
/// keeps the `chmod` from running on a write that failed.
fn write_script(dir: &str, name: &str) -> String {
    let path = quote_remote_path(&format!("{dir}/{name}"));
    format!("umask 077; cat > {path} && chmod {FILE_MODE} {path}")
}

/// When a secret-carrying file is staged for a `vagrant` run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretStaging {
    /// Every run. `vagrant provision` always provisions, so it
    /// always reads the file.
    Always,
    /// Only while vagrant has not provisioned the machine.
    ///
    /// Vagrant writes its provision marker
    /// (`remote::provision_marker`) after the boot and before its
    /// provisioners run, and `vagrant up` provisions only a machine
    /// without one: a machine it creates, or one whose first `up`
    /// stopped before the boot finished. A provision that started
    /// and failed leaves the marker, so only `vagrant provision`
    /// runs it again. A marker whose machine is gone is wiped when
    /// vagrant loads the machine, which `remote::load_machine` has
    /// it do first. With the marker there, `up` boots without
    /// provisioning and nothing reads a staged secret, so nothing
    /// is written. Only the VM host has the marker, so the test runs
    /// there.
    UnlessProvisioned,
}

/// The shell that writes the secret `name` in `dir` from its input,
/// as `staging` says.
///
/// Under `UnlessProvisioned` the marker test runs in the same shell
/// that would write, and a present marker makes the shell read its
/// input into `/dev/null` rather than leave it unread: the bytes
/// still have to be read, or the writer on the other end of the
/// pipe fails.
fn secret_script(
    cfg: &Config,
    dir: &str,
    name: &str,
    staging: SecretStaging,
) -> String {
    let write = write_script(dir, name);
    match staging {
        SecretStaging::Always => write,
        SecretStaging::UnlessProvisioned => {
            let marker = quote_remote_path(&format!(
                "{dir}/{}",
                super::provision_marker(cfg.vm.provider)
            ));
            format!(
                "if [ -e {marker} ]; then cat > /dev/null; else {write}; fi"
            )
        }
    }
}

/// [`write_file`] for a secret-carrying file, staged as `staging`
/// says.
#[must_use]
pub fn write_secret(
    cfg: &Config,
    dir: &str,
    name: &str,
    contents: &[u8],
    staging: SecretStaging,
) -> RemoteCommand {
    let script = secret_script(cfg, dir, name, staging);
    super::transport(cfg, &script, super::Tty::NoPty).with_stdin(contents)
}

/// [`write_secret`] for a file whose size a dry run must not print,
/// as [`write_file_of_hidden_size`] is to [`write_file`].
///
/// It carries the git credential: `https://` plus a username, a host
/// and two separators the reader already has, so a byte count would
/// measure the token.
#[must_use]
pub fn write_secret_of_hidden_size(
    cfg: &Config,
    dir: &str,
    name: &str,
    contents: &[u8],
    staging: SecretStaging,
) -> RemoteCommand {
    let script = secret_script(cfg, dir, name, staging);
    super::transport(cfg, &script, super::Tty::NoPty)
        .with_stdin_of_hidden_size(contents)
}

/// [`write_file`] for a file whose size a dry run must not
/// print.
///
/// The same command, so the file gets the same `umask` and the
/// same mode. The dry run is the whole difference: its line
/// says the contents are not shown and gives no count, because
/// this file's length is a measurement of one secret.
/// `crate::remote::Stdin` says which payloads those are.
#[must_use]
pub fn write_file_of_hidden_size(
    cfg: &Config,
    dir: &str,
    name: &str,
    contents: &[u8],
) -> RemoteCommand {
    let script = write_script(dir, name);
    super::transport(cfg, &script, super::Tty::NoPty)
        .with_stdin_of_hidden_size(contents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::Stdin;

    fn cfg() -> Config {
        Config::for_tests()
    }

    #[test]
    fn the_contents_travel_on_standard_input() {
        let c = write_file(&cfg(), "/srv/x", "Vagrantfile", b"a $(id) b\n");
        assert_eq!(c.program, "ssh");
        assert_eq!(
            c.stdin.as_ref().map(Stdin::bytes),
            Some(&b"a $(id) b\n"[..])
        );
    }

    #[test]
    fn no_argument_holds_any_of_the_contents() {
        // The whole point of the pipe: a secret in a file bombyx
        // sends must not reach a command line, where `ps` shows
        // it to every account on either machine.
        let c = write_file(&cfg(), "/srv/x", "f", b"TOKEN=hunter2\n");
        for arg in &c.args {
            assert!(!arg.contains("hunter2"), "{arg}");
        }
        assert!(!c.to_string().contains("hunter2"), "{c}");
    }

    #[test]
    fn the_file_is_created_private_and_an_existing_one_is_fixed() {
        // Two halves, and each covers what the other cannot.
        // `umask` decides the mode of a file being created, so
        // the contents never exist at a readable mode even for
        // an instant. It does nothing to a file that is already
        // there, and a re-provision writes over one -- `cat >`
        // truncates without touching the mode -- so the `chmod`
        // is what corrects a file an earlier run left at 0664.
        let c = write_file(&cfg(), "/srv/x", "Vagrantfile", b"x\n");
        let script = c.args.last().expect("a script argument");
        assert!(script.contains("umask 077"), "{script}");
        assert!(
            script.contains("chmod 600 '/srv/x/Vagrantfile'"),
            "{script}"
        );
        // The chmod must not run when the write failed: a
        // half-written file left readable is the case being
        // closed.
        assert!(script.contains("&& chmod"), "{script}");
    }

    #[test]
    fn the_command_redirects_into_the_named_file() {
        let c = write_file(&cfg(), "/srv/x", "Vagrantfile", b"x\n");
        let script = c.args.last().expect("a script argument");
        assert!(script.contains("cat > '/srv/x/Vagrantfile'"), "{script}");
    }

    #[test]
    fn a_guarded_write_tests_the_provision_marker_and_drains_otherwise() {
        // The marker sits in the machine's data directory under the
        // provider's name. When it is there the input is read into
        // `/dev/null`, so the writer's pipe never breaks; when it is
        // not, the ordinary private write runs.
        let c = write_secret(
            &cfg(),
            "~/vms/p",
            "bombyx.env",
            b"TOKEN=hunter2\n",
            SecretStaging::UnlessProvisioned,
        );
        let script = c.args.last().expect("a script argument");
        assert!(
            script.ends_with(
                "if [ -e ~/'vms/p/.vagrant/machines/default/libvirt/\
                 action_provision' ]; then cat > /dev/null; else umask 077; \
                 cat > ~/'vms/p/bombyx.env' && chmod 600 ~/'vms/p/bombyx.env'; \
                 fi"
            ),
            "{script}"
        );
        assert_eq!(
            c.stdin.as_ref().map(Stdin::bytes),
            Some(&b"TOKEN=hunter2\n"[..])
        );
        assert!(!c.to_string().contains("hunter2"), "{c}");
    }

    #[test]
    fn a_guarded_write_of_hidden_size_still_hides_it() {
        let c = write_secret_of_hidden_size(
            &cfg(),
            "~/vms/p",
            "bombyx.git-credentials",
            b"https://u:tok@host\n",
            SecretStaging::UnlessProvisioned,
        );
        let shown = c.to_string();
        assert!(shown.contains("action_provision"), "{shown}");
        assert!(!shown.contains("tok"), "{shown}");
        assert!(shown.contains("not shown"), "{shown}");
    }

    #[test]
    fn keeps_a_tilde_expandable() {
        // Quoting the whole path would create a directory
        // literally named `~`.
        let c = write_file(&cfg(), "~/vms/p", "Vagrantfile", b"x\n");
        assert!(c.args[1].contains("~/'vms/p/Vagrantfile'"), "{}", c.args[1]);
    }

    #[test]
    fn a_payload_needs_no_escaping_of_any_shape() {
        // The shapes a shell would otherwise act on: a word
        // that could end a redirection, a last line with no
        // newline, a `$` the far shell would substitute, and a
        // NUL no command line can hold at all. Each arrives
        // byte for byte.
        for payload in [
            "BOMBYX_EOF\nrm -rf ~\n",
            "no trailing newline",
            "$(id) `id` ${HOME}\n",
            "a\0b\n",
        ] {
            let c = write_file(&cfg(), "/srv/x", "f", payload.as_bytes());
            assert_eq!(
                c.stdin.as_ref().map(Stdin::bytes),
                Some(payload.as_bytes()),
                "{payload:?}"
            );
        }
    }

    #[test]
    fn the_plan_line_says_how_much_it_is_not_showing() {
        // `--dry-run` prints this. It cannot print the file, so
        // it says the size instead.
        let c = write_file(&cfg(), "/srv/x", "Vagrantfile", b"a\nb\nc\n");
        let shown = c.to_string();
        assert!(shown.contains("6 bytes on stdin"), "{shown}");
        assert_eq!(shown.lines().count(), 1, "{shown}");
    }
}
