//! Starting a built command and waiting for it to finish.
//!
//! `remote` builds commands and starts none, which is what lets
//! its quoting and composition be tested without a VM host
//! anywhere near them. This module is the other half, and it has
//! two ways to run one. [`Resolver::execute`] leaves the child
//! bombyx's own streams, so a provisioning run scrolls past as
//! it happens. [`Resolver::output`] collects what the child
//! printed, for a reply bombyx parses rather than shows.
//!
//! It sits in the library rather than in `main` so that the
//! program lookup and the standard-input path can have tests;
//! `src/bin/` is outside the coverage gate. What is left in the
//! binary is the dry run and the message printed when a command
//! fails.
//!
//! # A caller never handles a program path
//!
//! [`Resolver`] looks every program up through `tool`, and then
//! runs commands itself. So no call site holds a resolved path
//! beside a command and has to keep the two in step -- a pairing
//! nothing could have checked. Looking up every program **before
//! any of them runs** is the other half: resolving inside the
//! loop would let a plan change something on the VM host and
//! only then discover that its next program is missing, which is
//! the change-state-then-fail behaviour `doctor` exists to
//! prevent.
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

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Output, Stdio};

use crate::remote::RemoteCommand;
use crate::tool;

/// What stopped a command from running, or from running whole.
///
/// Four variants rather than one `io::Error`, because the fixes
/// differ: install the program, mend the connection, look at the
/// far side. A single message saying "running ssh" leaves the
/// operator to guess which.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The program is not on `PATH`.
    ///
    /// Carries the bare name rather than a message, so the caller
    /// words it. `doctor` says one thing in a report row and
    /// `main` another when a plan stops.
    #[error("{program} is not on PATH")]
    NotOnPath {
        /// The bare program name, as the command asked for it.
        program: String,
    },
    /// The child would not start.
    #[error("could not start {program}")]
    Start {
        /// The bare program name.
        program: String,
        /// What the operating system said.
        #[source]
        cause: std::io::Error,
    },
    /// The payload could not be written into the child's pipe.
    #[error("could not send the payload to {program}")]
    Write {
        /// The bare program name.
        program: String,
        /// What the operating system said.
        #[source]
        cause: std::io::Error,
    },
    /// Waiting for the child failed, or its output could not be
    /// collected.
    #[error("could not wait for {program}")]
    Wait {
        /// The bare program name.
        program: String,
        /// What the operating system said.
        #[source]
        cause: std::io::Error,
    },
}

impl Error {
    /// The bare program name this is about.
    #[must_use]
    pub fn program(&self) -> &str {
        match self {
            Self::NotOnPath { program }
            | Self::Start { program, .. }
            | Self::Write { program, .. }
            | Self::Wait { program, .. } => program,
        }
    }
}

/// Every program a set of commands needs, looked up before any
/// of them runs.
///
/// Build one with [`Resolver::for_commands`], then run commands
/// through it. It holds the resolved paths, so a caller never
/// pairs a path with a command itself.
#[derive(Debug)]
pub struct Resolver(HashMap<String, PathBuf>);

impl Resolver {
    /// Looks up every distinct program `commands` names.
    ///
    /// The lookup goes through [`tool::resolve`], which never
    /// searches the working directory.
    ///
    /// # Errors
    ///
    /// [`Error::NotOnPath`] for the first program that is not
    /// there, so nothing has run by the time a caller learns of
    /// it.
    pub fn for_commands(commands: &[RemoteCommand]) -> Result<Self, Error> {
        let mut found = HashMap::new();
        for cmd in commands {
            if let Entry::Vacant(slot) = found.entry(cmd.program.clone()) {
                let path = tool::resolve(&cmd.program).ok_or_else(|| {
                    Error::NotOnPath {
                        program: cmd.program.clone(),
                    }
                })?;
                slot.insert(path);
            }
        }
        Ok(Self(found))
    }

    /// Looks up the one program `cmd` names.
    ///
    /// # Errors
    ///
    /// As [`Resolver::for_commands`].
    pub fn for_command(cmd: &RemoteCommand) -> Result<Self, Error> {
        Self::for_commands(std::slice::from_ref(cmd))
    }

