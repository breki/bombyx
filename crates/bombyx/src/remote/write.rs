//! Writing the files bombyx generates onto the VM host.
//!
//! Every file bombyx stages on the VM host goes through here.
//! `crate::plan::plan` decides which ones: the Vagrantfile and
//! the bootstrap script on every run, the project's secrets
//! when the config names an `env_file`, and a git credential
//! when it names a `repo_token`. Those are the only project
//! files any machine outside the guest holds. Not one of them
//! comes from the project's repository -- bombyx generates the
//! first two and the last, and the secrets are a file on the
//! operator's own workstation that the config names. So a
//! project cannot supply any of them however it arranges its
//! own directory. See `docs/trust-boundary.md`.
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
/// Used for every staged file whose length a dry run may
/// print. [`write_file_of_hidden_size`] below is the one whose
/// length may not, and it says why.
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
    let path = quote_remote_path(&format!("{dir}/{name}"));
    // `umask` sets the mode a *newly created* file gets, so the
    // contents never exist at a readable mode even briefly. It
    // leaves an existing file alone, and a re-provision writes
    // over one -- `cat >` truncates and does not touch the mode
    // -- so `chmod` is what corrects a file an earlier run left
    // readable. The `&&` keeps the `chmod` from running on a
    // write that failed.
    let script = format!("umask 077; cat > {path} && chmod {FILE_MODE} {path}");
    super::transport(cfg, &script, super::Tty::NoPty).with_stdin(contents)
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
    let path = quote_remote_path(&format!("{dir}/{name}"));
    let script = format!("umask 077; cat > {path} && chmod {FILE_MODE} {path}");
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
        // is what corrects a file an older bombyx left at 0664.
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
