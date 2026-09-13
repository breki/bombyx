//! What a command *is*.
//!
//! [`RemoteCommand`] is plain data: it carries a program, its
//! arguments, optionally a directory to run in, and optionally
//! bytes for the child's standard input. It renders itself for a
//! dry run. Nothing here starts anything.
//!
//! That last field is the one a reader misses. A command is not
//! described by its argv alone, so anything consuming a
//! `RemoteCommand` has four fields to honour rather than three.

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
    /// Bytes the child reads on standard input.
    ///
    /// `None` leaves the child bombyx's own standard input,
    /// which is what the terminal session `bombyx shell` opens
    /// needs.
    pub stdin: Option<Stdin>,
}

/// Bytes handed to a child process on standard input.
///
/// Any bytes at all are legal, so the rule this type carries is
/// not about their shape: **nothing renders them.** `Debug` is
/// written by hand to report a length, and there is no
/// `Display` at all.
///
/// That rule matters because the whole reason to use standard
/// input is that a command line is readable by every account on
/// the machine running it. A payload that then reached the
/// screen through a `{:?}` in an error message would undo it.
#[derive(Clone, PartialEq, Eq)]
pub struct Stdin(Vec<u8>);

impl Stdin {
    /// The bytes themselves, for whoever writes them into a pipe.
    ///
    /// Crate-private, so the "nothing renders them" rule above is
    /// something the compiler holds outside this crate rather
    /// than something a caller is asked to respect. `run` is the
    /// one place that needs them.
    #[must_use]
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0
    }

    /// How many bytes there are, which is all any render says.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the payload is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Stdin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Stdin({} bytes)", self.0.len())
    }
}

impl RemoteCommand {
    /// Creates a command from a program and its arguments.
    #[must_use]
    pub fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            dir: None,
            stdin: None,
        }
    }

    /// Sets the directory the command runs in.
    #[must_use]
    pub fn in_dir(mut self, dir: &Path) -> Self {
        self.dir = Some(dir.to_path_buf());
        self
    }

    /// This command with any payload dropped.
    ///
    /// [`Display`](std::fmt::Display) ends a command carrying a
    /// payload with a `#` comment, so anything printed after the
    /// command sits inside that comment. This drops the payload
    /// for those cases: a failure message that continues with an
    /// exit status, and a test pinning the shell rather than the
    /// size.
    #[must_use]
    pub fn without_payload(&self) -> Self {
        Self {
            stdin: None,
            ..self.clone()
        }
    }

    /// Sets the bytes the child reads on standard input.
    ///
    /// Every account on a Unix machine can list the arguments of
    /// a running command, and cannot read what travels down a
    /// pipe between two processes. So this is how bombyx sends a
    /// file whose contents no other account should see.
    #[must_use]
    pub fn with_stdin(mut self, bytes: &[u8]) -> Self {
        self.stdin = Some(Stdin(bytes.to_vec()));
        self
    }
}

/// Renders a command for `--dry-run`.
///
/// The output is genuine shell: an argument is printed bare
/// only when every character is unambiguous, and otherwise
/// double-quoted with `\`, `"`, `$` and backtick escaped. A
/// reader can therefore tell where each argument begins and
/// ends, and pasting the line runs the same program on the same
/// arguments bombyx would have given it.
///
/// A command carrying a [`Stdin`] payload ends in a shell
/// comment saying how many bytes bombyx will send down the pipe.
/// The bytes are never printed, which is the point of putting
/// them there. Pasting such a line therefore runs the command
/// with the terminal as its input rather than that payload, and
/// the comment is what tells the reader so.
impl fmt::Display for RemoteCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(dir) = &self.dir {
            write!(f, "cd {} && ", display_arg(&dir.to_string_lossy()))?;
        }
        f.write_str(&self.program)?;
        for arg in &self.args {
            write!(f, " {}", display_arg(arg))?;
        }
        if let Some(stdin) = &self.stdin {
            write!(f, "  # {} bytes on stdin, not shown", stdin.len())?;
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
    fn display_says_a_payload_is_on_stdin_without_printing_it() {
        let c = RemoteCommand::new("ssh", &["h", "cat > f"])
            .with_stdin(b"SECRET_TOKEN=hunter2\n");
        let shown = c.to_string();
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("21 bytes on stdin"), "{shown}");
    }

    #[test]
    fn debug_does_not_print_the_payload() {
        // `RemoteCommand` derives `Debug`, so a payload held as
        // a plain `Vec<u8>` would print in full from any
        // `{:?}` -- a test failure message, an error context.
        let c =
            RemoteCommand::new("sh", &["-c", "cat > f"]).with_stdin(b"hunter2");
        let shown = format!("{c:?}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("7 bytes"), "{shown}");
    }

    #[test]
    fn a_command_with_no_payload_renders_just_its_argv() {
        let c = RemoteCommand::new("ssh", &["h", "uptime"]);
        assert_eq!(c.to_string(), "ssh h uptime");
    }

    #[test]
    fn display_shows_the_working_directory() {
        let c = RemoteCommand::new("tar", &["-czf", "a.tgz"])
            .in_dir(Path::new("/work"));
        assert_eq!(c.to_string(), "cd /work && tar -czf a.tgz");
    }
}
