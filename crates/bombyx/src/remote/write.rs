//! Writing bombyx's generated files onto the VM host.
//!
//! The Vagrantfile and the bootstrap script are not shipped in
//! the push. They are written over SSH afterwards, so the
//! project cannot supply either -- see `docs/trust-boundary.md`.
//!
//! A quoted heredoc carries the payload, which is what stops the
//! host shell expanding anything in it. A Vagrantfile is Ruby,
//! full of `#{...}` and `$`, and an unquoted delimiter would let
//! the host shell evaluate those before vagrant ever read the
//! file.

use super::{Config, RemoteCommand, quote_remote_path};

/// The delimiter a written file's heredoc closes on.
///
/// A starting point rather than a fixed string: see
/// [`delimiter_for`], which lengthens it until the payload
/// cannot contain it.
const HEREDOC: &str = "BOMBYX_EOF";

/// A delimiter no line of `contents` is equal to.
///
/// A heredoc ends at the first line equal to its delimiter, so a
/// payload containing that line would close it early and hand
/// the remainder to the VM host's login shell as commands. That
/// is the trusted machine, not the sandbox.
///
/// An earlier version relied on `Config::validate` refusing
/// control characters, and asserted the invariant in a test over
/// one fixture. Both are the wrong place: `write_file` is
/// public, `Config`'s fields are public, and a guard in another
/// module is exactly what a fifth `[vm]` field would be added
/// without. Lengthening the delimiter cannot fail and needs no
/// cooperation from a caller.
fn delimiter_for(contents: &str) -> String {
    let mut delimiter = HEREDOC.to_owned();
    while contents.lines().any(|line| line.trim() == delimiter) {
        delimiter.push('_');
    }
    delimiter
}

impl RemoteCommand {
    /// Renders this command for a dry run, eliding a heredoc
    /// body.
    ///
    /// Only the file writes carry one, and each carries a whole
    /// file, so printing them in full turns `bombyx --dry-run
    /// up` into roughly seventy lines in which a payload line
    /// cannot be told from the next command. Every other command
    /// renders exactly as [`Display`](std::fmt::Display) would:
    /// eliding part of one would hide what bombyx runs.
    ///
    /// **This is the one place a dry run stops being literal.**
    /// The elided body is written to the host in full; only the
    /// printing of it is dropped, and the line says how much.
    ///
    /// An inherent method rather than a free function so it sits
    /// beside `Display` in the docs. As a free function it was
    /// missed at one of the two call sites that print a command.
    #[must_use]
    pub fn abbreviated(&self) -> String {
        let shown = self.to_string();
        let Some(open) = shown.find("<<'") else {
            return shown;
        };
        let after = open + 3;
        let Some(close) = shown[after..].find('\'') else {
            return shown;
        };
        let delimiter = &shown[after..after + close];
        let body_at = after + close + 2;
        if body_at > shown.len() {
            return shown;
        }
        // Counted to the closing delimiter, not to the end: what
        // follows is the delimiter line and whatever quoting
        // `Display` put around the argument, and neither is part
        // of the file.
        let elided = shown[body_at..]
            .lines()
            .take_while(|l| l.trim() != delimiter)
            .count();
        format!("{}<<'{delimiter}' ({elided} lines elided)", &shown[..open])
    }
}

