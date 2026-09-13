//! Writing the files bombyx generates onto the VM host.
//!
//! bombyx sends the Vagrantfile and the bootstrap script over
//! SSH, and they are the only project files any machine outside
//! the guest holds. Neither comes from the project's repository,
//! so a project cannot supply either of them however it arranges
//! its own directory. See `docs/trust-boundary.md`.
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
//! **Why the file is not an argument.** `ps` shows every account
//! on a machine the full command line of every running process.
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

/// Builds the command that writes `contents` into the file
/// `name`, in the directory `dir`, on the VM host.
///
/// Nothing runs here. Like everything in `remote`, this only
/// builds the command; `run::Resolver` is what starts it. That
/// split is what lets the interesting part be unit-tested
/// without a VM host anywhere near it.
///
/// Used for the two files bombyx generates, the Vagrantfile and
/// the bootstrap script.
///
/// You can pass any `contents` at all. They travel on the
/// command's standard input rather than in its arguments, so no
/// caller has to have checked or escaped them first.
#[must_use]
pub fn write_file(
    cfg: &Config,
    dir: &str,
    name: &str,
    contents: &str,
) -> RemoteCommand {
    let path = quote_remote_path(&format!("{dir}/{name}"));
    super::transport(cfg, &format!("cat > {path}"), super::Tty::NoPty)
        .with_stdin(contents.as_bytes())
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
        let c = write_file(&cfg(), "/srv/x", "Vagrantfile", "a $(id) b\n");
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
        let c = write_file(&cfg(), "/srv/x", "f", "TOKEN=hunter2\n");
        for arg in &c.args {
            assert!(!arg.contains("hunter2"), "{arg}");
        }
        assert!(!c.to_string().contains("hunter2"), "{c}");
    }

    #[test]
    fn the_command_redirects_into_the_named_file() {
        let c = write_file(&cfg(), "/srv/x", "Vagrantfile", "x\n");
        let script = c.args.last().expect("a script argument");
        assert!(script.contains("cat > '/srv/x/Vagrantfile'"), "{script}");
    }

    #[test]
    fn keeps_a_tilde_expandable() {
        // Quoting the whole path would create a directory
        // literally named `~`.
        let c = write_file(&cfg(), "~/vms/p", "Vagrantfile", "x\n");
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
            let c = write_file(&cfg(), "/srv/x", "f", payload);
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
        let c = write_file(&cfg(), "/srv/x", "Vagrantfile", "a\nb\nc\n");
        let shown = c.to_string();
        assert!(shown.contains("6 bytes on stdin"), "{shown}");
        assert_eq!(shown.lines().count(), 1, "{shown}");
    }
}
