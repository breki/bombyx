//! Helpers for invoking `ssh`/`scp` without a shell in
//! between.
//!
//! Each helper uses `std::process::Command` with an
//! explicit arg vector — no shell parsing, no path
//! conversion quirks (the `MSYS_NO_PATHCONV` dance that
//! bash deploy scripts traditionally had to do).

use std::fmt;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug)]
pub enum RemoteError {
    /// Process spawn failed (e.g. ssh binary missing).
    /// `Display` renders the inner io message.
    Spawn(io::Error),
    /// `Child::wait` failed.
    Wait(io::Error),
    /// Writing the bash script to ssh's stdin failed.
    StdinWrite(io::Error),
    /// The spawned child had no stdin handle despite
    /// `Stdio::piped()`.
    MissingStdin,
    /// The remote process exited non-zero. `cmd` is a
    /// caller-provided label identifying which call
    /// site failed.
    NonZeroExit { cmd: String, code: Option<i32> },
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemoteError::Spawn(e) => write!(f, "failed to spawn: {e}"),
            RemoteError::Wait(e) => write!(f, "wait failed: {e}"),
            RemoteError::StdinWrite(e) => {
                write!(f, "failed to write to ssh stdin: {e}")
            }
            RemoteError::MissingStdin => {
                write!(f, "ssh child process has no stdin handle")
            }
            RemoteError::NonZeroExit { cmd, code } => match code {
                Some(c) => write!(f, "{cmd} exited with status {c}"),
                None => write!(f, "{cmd} terminated by signal"),
            },
        }
    }
}

/// Run `ssh <remote> <cmd>` and stream output through.
/// `label` is included in the error message on non-zero
/// exit so the caller can disambiguate which step failed.
pub fn ssh_run(
    remote: &str,
    cmd: &str,
    label: &str,
) -> Result<(), RemoteError> {
    let status = Command::new("ssh")
        .arg(remote)
        .arg(cmd)
        .status()
        .map_err(RemoteError::Spawn)?;
    if status.success() {
        Ok(())
    } else {
        Err(RemoteError::NonZeroExit {
            cmd: format!("ssh ({label})"),
            code: status.code(),
        })
    }
}

/// Run `ssh <remote> <cmd>` and capture stdout.
pub fn ssh_capture(
    remote: &str,
    cmd: &str,
    label: &str,
) -> Result<String, RemoteError> {
    let out = Command::new("ssh")
        .arg(remote)
        .arg(cmd)
        .output()
        .map_err(RemoteError::Spawn)?;
    if !out.status.success() {
        return Err(RemoteError::NonZeroExit {
            cmd: format!("ssh ({label})"),
            code: out.status.code(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Pipe a bash script into `ssh <remote> bash -s -- args…`.
///
/// The script runs in a remote bash; its `$1`, `$2`, etc.
/// resolve to the values in `args`.
pub fn ssh_bash(
    remote: &str,
    script: &str,
    args: &[&str],
    label: &str,
) -> Result<(), RemoteError> {
    let mut cmd = Command::new("ssh");
    cmd.arg(remote).arg("bash").arg("-s").arg("--");
    for a in args {
        cmd.arg(a);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .spawn()
        .map_err(RemoteError::Spawn)?;

    {
        let stdin = child.stdin.as_mut().ok_or(RemoteError::MissingStdin)?;
        stdin
            .write_all(script.as_bytes())
            .map_err(RemoteError::StdinWrite)?;
    }

    let status = child.wait().map_err(RemoteError::Wait)?;
    if status.success() {
        Ok(())
    } else {
        Err(RemoteError::NonZeroExit {
            cmd: format!("ssh bash -s ({label})"),
            code: status.code(),
        })
    }
}

/// `scp <local> <remote>:<dest>`.
///
/// `local` is passed as a relative name; `cwd` is the
/// directory we run scp from. This sidesteps the Windows
/// drive-letter parsing issue where `scp D:\foo bar:dst`
/// would be interpreted as three remote hosts.
///
/// The `--` separator is passed before the positional
/// args so a `local` starting with `-` is not mistaken
/// for a scp flag.
pub fn scp_to(
    remote: &str,
    local: &str,
    remote_dest: &str,
    cwd: &Path,
    label: &str,
) -> Result<(), RemoteError> {
    let target = format!("{remote}:{remote_dest}");
    let status = Command::new("scp")
        .arg("-r")
        .arg("--")
        .arg(local)
        .arg(&target)
        .current_dir(cwd)
        .status()
        .map_err(RemoteError::Spawn)?;
    if status.success() {
        Ok(())
    } else {
        Err(RemoteError::NonZeroExit {
            cmd: format!("scp ({label})"),
            code: status.code(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonzero_exit_includes_label() {
        let e = RemoteError::NonZeroExit {
            cmd: "ssh (stop service)".into(),
            code: Some(42),
        };
        assert_eq!(format!("{e}"), "ssh (stop service) exited with status 42");
    }

    #[test]
    fn nonzero_exit_signal() {
        let e = RemoteError::NonZeroExit {
            cmd: "scp (frontend)".into(),
            code: None,
        };
        assert_eq!(format!("{e}"), "scp (frontend) terminated by signal");
    }

    #[test]
    fn spawn_error_displays_inner() {
        let io = io::Error::new(io::ErrorKind::NotFound, "ssh not found");
        let e = RemoteError::Spawn(io);
        assert!(format!("{e}").contains("ssh not found"));
    }

    #[test]
    fn missing_stdin_display() {
        let e = RemoteError::MissingStdin;
        assert_eq!(format!("{e}"), "ssh child process has no stdin handle");
    }
}