    /// Runs `cmd` and waits, leaving the child bombyx's own
    /// three streams.
    ///
    /// So the operator watches a provisioning run scroll past,
    /// and `bombyx shell` gets a terminal session. Use
    /// [`Resolver::output`] for a reply bombyx parses.
    ///
    /// Named for what it does rather than after
    /// [`Command::spawn`](std::process::Command::spawn), which
    /// starts a child and returns without waiting.
    ///
    /// # Errors
    ///
    /// [`Error::Start`], [`Error::Write`] or [`Error::Wait`].
    /// A command this resolver was not built from reports
    /// [`Error::NotOnPath`].
    pub fn execute(&self, cmd: &RemoteCommand) -> Result<ExitStatus, Error> {
        let mut child = self.prepared(cmd)?;

        let Some(payload) = &cmd.stdin else {
            return child.status().map_err(|cause| Error::Start {
                program: cmd.program.clone(),
                cause,
            });
        };

        child.stdin(Stdio::piped());
        let (mut running, mut pipe) = started(&cmd.program, child)?;
        let written = pipe.write_all(payload.bytes());
        // Closing the pipe is what produces the end-of-file that
        // a program reading its input waits for. Held open,
        // `cat` never returns and neither does the `wait` below.
        drop(pipe);
        let waited = running.wait();
        answer(&cmd.program, written, waited.as_ref().ok().copied())?;
        waited.map_err(|cause| Error::Wait {
            program: cmd.program.clone(),
            cause,
        })
    }

    /// Runs `cmd` and collects what it printed.
    ///
    /// For a command whose reply bombyx parses: a `doctor`
    /// probe, a `list` status call.
    ///
    /// # Errors
    ///
    /// As [`Resolver::execute`].
    pub fn output(&self, cmd: &RemoteCommand) -> Result<Output, Error> {
        let mut child = self.prepared(cmd)?;
        child.stdout(Stdio::piped()).stderr(Stdio::piped());

        let Some(payload) = &cmd.stdin else {
            return child.output().map_err(|cause| Error::Start {
                program: cmd.program.clone(),
                cause,
            });
        };

        child.stdin(Stdio::piped());
        let (running, mut pipe) = started(&cmd.program, child)?;

        // The write goes on its own thread, and this one waits.
        // Both directions have to move at once: the child's
        // stdout is a pipe too, so a child that prints more than
        // a pipe holds before it finishes reading would block on
        // stdout while bombyx blocked on stdin, and neither
        // would ever move again.
        let bytes = payload.bytes().to_vec();
        let writer = std::thread::spawn(move || {
            let written = pipe.write_all(&bytes);
            // Dropping the handle closes the pipe, which is the
            // end-of-file the child waits for.
            drop(pipe);
            written
        });
        let collected = running.wait_with_output();
        // A panicking writer dropped the pipe part-way, so the
        // child saw a clean end-of-file and may well have
        // exited 0 over half a file. Reporting that as a
        // successful write is the one outcome `answer` exists to
        // prevent, so the panic becomes a write error instead.
        let written = writer.join().unwrap_or_else(|_| {
            Err(std::io::Error::other("the writing thread panicked"))
        });
        let status = collected.as_ref().ok().map(|o| o.status);
        answer(&cmd.program, written, status)?;
        collected.map_err(|cause| Error::Wait {
            program: cmd.program.clone(),
            cause,
        })
    }

    /// The child both entry points start from, before either
    /// says anything about its streams.
    ///
    /// One place reads `dir`, so a second runner cannot forget
    /// it.
    ///
    /// # Errors
    ///
    /// [`Error::NotOnPath`] when this resolver was not built
    /// from a command naming that program.
    fn prepared(&self, cmd: &RemoteCommand) -> Result<Command, Error> {
        let path =
            self.0.get(&cmd.program).ok_or_else(|| Error::NotOnPath {
                program: cmd.program.clone(),
            })?;
        let mut child = Command::new(path);
        child.args(&cmd.args);
        if let Some(dir) = &cmd.dir {
            child.current_dir(dir);
        }
        Ok(child)
    }
}

/// Starts `child` and takes the writing end of its pipe.
///
/// The `else` arm is unreachable after the caller asked for a
/// pipe. It is an error rather than a panic because the
/// signature promises one, and the child is waited on so it is
/// not left running.
fn started(
    program: &str,
    mut child: Command,
) -> Result<(std::process::Child, std::process::ChildStdin), Error> {
    let mut running = child.spawn().map_err(|cause| Error::Start {
        program: program.to_owned(),
        cause,
    })?;
    let Some(pipe) = running.stdin.take() else {
        let _ = running.wait();
        return Err(Error::Write {
            program: program.to_owned(),
            cause: std::io::Error::other("the child stdin was not piped"),
        });
    };
    Ok((running, pipe))
}

