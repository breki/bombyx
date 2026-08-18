//! What a command *is*, and where the push archive lives.
//!
//! Both are plain data. [`RemoteCommand`] carries a program, its
//! arguments and optionally a directory, and renders itself for a
//! dry run; nothing here spawns anything. [`PushArchive`] records
//! the one naming rule the push depends on.

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
    /// Used for `tar` and `scp` so they can be given a bare
    /// archive file name -- see [`PushArchive`].
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

/// Where the transient push archive lives on each end.
///
/// The archive is written into `dir`, and `tar` and `scp` are
/// both run *in* `dir` and given the bare `name`. That is
/// deliberate: on Windows an absolute path starts with a
/// drive letter (`C:\Users\...`), and `scp` reads everything
/// before the first colon as a *host name*, so passing the
/// absolute path would make it try to connect to a host
/// called `C`.
///
/// On the VM host the archive lands in the login directory
/// under the same bare name. Keeping the remote target free
/// of directories and metacharacters avoids depending on
/// whether a given `scp` build expands the remote path
/// through a shell (pre-9.0, and `-O`) or over SFTP (9.0+),
/// which quote incompatibly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushArchive {
    /// Local directory holding the archive.
    pub dir: PathBuf,
    /// Archive file name, used unchanged on both ends.
    pub name: String,
}

impl PushArchive {
    /// Builds an archive descriptor in `dir`, named uniquely
    /// for this run.
    ///
    /// `unique` distinguishes concurrent runs: two pushes
    /// sharing one name would race, and one could ship a
    /// different project's tree or delete the other's archive
    /// mid-transfer.
    #[must_use]
    pub fn new(dir: &Path, unique: &str) -> Self {
        Self {
            dir: dir.to_path_buf(),
            name: format!(".bombyx-push-{unique}.tar.gz"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn push_archive_name_is_unique_per_run() {
        let a = PushArchive::new(Path::new("/work"), "1-2");
        let b = PushArchive::new(Path::new("/work"), "3-4");
        assert_ne!(a.name, b.name);
        assert_eq!(a.name, ".bombyx-push-1-2.tar.gz");
    }

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
