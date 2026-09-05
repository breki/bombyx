//! Writing the files bombyx generates onto the VM host.
//!
//! bombyx sends the Vagrantfile and the bootstrap script over
//! SSH, and they are the only project files any machine outside
//! the guest holds. Neither comes from the project's repository,
//! so a project cannot supply either of them however it arranges
//! its own directory. See `docs/trust-boundary.md`.
//!
//! Getting a whole file across an SSH connection is done with a
//! shell *heredoc*, which looks like this:
//!
//! ```sh
//! cat > somefile <<'END'
//! ...the file content, however many lines...
//! END
//! ```
//!
//! The shell reads every line until it sees one that is exactly
//! `END`, and feeds all of it to `cat`.
//!
//! The quotes around `'END'` matter more than they look. Without
//! them, the shell would look *inside* the content for things to
//! substitute -- anything with a `$` in it, for instance. A
//! Vagrantfile is Ruby, and Ruby is full of `$` and `#{...}`, so
//! the shell would mangle the file before `vagrant` ever read
//! it. Quoting the delimiter tells the shell to keep its hands
//! off and pass the lines through unchanged.

use super::{Config, RemoteCommand, quote_remote_path};

/// The word a heredoc ends on, before [`delimiter_for`]
/// lengthens it.
///
/// A starting point, not a fixed value: `delimiter_for` appends
/// `_` until no line of the file being sent matches it.
const HEREDOC: &str = "BOMBYX_EOF";

/// Picks an end-word that cannot appear in `contents`.
///
/// Here is the problem. A heredoc ends at the first line equal
/// to its end-word. So if the file we are sending happens to
/// contain a line that is exactly `BOMBYX_EOF`, the shell stops
/// reading there -- and treats everything after it as commands
/// to run. On the VM host. Which is the machine we trust, not
/// the sandbox.
///
/// The fix is to keep adding `_` to the end-word until no line
/// of the file matches it. `BOMBYX_EOF`, then `BOMBYX_EOF_`,
/// and so on. This always terminates, because the file is
/// finite: eventually the word is longer than anything in it.
///
/// Note *where* this rule lives. It would be tempting to rely
/// on config validation refusing such values instead, and to
/// prove it with a test. Both are too far away. This function
/// is public, so a caller can hand it anything at all, and a
/// rule kept in another file is exactly the one somebody
/// forgets when they add a field. Handling it right here
/// cannot be forgotten and cannot fail.
fn delimiter_for(contents: &str) -> String {
    let mut delimiter = HEREDOC.to_owned();
    while contents.lines().any(|line| line.trim() == delimiter) {
        delimiter.push('_');
    }
    delimiter
}

impl RemoteCommand {
    /// Renders this command for `--dry-run`, with the contents
    /// of any heredoc left out.
    ///
    /// `--dry-run` prints the commands bombyx would run without
    /// running them. Two of those commands carry an entire file
    /// each, so printing them as they are gives you about
    /// seventy lines where you cannot tell a line of the file
    /// from the start of the next command. This prints one line
    /// per command instead, saying how many lines it left out.
    ///
    /// **This is the only place a dry run does not show you
    /// exactly what runs**, so it is worth being clear: the
    /// file is still written to the host in full. Only the
    /// *printing* is shortened, and the line tells you so.
    ///
    /// Every other command comes out identical to what
    /// [`Display`](std::fmt::Display) gives you. Shortening any
    /// of those would hide something bombyx actually does.
    ///
    /// It is a method on `RemoteCommand` rather than a
    /// standalone function so that it shows up next to
    /// `Display` in the docs. Two places print a command, and a
    /// standalone function is easy for one of them to miss.
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
        // Count up to the end-word, not to the end of the
        // string. After the end-word comes the end-word's own
        // line, plus whatever quote characters `Display` put
        // around the whole argument. None of that is part of
        // the file, so counting it would overstate the total.
        let elided = shown[body_at..]
            .lines()
            .take_while(|l| l.trim() != delimiter)
            .count();
        format!("{}<<'{delimiter}' ({elided} lines elided)", &shown[..open])
    }
}

/// Builds the `ssh` command that writes `contents` into the
/// file `name`, in the directory `dir`, on the VM host.
///
/// Nothing runs here. Like everything in `remote`, this only
/// builds the command; `main` is what spawns it. That split is
/// what lets the interesting part be unit-tested without a VM
/// host anywhere near it.
///
/// Used for the two files bombyx generates, the Vagrantfile and
/// the bootstrap script.
///
/// You can pass any `contents` at all. The private
/// `delimiter_for` picks an end-word the content cannot
/// contain, so nothing here has to be escaped and no caller has
/// to have checked it first.
#[must_use]
pub fn write_file(
    cfg: &Config,
    dir: &str,
    name: &str,
    contents: &str,
) -> RemoteCommand {
    let path = quote_remote_path(&format!("{dir}/{name}"));
    let delimiter = delimiter_for(contents);
    // A heredoc ends at a line that is *exactly* the end-word.
    // If the file does not end in a newline, the end-word would
    // be stuck onto the back of the last line instead of sitting
    // on its own, and the shell would keep reading forever
    // looking for a match it will never find.
    let newline = if contents.ends_with('\n') { "" } else { "\n" };
    let script = format!(
        "cat > {path} <<'{delimiter}'\n{contents}{newline}{delimiter}\n"
    );
    super::transport(cfg, &script, super::Tty::NoPty)
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
