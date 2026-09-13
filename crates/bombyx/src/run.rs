//! Starting a built command and waiting for it to finish.
//!
//! `remote` builds commands and starts none, which is what lets
//! its quoting and composition be tested without a VM host
//! anywhere near them. This module is the other half: one
//! function that takes a built [`RemoteCommand`] and runs it.
//!
//! It sits in the library rather than in `main` so that the
//! standard-input path below can have tests. The binary's own
//! `execute` wraps this with program resolution, the dry run,
//! and the message printed when a command fails.
//!
//! # Why a command may carry input
//!
//! Every account on a Unix machine can list the commands other
//! accounts are running, arguments included -- that is what
//! `ps` prints. So a file sent as part of a command line is
//! readable by anyone with a login on either machine, for as
//! long as the command runs. Bytes travelling down a pipe
//! between two processes are not listed anywhere.
//!
//! [`RemoteCommand::stdin`] is how a caller asks for the pipe.
//! `remote::write_file` uses it for the files bombyx generates.

use std::io::{ErrorKind, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

use crate::remote::RemoteCommand;

/// Runs `cmd`, with `program` as the executable to start, and
/// waits for it to finish.
///
/// `program` is passed separately because the caller resolves it
/// through `tool` first: `cmd.program` is a bare name such as
/// `ssh`, and starting a bare name goes back through the
/// operating system's own search, which is what `tool` exists to
/// avoid.
///
/// # Errors
///
/// Returns the error from starting the child, from waiting on
/// it, or from writing the payload into its pipe.
pub fn spawn(
    program: &Path,
    cmd: &RemoteCommand,
) -> std::io::Result<ExitStatus> {
    let mut child = Command::new(program);
    child.args(&cmd.args);
    if let Some(dir) = &cmd.dir {
        child.current_dir(dir);
    }

    let Some(payload) = &cmd.stdin else {
        // With no payload the child keeps bombyx's own three
        // streams. `bombyx shell` opens a terminal session and
        // needs exactly that.
        return child.status();
    };

    child.stdin(Stdio::piped());
    let mut running = child.spawn()?;
    let mut pipe = running.stdin.take().expect("a piped stdin, just asked for");
    let written = pipe.write_all(payload.bytes());
    // Closing the pipe is what produces the end-of-file that a
    // program reading its input waits for. Held open, `cat`
    // never returns and neither does the `wait` below.
    drop(pipe);
    let status = running.wait()?;
    match written {
        // A broken pipe means the child stopped reading, which
        // its exit status describes better than this write does.
        Err(e) if e.kind() != ErrorKind::BrokenPipe => Err(e),
        _ => Ok(status),
    }
}

// These tests start `sh`, which bombyx itself only ever starts
// on Unix: `config::transport` refuses the local route on
// Windows, so `sh -c` is not a command that runs there. The
// whole module is therefore gated, and the pipe handling above
// -- the same std code on every platform -- goes unexercised on
// Windows.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use tempfile::TempDir;

    fn sh(script: &str) -> RemoteCommand {
        RemoteCommand::new("sh", &["-c", script])
    }

    fn shell() -> &'static Path {
        Path::new("/bin/sh")
    }

    #[test]
    fn the_child_reads_the_payload_from_its_input() {
        let dir = TempDir::new().expect("a temporary directory");
        let cmd = sh("cat > out").in_dir(dir.path()).with_stdin(
            b"TOKEN=hunter2\nlines\xffwith a byte no text has\n",
        );
        let status = spawn(shell(), &cmd).expect("sh runs");
        assert!(status.success(), "{status}");
        let got = std::fs::read(dir.path().join("out"))
            .expect("the file sh wrote");
        assert_eq!(
            got,
            b"TOKEN=hunter2\nlines\xffwith a byte no text has\n"
        );
    }

    #[test]
    fn a_child_ignoring_its_input_still_reports_its_status() {
        // Bigger than a pipe's buffer, so the write really
        // does hit the closed end rather than fitting in the
        // kernel and succeeding. Without the broken-pipe arm
        // in `spawn`, this reports a write error and loses
        // the 3.
        let payload = vec![b'x'; 256 * 1024];
        let status = spawn(shell(), &sh("exit 3").with_stdin(&payload))
            .expect("sh runs");
        assert_eq!(status.code(), Some(3), "{status}");
    }

    #[test]
    fn a_command_without_a_payload_runs_unchanged() {
        let status = spawn(shell(), &sh("exit 7")).expect("sh runs");
        assert_eq!(status.code(), Some(7), "{status}");
    }

    #[test]
    fn an_empty_payload_still_closes_the_pipe() {
        // `cat` returns on end-of-file and nothing else, so
        // a payload with no bytes in it is the shortest test
        // that the pipe is closed rather than merely written
        // to.
        let dir = TempDir::new().expect("a temporary directory");
        let cmd = sh("cat > out").in_dir(dir.path()).with_stdin(b"");
        let status = spawn(shell(), &cmd).expect("sh runs");
        assert_eq!(status.into_raw() & 0x7f, 0, "sh was signalled");
        assert_eq!(
            std::fs::read(dir.path().join("out")).expect("the file"),
            b""
        );
    }
}