/// Decides whether a write failure is the answer to report.
///
/// `status` is how the child ended, and `None` means the wait
/// itself failed.
///
/// A child that stops reading breaks the pipe, so `write_all`
/// fails and the child's own status says why. Reporting the
/// write error then would replace "vagrant exited 1" with
/// "broken pipe", which is the less useful of the two.
///
/// **That swallow needs a failing status to hold it up.** A
/// child that stops reading and then exits 0 leaves a partial
/// file behind, and the next command reads that file. So a
/// broken pipe over a successful status stays an error.
///
/// # Errors
///
/// [`Error::Write`] when the write failure is the one worth
/// reporting.
fn answer(
    program: &str,
    written: std::io::Result<()>,
    status: Option<ExitStatus>,
) -> Result<(), Error> {
    let Err(cause) = written else {
        return Ok(());
    };
    if cause.kind() == ErrorKind::BrokenPipe
        && matches!(status, Some(s) if !s.success())
    {
        return Ok(());
    }
    Err(Error::Write {
        program: program.to_owned(),
        cause,
    })
}

/// These need no shell, so they run on every platform, unlike
/// the module below.
#[cfg(test)]
mod lookup_tests {
    use super::*;

    /// A name no machine has a program for.
    const ABSENT: &str = "bombyx-no-such-program-a7f3";

    #[test]
    fn a_program_that_is_not_on_path_stops_the_lookup() {
        // Before anything runs, which is the point of looking
        // every program up first.
        let cmds = [RemoteCommand::new(ABSENT, &["--version"])];
        let e =
            Resolver::for_commands(&cmds).expect_err("no such program exists");
        assert!(matches!(e, Error::NotOnPath { .. }), "{e:?}");
        assert_eq!(e.program(), ABSENT);
    }

    #[test]
    fn a_command_the_resolver_never_saw_is_not_on_path() {
        // A resolver holds the programs it was built from. Asked
        // for another, it refuses rather than falling back to a
        // bare-name spawn, which would go through the operating
        // system's own search.
        let known = RemoteCommand::new(ABSENT, &[]);
        let resolver = Resolver(HashMap::from([(
            known.program.clone(),
            PathBuf::from("/nowhere"),
        )]));
        let other = RemoteCommand::new("some-other-program", &[]);
        let e = resolver.execute(&other).expect_err("not this resolver's");
        assert!(matches!(e, Error::NotOnPath { .. }), "{e:?}");
        assert_eq!(e.program(), "some-other-program");
    }

    #[test]
    fn a_program_that_will_not_start_reports_start() {
        // A resolved path that is not an executable. The lookup
        // cannot produce one, so the resolver is built by hand;
        // what is being checked is that the failure is named
        // `Start` rather than folded in with the others.
        let dir = tempfile::TempDir::new().expect("a temporary directory");
        let not_a_program = dir.path().join("data.txt");
        std::fs::write(&not_a_program, "not a program").expect("write");
        let cmd = RemoteCommand::new("data.txt", &[]);
        let resolver =
            Resolver(HashMap::from([(cmd.program.clone(), not_a_program)]));
        let e = resolver.execute(&cmd).expect_err("not an executable");
        assert!(matches!(e, Error::Start { .. }), "{e:?}");
        assert_eq!(e.program(), "data.txt");
    }

    #[test]
    fn output_reports_a_program_that_will_not_start() {
        // The same arm on the collecting entry point, which has
        // its own copy of it.
        let dir = tempfile::TempDir::new().expect("a temporary directory");
        let not_a_program = dir.path().join("data.txt");
        std::fs::write(&not_a_program, "not a program").expect("write");
        let cmd = RemoteCommand::new("data.txt", &[]);
        let resolver =
            Resolver(HashMap::from([(cmd.program.clone(), not_a_program)]));
        let e = resolver.output(&cmd).expect_err("not an executable");
        assert!(matches!(e, Error::Start { .. }), "{e:?}");
    }

