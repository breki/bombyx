//! Checking bombyx's preconditions before they cost anything.
//!
//! `bombyx up` changes state before it runs `vagrant`: it
//! creates a directory on the host and writes two generated
//! files into it.
//! So a host that is missing something reports it half-way
//! through. This module models the up-front check instead.
//!
//! Nothing here runs a process. It owns the probe list, the
//! rules for reading a probe's result, the skip cascade and the
//! report -- everything that decides anything. The binary
//! supplies process spawning and nothing else, which matters
//! because `src/bin/` is outside the coverage gate.
//!
//! One rule shaped every probe: **a probe must carry a verdict,
//! not a value.** A check that merely prints something and
//! passes on a zero exit reports the state of the host it was
//! pointed at, not the state it exists to catch --
//! `remote::probe::posix_shell` records the case that taught
//! this. Where a probe cannot decide in the shell, the verdict
//! is applied here.
//!
//! This file holds the shared vocabulary -- what a check is, and
//! what became of it -- and nothing else. Each concern built on
//! it is its own submodule, and they barely touch each other:
//!
//! | Module | Owns |
//! |--------|------|
//! | `probes` | which probes run, in what order, and how a reply is read |
//! | `readonly` | the check that a probe script would not change the host |
//! | `text` | making host-supplied text safe to print, and clipping it |
//! | `local` | the checks that run on this workstation |
//! | `report` | collecting findings and rendering them aligned |
//!
//! Their public names are re-exported here, so callers see one
//! `doctor` interface and the split stays an implementation
//! detail.

mod local;
mod probes;
mod readonly;
mod report;
mod text;

pub use local::local_tool_finding;
pub use probes::{
    HostProbe, Verdict, classify, host_findings, host_probes, probe_commands,
    provider_finding, run_probes,
};
pub use readonly::mutating_token;
pub use report::Report;

/// Where a probe runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// On this workstation.
    Local,
    /// On the VM host, over SSH.
    Host,
}

/// What a probe found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The precondition holds, with a short supporting detail.
    Pass(String),
    /// The precondition does not hold, with the reason.
    Fail(String),
    /// Not run, because a gating probe already failed.
    Skip(String),
}

/// What running one probe produced.
///
/// A struct rather than three positional arguments: `stdout` and
/// `stderr` are both strings and are read differently -- a
/// verdict is applied to stdout, and a failure reason prefers
/// stderr -- so transposing them at a call site would compile
/// and silently invert the diagnosis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    /// Whether the command exited zero.
    pub success: bool,
    /// What it wrote to stdout.
    pub stdout: String,
    /// What it wrote to stderr.
    pub stderr: String,
}

impl ProbeResult {
    /// Builds a result from a finished child process.
    ///
    /// Constructing it here rather than in the binary is what
    /// keeps the field order from being a caller's problem.
    #[must_use]
    pub fn from_output(output: &std::process::Output) -> Self {
        Self {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

/// What asking a local tool for its version produced.
///
/// Named states rather than a nested `Option<Result<_, _>>`.
/// That shape had no name for any of its cases, so the caller's
/// `None` arm covered two unrelated situations -- "this tool has
/// no version flag worth asking" and "it was never resolved" --
/// and only a second argument told them apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionAnswer {
    /// No version was asked for.
    ///
    /// Two ways to arrive here, and only one of them reaches a
    /// verdict. The caller may decide a tool answers nothing
    /// worth printing, which no tool `doctor` checks does
    /// today. Or the tool was never found on `PATH`, and then
    /// [`local_tool_finding`] fails on the missing path without
    /// reading this at all.
    NotAsked,
    /// It ran. Whatever it printed is here, exit status included.
    Answered(ProbeResult),
    /// It was found on the `PATH` but would not start.
    WouldNotStart(String),
}

/// One precondition and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Where the probe ran.
    pub scope: Scope,
    /// Short label, e.g. `vagrant`.
    pub name: String,
    /// What the probe found.
    pub outcome: Outcome,
}

impl Finding {
    /// Builds a finding.
    #[must_use]
    pub fn new(scope: Scope, name: &str, outcome: Outcome) -> Self {
        Self {
            scope,
            name: name.to_owned(),
            outcome,
        }
    }
}

/// The reason reported when a program is not on the `PATH`.
///
/// One phrasing, used by every caller. Two hand-written variants
/// of the same sentence drift, and a report whose wording
/// depends on which code path produced it is harder to grep and
/// harder to trust.
#[must_use]
pub fn not_on_path(name: &str) -> String {
    format!("{name} not found on PATH")
}

/// The reason reported when a program was found but would not
/// start.
///
/// Present-but-unusable is a different fault from absent, and
/// naming it as such is the difference between "install this"
/// and "your install is broken".
#[must_use]
pub fn cannot_run(what: &str, err: &str) -> String {
    format!("cannot run {what}: {err}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Output;

    /// A finished process, with a caller-chosen exit code.
    ///
    /// `ExitStatus` has no public constructor, so the status has
    /// to come from a real process. `sh -c 'exit N'` supplies it
    /// on Unix and `cmd /c exit N` on Windows -- both are always
    /// present, and both take the code as an argument, which a
    /// fixed command such as `true` could not.
    fn output(code: i32, stdout: &str, stderr: &str) -> Output {
        let status = if cfg!(windows) {
            std::process::Command::new("cmd")
                .args(["/c", &format!("exit {code}")])
                .status()
        } else {
            std::process::Command::new("sh")
                .args(["-c", &format!("exit {code}")])
                .status()
        }
        .expect("the shell must be runnable");
        Output {
            status,
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn a_probe_result_carries_both_streams_and_the_verdict() {
        let r = ProbeResult::from_output(&output(0, "out", "err"));
        assert!(r.success);
        assert_eq!(r.stdout, "out");
        assert_eq!(r.stderr, "err");

        let r = ProbeResult::from_output(&output(1, "", "boom"));
        assert!(!r.success);
        assert_eq!(r.stderr, "boom");
    }

    #[test]
    fn a_probe_result_keeps_invalid_utf8_rather_than_failing() {
        // A probe reads whatever the host printed, and a byte
        // sequence that is not UTF-8 must not lose the rest of
        // the line. `from_utf8_lossy` substitutes U+FFFD.
        let mut out = output(0, "", "");
        out.stdout = vec![b'a', 0xff, b'b'];
        let r = ProbeResult::from_output(&out);
        assert_eq!(r.stdout, "a\u{fffd}b");
    }
}
