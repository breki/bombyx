//! Whether `destroy` may go ahead, and the question it asks first.
//!
//! A decision rather than a process, so it lives in the library
//! where the coverage gate reaches it. The binary reads whether
//! stdin and stderr are terminals and hands in both answers, then
//! passes stdin and stderr as the reader and writer; a test hands
//! in its own.

use std::io::{self, BufRead, Write};

use thiserror::Error;

use crate::name::ProjectName;

/// The two `destroy` flags that bear on consent, named so that a
/// call cannot pass them in the wrong order.
#[derive(Debug, Clone, Copy, Default)]
pub struct DestroyFlags {
    /// `--yes`: destroy without asking.
    pub yes: bool,
    /// `--dry-run`: print the plan and destroy nothing.
    pub dry_run: bool,
}

/// Which of the two streams the prompt uses are terminals.
///
/// Both have to be. The question is written to stderr and the
/// answer read from stdin, so a terminal on stdin alone would put
/// the target and the question in a redirected file while bombyx
/// waited on the keyboard.
#[derive(Debug, Clone, Copy)]
pub struct Terminals {
    /// Whether stdin, where the answer is read, is a terminal.
    pub stdin: bool,
    /// Whether stderr, where the question is written, is a
    /// terminal.
    pub stderr: bool,
}

/// How a `destroy` run gets the operator's go-ahead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consent {
    /// A dry run, which destroys nothing, so there is nothing to
    /// ask.
    DryRun,
    /// `--yes`: the operator consented on the command line.
    Yes,
    /// stdin and stderr are both terminals, so somebody can read
    /// the target and type the project name.
    Ask,
    /// stdin or stderr is not a terminal and `--yes` was not
    /// passed, so nobody can be asked.
    NoTerminal,
}

impl Consent {
    /// The consent a `destroy` run has, from its flags and from
    /// which of its streams are terminals.
    ///
    /// When stdin is a pipe, bombyx refuses rather than reading
    /// the answer from it. A script that would pipe the name in can
    /// pass `--yes` instead, which says the same thing plainly, and
    /// piped text proves nobody read the target. [`Terminals`]
    /// says why stderr counts too.
    #[must_use]
    pub fn of(flags: DestroyFlags, terminals: Terminals) -> Self {
        if flags.dry_run {
            Self::DryRun
        } else if flags.yes {
            Self::Yes
        } else if terminals.stdin && terminals.stderr {
            Self::Ask
        } else {
            Self::NoTerminal
        }
    }
}

/// Why [`confirm_destroy`] refused.
#[derive(Debug, Error)]
pub enum Refusal {
    /// Nobody can be asked the question.
    #[error(
        "destroy asks you to type the project name, which needs a \
         terminal on both stdin and stderr: pass --yes to destroy \
         {target} without asking"
    )]
    NoTerminal {
        /// The `<host>:<dir>` that was not destroyed.
        target: String,
    },
    /// stdin closed before a line arrived.
    #[error("no answer; refusing to destroy {target}")]
    NoAnswer {
        /// The `<host>:<dir>` that was not destroyed.
        target: String,
    },
    /// The typed name is not the project's.
    #[error(
        "{answer:?} does not match the project being destroyed \
         ({project:?}); refusing to destroy {target}"
    )]
    Mismatch {
        /// What the operator typed, trimmed.
        answer: String,
        /// The project being destroyed.
        project: String,
        /// The `<host>:<dir>` that was not destroyed.
        target: String,
    },
    /// Reading the answer or writing the prompt failed.
    #[error("could not ask for confirmation: {0}")]
    Io(#[from] io::Error),
}