    #[test]
    fn every_error_names_its_program() {
        // `main` words the message from the name, so a variant
        // that lost it would report a failure about nothing.
        let io = || std::io::Error::other("something");
        let errors = [
            Error::NotOnPath {
                program: "a".to_owned(),
            },
            Error::Start {
                program: "a".to_owned(),
                cause: io(),
            },
            Error::Write {
                program: "a".to_owned(),
                cause: io(),
            },
            Error::Wait {
                program: "a".to_owned(),
                cause: io(),
            },
        ];
        for e in &errors {
            assert_eq!(e.program(), "a", "{e:?}");
            assert!(e.to_string().contains('a'), "{e}");
        }
    }
}

/// The payload decisions, on every platform.
///
/// **These must not be gated to Unix.** The write path runs on
/// Windows too -- `ssh vmhost "cat > file"` is what a Windows
/// workstation uses -- and `std`'s pipe, child and
/// `ChildStdin::write_all` are separate implementations per
/// platform rather than one shared one. So `answer`'s reading of
/// a broken pipe is a claim about each platform separately.
///
/// Each test therefore needs a child that reads standard input
/// and exits with a chosen code, and nothing more shell-shaped
/// than that. `exiting_with` supplies one per platform, the way
/// `doctor`'s own test helper does.
#[cfg(test)]
mod payload_tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// A command that ignores its input and exits with `code`.
    ///
    /// `cmd /c exit N` on Windows and `sh -c 'exit N'` on Unix.
    /// Both shells are always present, and both take the code as
    /// an argument, which a fixed command such as `true` could
    /// not.
    fn exiting_with(code: i32) -> RemoteCommand {
        if cfg!(windows) {
            RemoteCommand::new("cmd", &["/c", &format!("exit {code}")])
        } else {
            RemoteCommand::new("sh", &["-c", &format!("exit {code}")])
        }
    }

    /// Runs `cmd` through a resolver built for it alone.
    fn run(cmd: &RemoteCommand) -> Result<ExitStatus, Error> {
        Resolver::for_command(cmd)?.execute(cmd)
    }

    /// Runs `cmd` on a thread, failing rather than hanging.
    fn finished(cmd: &RemoteCommand) -> Result<ExitStatus, Error> {
        let (tx, rx) = mpsc::channel();
        let owned = cmd.clone();
        std::thread::spawn(move || {
            let _ = tx.send(run(&owned));
        });
        rx.recv_timeout(Duration::from_secs(5))
            .expect("the command finished; a timeout means a stuck pipe")
    }

    /// Bigger than any pipe buffer, so the write really does
    /// reach a closed end rather than fitting in the kernel and
    /// succeeding.
    fn oversized() -> Vec<u8> {
        vec![b'x'; 256 * 1024]
    }

    #[test]
    fn a_child_ignoring_its_input_still_reports_its_status() {
        // Without the broken-pipe arm in `answer`, this reports a
        // write error and loses the 3.
        let status = finished(&exiting_with(3).with_stdin(&oversized()))
            .expect("the shell runs");
        assert_eq!(status.code(), Some(3), "{status}");
    }

    #[test]
    fn a_child_ignoring_its_input_and_succeeding_is_an_error() {
        // The other half of that arm. A child that stops reading
        // and then exits 0 has taken part of the file and
        // reported success, and the next command reads that file.
        let cmd = exiting_with(0).with_stdin(&oversized());
        let e = finished(&cmd)
            .expect_err("a partial write must not report success");
        assert!(matches!(e, Error::Write { .. }), "{e:?}");
    }

    #[test]
    fn a_command_without_a_payload_runs_unchanged() {
        let status = run(&exiting_with(7)).expect("the shell runs");
        assert_eq!(status.code(), Some(7), "{status}");
    }
}