/// Builds the `ssh` command that writes `contents` to
/// `dir/name` on the VM host.
///
/// Used for the Vagrantfile and the bootstrap script. bombyx
/// generates both and writes them here rather than shipping them
/// in the push, so the project cannot supply either.
///
/// Safe for any payload. The heredoc delimiter is lengthened
/// until no line of `contents` equals it, so nothing needs
/// escaping and no caller has to have validated anything first.
#[must_use]
pub fn write_file(
    cfg: &Config,
    dir: &str,
    name: &str,
    contents: &str,
) -> RemoteCommand {
    let path = quote_remote_path(&format!("{dir}/{name}"));
    let delimiter = delimiter_for(contents);
    // Without a trailing newline the delimiter would land on the
    // same line as the file's last line, and the heredoc would
    // never close.
    let newline = if contents.ends_with('\n') { "" } else { "\n" };
    let script = format!(
        "cat > {path} <<'{delimiter}'\n{contents}{newline}{delimiter}\n"
    );
    RemoteCommand::new("ssh", &[&cfg.host, &script])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::{Tty, vagrant};

    fn cfg() -> Config {
        Config::for_tests()
    }

    #[test]
    fn sends_the_contents_through_a_quoted_heredoc() {
        // Quoted, so the host shell expands nothing in the
        // payload. Unquoted it would evaluate `$(...)` in a
        // Vagrantfile on the VM host.
        let c = write_file(&cfg(), "/srv/x", "Vagrantfile", "a $(id) b\n");
        assert_eq!(c.program, "ssh");
        assert!(c.args[1].contains("<<'BOMBYX_EOF'"), "{}", c.args[1]);
        assert!(c.args[1].contains("a $(id) b"), "{}", c.args[1]);
        assert!(c.args[1].contains("'/srv/x/Vagrantfile'"), "{}", c.args[1]);
    }

    #[test]
    fn keeps_a_tilde_expandable() {
        // Quoting the whole path would create a directory
        // literally named `~`.
        let c = write_file(&cfg(), "~/vms/p", "Vagrantfile", "x\n");
        assert!(c.args[1].contains("~/'vms/p/Vagrantfile'"), "{}", c.args[1]);
    }

    #[test]
    fn a_payload_carrying_the_delimiter_cannot_close_it_early() {
        // The whole family, since this is a guard: the delimiter
        // alone, indented, and the lengthened form that a naive
        // single retry would still collide with.
        for payload in [
            "a\nBOMBYX_EOF\nrm -rf ~\n",
            "a\n   BOMBYX_EOF   \nrm -rf ~\n",
            "a\nBOMBYX_EOF\nBOMBYX_EOF_\nrm -rf ~\n",
        ] {
            let c = write_file(&cfg(), "/srv/x", "f", payload);
            let script = &c.args[1];
            let delimiter = script
                .split_once("<<'")
                .and_then(|(_, rest)| rest.split_once('\''))
                .expect("a heredoc opener")
                .0
                .to_owned();
            assert!(
                !payload.lines().any(|l| l.trim() == delimiter),
                "payload contains the delimiter {delimiter}"
            );
            // And it still closes: exactly one line equals it,
            // the one this function appended.
            let body = script.split_once('\n').unwrap().1;
            assert_eq!(
                body.lines().filter(|l| l.trim() == delimiter).count(),
                1,
                "{script}"
            );
        }
    }

    #[test]
    fn abbreviated_elides_a_heredoc_body_and_counts_it() {
        let c = write_file(&cfg(), "/srv/x", "Vagrantfile", "a\nb\nc\n");
        let shown = c.abbreviated();
        assert!(shown.contains("<<'BOMBYX_EOF'"), "{shown}");
        assert!(shown.contains("3 lines elided"), "{shown}");
        assert!(!shown.contains("\nb\n"), "body still present: {shown}");
        assert_eq!(shown.lines().count(), 1, "{shown}");
    }

    #[test]
    fn abbreviated_reports_a_lengthened_delimiter() {
        // The count has to be read against whichever delimiter
        // was chosen, not against the constant.
        let c = write_file(&cfg(), "/srv/x", "f", "a\nBOMBYX_EOF\nb\n");
        let shown = c.abbreviated();
        assert!(shown.contains("<<'BOMBYX_EOF_'"), "{shown}");
        assert!(shown.contains("3 lines elided"), "{shown}");
    }

    #[test]
    fn abbreviated_leaves_an_ordinary_command_alone() {
        // Every other command is one line already, and eliding
        // part of one would hide what bombyx runs.
        let c = vagrant(&cfg(), &["status"], Tty::NoPty);
        assert_eq!(c.abbreviated(), c.to_string());
    }
}
