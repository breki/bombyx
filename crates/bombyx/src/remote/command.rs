//! What a command *is*.
//!
//! [`RemoteCommand`] is plain data: it carries a program, its
//! arguments and optionally a directory, and renders itself for a
//! dry run. Nothing here spawns anything.

use std::fmt;
use std::path::{Path, PathBuf};

use super::quote::display_arg;

/// A command to execute: a program, its arguments, and
/// optionally the directory to run it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCommand {
    /// Program to run, e.g. `ssh`.
    pub program: String,
    /// Arguments passed to the program.
    pub args: Vec<String>,
    /// Directory to run the program in.
    ///
    /// `bombyx self-update` sets it, so the commands that
    /// unpack a release run in the download directory and can
    /// be given bare file names.
    pub dir: Option<PathBuf>,
}

impl RemoteCommand {
    /// Creates a command from a program and its arguments.
    #[must_use]
    pub fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            dir: None,
        }
    }

    /// Sets the directory the command runs in.
    #[must_use]
    pub fn in_dir(mut self, dir: &Path) -> Self {
        self.dir = Some(dir.to_path_buf());
        self
    }
}

/// Renders a command for `--dry-run`.
///
/// The output is genuine shell: an argument is printed bare
/// only when every character is unambiguous, and otherwise
/// double-quoted with `\`, `"`, `$` and backtick escaped. A
/// reader can therefore tell where each argument begins and
/// ends, and pasting the line runs the same thing bombyx
/// would have run.
impl fmt::Display for RemoteCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(dir) = &self.dir {
            write!(f, "cd {} && ", display_arg(&dir.to_string_lossy()))?;
        }
        f.write_str(&self.program)?;
        for arg in &self.args {
            write!(f, " {}", display_arg(arg))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn displays_a_plain_command_unquoted() {
        let c = RemoteCommand::new("scp", &["a.tgz", "vmhost:a.tgz"]);
        assert_eq!(c.to_string(), "scp a.tgz vmhost:a.tgz");
    }

    #[test]
    fn displays_a_spaced_argument_quoted() {
        let c = RemoteCommand::new("ssh", &["vmhost", "cd x && vagrant up"]);
        assert_eq!(c.to_string(), "ssh vmhost \"cd x && vagrant up\"");
    }

    #[test]
    fn display_escapes_what_a_shell_would_expand() {
        // A dry run is the review step, so its output must
        // not read as something other than what will run.
        let c = RemoteCommand::new("ssh", &["h", "a $(id) `id` \"q\" \\"]);
        assert_eq!(c.to_string(), r#"ssh h "a \$(id) \`id\` \"q\" \\""#);
    }

    #[test]
    fn display_quotes_an_empty_argument() {
        let c = RemoteCommand::new("ssh", &[""]);
        assert_eq!(c.to_string(), "ssh \"\"");
    }

    #[test]
    fn display_shows_the_working_directory() {
        let c = RemoteCommand::new("tar", &["-czf", "a.tgz"])
            .in_dir(Path::new("/work"));
        assert_eq!(c.to_string(), "cd /work && tar -czf a.tgz");
    }
}