// These tests need a POSIX shell, for `cat` and a redirection.
// What they cover beyond `payload_tests` is the content that
// arrives and the two-way streaming, neither of which has a
// portable spelling worth the contortion.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;
    use tempfile::TempDir;

    fn sh(script: &str) -> RemoteCommand {
        RemoteCommand::new("sh", &["-c", script])
    }

    /// A resolver holding just the programs `cmds` name.
    fn resolver(cmds: &[RemoteCommand]) -> Resolver {
        Resolver::for_commands(cmds).expect("sh is on PATH")
    }

    /// Runs one command through a resolver built for it alone.
    fn run(cmd: &RemoteCommand) -> Result<ExitStatus, Error> {
        Resolver::for_command(cmd)?.execute(cmd)
    }

    /// Runs `cmd`, failing the test rather than hanging when it
    /// does not finish.
    ///
    /// The failure this guards is a real one: with the `drop`
    /// that closes the pipe removed, `cat` waits for an
    /// end-of-file that never arrives and so does the `wait`
    /// behind it. Called directly, such a test does not fail --
    /// it stalls, and takes the whole suite and the CI job with
    /// it. Five seconds is far above what these take (under a
    /// hundredth of a second each) and far below any CI limit.
    fn within_five_seconds(cmd: &RemoteCommand) -> ExitStatus {
        finished(cmd).expect("sh runs")
    }

    /// Runs `cmd` on a thread and fails the test if it does not
    /// finish, whichever way it ended.
    fn finished(cmd: &RemoteCommand) -> Result<ExitStatus, Error> {
        let (tx, rx) = mpsc::channel();
        let owned = cmd.clone();
        std::thread::spawn(move || {
            let _ = tx.send(run(&owned));
        });
        rx.recv_timeout(Duration::from_secs(5)).expect(
            "the command finished; a timeout means the pipe stayed open",
        )
    }

    #[test]
    fn the_child_reads_the_payload_from_its_input() {
        const PAYLOAD: &[u8] =
            b"TOKEN=hunter2\nlines\xffwith a byte no text has\n";
        let dir = TempDir::new().expect("a temporary directory");
        let cmd = sh("cat > out").in_dir(dir.path()).with_stdin(PAYLOAD);
        let status = within_five_seconds(&cmd);
        assert!(status.success(), "{status}");
        let got =
            std::fs::read(dir.path().join("out")).expect("the file sh wrote");
        assert_eq!(got, PAYLOAD);
    }

    #[test]
    fn an_empty_payload_still_closes_the_pipe() {
        // `cat` returns on end-of-file and nothing else, so a
        // payload with no bytes in it is the shortest case that
        // needs the pipe closed rather than merely written to.
        // The deadline is what makes this a test: the file is
        // created empty by the `>` before `cat` runs, so its
        // contents prove nothing on their own.
        let dir = TempDir::new().expect("a temporary directory");
        let cmd = sh("cat > out").in_dir(dir.path()).with_stdin(b"");
        let status = within_five_seconds(&cmd);
        assert!(status.success(), "{status}");
        assert_eq!(
            std::fs::read(dir.path().join("out")).expect("the file"),
            b""
        );
    }

    #[test]
    fn output_collects_what_the_child_printed() {
        let cmd = sh("cat; echo done").with_stdin(b"fed on stdin\n");
        let got = resolver(std::slice::from_ref(&cmd))
            .output(&cmd)
            .expect("sh runs");
        assert!(got.status.success(), "{:?}", got.status);
        assert_eq!(got.stdout, b"fed on stdin\ndone\n");
    }

    #[test]
    fn output_does_not_deadlock_on_a_talkative_child() {
        // The child prints more than a pipe holds before it
        // finishes reading. Written on this thread, bombyx would
        // block on stdin while the child blocks on stdout, and
        // neither would ever move. The deadline is the test.
        let payload = vec![b'x'; 256 * 1024];
        let cmd =
            sh("head -c 200000 /dev/zero | tr '\\0' 'y'; cat > /dev/null")
                .with_stdin(&payload);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx
                .send(Resolver::for_command(&cmd).and_then(|r| r.output(&cmd)));
        });
        let got = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("output finished; a timeout is the deadlock")
            .expect("sh runs");
        assert!(got.status.success(), "{:?}", got.status);
        assert_eq!(got.stdout.len(), 200_000);
    }

    #[test]
    fn output_runs_in_the_commands_directory() {
        // `run_command` in the binary used to build its own
        // child and pass neither `dir` nor `stdin`. Both are
        // read here, so the two runners cannot drift apart.
        let dir = TempDir::new().expect("a temporary directory");
        std::fs::write(dir.path().join("marker"), "here\n").expect("write");
        let cmd = sh("cat marker").in_dir(dir.path());
        let got = resolver(std::slice::from_ref(&cmd))
            .output(&cmd)
            .expect("sh runs");
        assert_eq!(got.stdout, b"here\n");
    }
}