/// Prints the `<host>:<dir>` about to be destroyed and, when
/// `consent` says to, asks for `project` to be typed back.
///
/// `Consent::NoTerminal` prints nothing: the target reaches the
/// operator only through the `Refusal` message.
///
/// The target is the part worth reading. The project name came
/// from the command line a moment earlier, so typing it again
/// proves little on its own; what the operator can check against
/// reality is which machine and which directory the config
/// resolved to, and the prompt shows that before it asks.
///
/// # Errors
///
/// Returns a [`Refusal`] when nobody can answer, when no answer
/// arrives, when the answer is not `project`, or when the prompt
/// cannot be written or the answer read.
pub fn confirm_destroy(
    project: &ProjectName,
    target: &str,
    consent: Consent,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> Result<(), Refusal> {
    let target = target.to_owned();
    match consent {
        Consent::DryRun => {
            writeln!(out, "bombyx: would destroy {target}")?;
            return Ok(());
        }
        Consent::Yes => {}
        Consent::NoTerminal => return Err(Refusal::NoTerminal { target }),
        Consent::Ask => {
            write!(
                out,
                "bombyx: this destroys {target}\n\
                 type the project name to confirm: "
            )?;
            out.flush()?;
            let mut answer = String::new();
            if input.read_line(&mut answer)? == 0 {
                return Err(Refusal::NoAnswer { target });
            }
            // `trim`, so a line ending of `\r\n` from a Windows
            // console compares equal.
            let answer = answer.trim();
            if answer != project.as_str() {
                return Err(Refusal::Mismatch {
                    answer: answer.to_owned(),
                    project: project.as_str().to_owned(),
                    target,
                });
            }
        }
    }
    writeln!(out, "bombyx: destroying {target}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Consent, DestroyFlags, Refusal, Terminals, confirm_destroy};
    use crate::name::ProjectName;

    const TARGET: &str = "vmhost:~/vms/myproject";

    /// Runs [`confirm_destroy`] with `typed` as stdin, returning
    /// the result and everything written to stderr.
    fn run(consent: Consent, typed: &str) -> (Result<(), Refusal>, String) {
        let project = ProjectName::parse("myproject").unwrap();
        let mut out = Vec::new();
        let result = confirm_destroy(
            &project,
            TARGET,
            consent,
            &mut typed.as_bytes(),
            &mut out,
        );
        (result, String::from_utf8(out).unwrap())
    }

    fn flags(yes: bool, dry_run: bool) -> DestroyFlags {
        DestroyFlags { yes, dry_run }
    }

    fn terminals(stdin: bool, stderr: bool) -> Terminals {
        Terminals { stdin, stderr }
    }

    #[test]
    fn a_dry_run_wins_then_yes_then_a_terminal() {
        for (stdin, stderr) in
            [(true, true), (true, false), (false, true), (false, false)]
        {
            let t = terminals(stdin, stderr);
            assert_eq!(Consent::of(flags(true, true), t), Consent::DryRun);
            assert_eq!(Consent::of(flags(false, true), t), Consent::DryRun);
            assert_eq!(Consent::of(flags(true, false), t), Consent::Yes);
        }
        assert_eq!(
            Consent::of(flags(false, false), terminals(true, true)),
            Consent::Ask
        );
    }

    #[test]
    fn asking_needs_a_terminal_on_both_stdin_and_stderr() {
        // With stderr redirected, the target and the question would
        // land in the file while bombyx waited on the keyboard, so
        // an operator could confirm a target they never saw.
        for (stdin, stderr) in [(true, false), (false, true), (false, false)] {
            assert_eq!(
                Consent::of(flags(false, false), terminals(stdin, stderr)),
                Consent::NoTerminal,
                "stdin={stdin} stderr={stderr}"
            );
        }
    }

    #[test]
    fn the_prompt_shows_the_target_before_it_asks() {
        let (result, out) = run(Consent::Ask, "myproject\n");
        result.unwrap();
        let asked = out.find("type the project name").unwrap();
        assert!(out[..asked].contains(TARGET), "{out}");
        assert!(out.ends_with(&format!("destroying {TARGET}\n")), "{out}");
    }

    #[test]
    fn a_windows_line_ending_still_matches() {
        run(Consent::Ask, "myproject\r\n").0.unwrap();
    }

    #[test]
    fn a_wrong_name_refuses_and_names_the_target() {
        let (result, out) = run(Consent::Ask, "other\n");
        let err = result.unwrap_err();
        assert!(matches!(err, Refusal::Mismatch { .. }), "{err:?}");
        assert!(err.to_string().contains(TARGET), "{err}");
        assert!(!out.contains("destroying"), "{out}");
    }

    #[test]
    fn no_answer_refuses() {
        let err = run(Consent::Ask, "").0.unwrap_err();
        assert!(matches!(err, Refusal::NoAnswer { .. }), "{err:?}");
    }

    #[test]
    fn no_terminal_says_to_pass_yes_and_prints_nothing() {
        let (result, out) = run(Consent::NoTerminal, "myproject\n");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--yes") && err.contains(TARGET), "{err}");
        assert!(out.is_empty(), "nothing is printed or asked: {out:?}");
    }

    #[test]
    fn yes_asks_nothing_and_says_it_is_destroying() {
        let (result, out) = run(Consent::Yes, "");
        result.unwrap();
        assert_eq!(out, format!("bombyx: destroying {TARGET}\n"));
    }

    #[test]
    fn a_dry_run_asks_nothing_and_says_it_would_destroy() {
        let (result, out) = run(Consent::DryRun, "");
        result.unwrap();
        assert_eq!(out, format!("bombyx: would destroy {TARGET}\n"));
    }
}
