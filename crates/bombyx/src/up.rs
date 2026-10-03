//! The order of `bombyx up`'s steps, and when a failed step stops
//! the run.
//!
//! `up` probes the machine, then boots it, refreshes the guest's
//! secrets and takes the `fresh-install` snapshot -- or does part of
//! that, or nothing -- depending on the state the probe found.
//! [`up_plan`] decides which steps run, in what order, and what note
//! `up` prints first; [`run_up_steps`] runs them and decides whether
//! the run goes on after a step fails. The binary's `up_run` only
//! prints the note and supplies the commands. Both decisions live
//! here, in the tested library, because `src/bin` is outside the
//! coverage gate, so a change to either fails a test rather than
//! waiting for a real run to show it.

use crate::listing::{
    VmState, refreshes_secrets_after_up, takes_fresh_snapshot,
};

/// A note `bombyx up` prints before its steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpNote {
    /// The machine is already running and there is nothing to
    /// refresh, so `up` does nothing.
    AlreadyUp,
    /// The machine is already running, so `up` only refreshes its
    /// secrets.
    RefreshingRunning,
    /// The probe could not confirm whether the machine is running,
    /// and `up` boots anyway: a probe bombyx cannot complete must not
    /// block the boot, but a running machine would be re-staged --
    /// its generated files and secrets written onto the VM host
    /// again -- and re-snapshotted, so the operator is told.
    Unconfirmed,
}

/// One step of `bombyx up` that runs commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpStep {
    /// Boot the machine.
    Boot,
    /// Rewrite the guest's secrets in full: what a guest needs when no
    /// provision ran. A configured hook runs with them, but only when
    /// a secrets file is staged, because it travels with that file.
    RefreshSecrets,
    /// Run only what follows a provision, since the provision already
    /// wrote the secrets: the hook, when one is configured and a
    /// secrets file is staged. With neither, the step runs nothing.
    RefreshAfterProvisioning,
    /// Take the `fresh-install` snapshot.
    Snapshot,
}

/// What `bombyx up` does for one probed state: a note to print,
/// then the steps to run, in order. No steps means nothing to do,
/// and a step may still run no commands when the project has
/// nothing for it to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpPlan {
    /// The note printed before the steps, if any.
    pub note: Option<UpNote>,
    /// The steps, in the order they run.
    pub steps: Vec<UpStep>,
}

/// What `bombyx up` does for the machine state the probe found.
///
/// `state` is the probe's answer, `None` when it returned no row.
/// `has_refresh` is whether `plan::refresh_secrets` produced any
/// command, which it does when a credential, a deploy key or a
/// secrets file was staged. A `secrets_refreshed` hook travels with
/// the secrets file, so a hook alone does not count. It matters only
/// for a running machine: the boot path lists its refresh step
/// either way, and that step runs no commands when there is nothing
/// to send.
///
/// - **Running:** nothing to boot. With something to refresh, the
///   secrets are refreshed; without, nothing runs.
/// - **Otherwise:** an unconfirmed state gets a note, then the
///   machine boots. The refresh after it is the full one unless this
///   `up` created the machine, because vagrant provisions a machine
///   it creates ([`refreshes_secrets_after_up`]). The snapshot comes
///   last, after the refresh so a `reset` returns to a guest holding
///   the hook's copy, and only when [`takes_fresh_snapshot`] says
///   this `up` created the machine or cannot tell.
#[must_use]
pub fn up_plan(state: Option<&VmState>, has_refresh: bool) -> UpPlan {
    if state.is_some_and(VmState::is_running) {
        return if has_refresh {
            UpPlan {
                note: Some(UpNote::RefreshingRunning),
                steps: vec![UpStep::RefreshSecrets],
            }
        } else {
            UpPlan {
                note: Some(UpNote::AlreadyUp),
                steps: Vec::new(),
            }
        };
    }
    let note = state
        .is_none_or(VmState::is_unknown)
        .then_some(UpNote::Unconfirmed);
    let mut steps = vec![UpStep::Boot];
    steps.push(if refreshes_secrets_after_up(state) {
        UpStep::RefreshSecrets
    } else {
        UpStep::RefreshAfterProvisioning
    });
    if takes_fresh_snapshot(state) {
        steps.push(UpStep::Snapshot);
    }
    UpPlan { note, steps }
}

/// How one step went, as the caller of [`run_up_steps`] reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepResult<R> {
    /// Every command of the step succeeded.
    Ok,
    /// The step failed, with what the run should report if it stops.
    /// A refresh never stops the run, so `run_up_steps` drops a
    /// refresh's value and reports `UpOutcome::RefreshFailed`.
    Failed(R),
}

/// How the whole `up` went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpOutcome<R> {
    /// Every step succeeded.
    Ok,
    /// A boot or a snapshot failed, and the run stopped there.
    Stopped(R),
    /// Every step ran, but a refresh failed.
    RefreshFailed,
}

/// Runs `steps` in order through `run`, and decides whether the run
/// goes on after a step fails.
///
/// - **A failed boot stops the run:** nothing after it can reach the
///   guest.
/// - **A failed refresh does not stop the snapshot.** A later `up`
///   finds the machine present and takes no `fresh-install`
///   snapshot, so skipping it here would leave `reset` with nothing
///   to return to. The run still reports the refresh's failure.
/// - **A failed snapshot stops the run** with its own status.
///
/// An error from `run` stops the run at once and is returned.
///
/// # Errors
///
/// Returns the first error `run` returns.
pub fn run_up_steps<R, E>(
    steps: &[UpStep],
    mut run: impl FnMut(UpStep) -> Result<StepResult<R>, E>,
) -> Result<UpOutcome<R>, E> {
    let mut refresh_ok = true;
    for &step in steps {
        match (step, run(step)?) {
            (_, StepResult::Ok) => {}
            (
                UpStep::RefreshSecrets | UpStep::RefreshAfterProvisioning,
                StepResult::Failed(_),
            ) => refresh_ok = false,
            (UpStep::Boot | UpStep::Snapshot, StepResult::Failed(r)) => {
                return Ok(UpOutcome::Stopped(r));
            }
        }
    }
    Ok(if refresh_ok {
        UpOutcome::Ok
    } else {
        UpOutcome::RefreshFailed
    })
}

#[cfg(test)]
mod tests {
    use super::UpStep::{
        Boot, RefreshAfterProvisioning, RefreshSecrets, Snapshot,
    };
    use super::*;

    fn reported(word: &str) -> VmState {
        VmState::Reported(word.to_owned())
    }

    #[test]
    fn a_running_machine_is_only_refreshed() {
        let running = reported("running");
        assert_eq!(
            up_plan(Some(&running), false),
            UpPlan {
                note: Some(UpNote::AlreadyUp),
                steps: vec![],
            }
        );
        assert_eq!(
            up_plan(Some(&running), true),
            UpPlan {
                note: Some(UpNote::RefreshingRunning),
                steps: vec![RefreshSecrets],
            }
        );
    }

    #[test]
    fn an_absent_machine_is_booted_provisioned_and_snapshotted() {
        // vagrant provisions a machine it creates, so only what
        // follows a provision runs after the boot.
        for absent in [VmState::NotCreated, reported("not created")] {
            for has_refresh in [false, true] {
                assert_eq!(
                    up_plan(Some(&absent), has_refresh),
                    UpPlan {
                        note: None,
                        steps: vec![Boot, RefreshAfterProvisioning, Snapshot],
                    },
                    "{absent:?}"
                );
            }
        }
    }

    #[test]
    fn a_stopped_machine_is_booted_and_refreshed_without_a_snapshot() {
        // Its disk is in use, so a `fresh-install` name would
        // mislabel it.
        for stopped in ["shutoff", "poweroff", "paused"] {
            assert_eq!(
                up_plan(Some(&reported(stopped)), true),
                UpPlan {
                    note: None,
                    steps: vec![Boot, RefreshSecrets],
                },
                "{stopped}"
            );
        }
    }

    #[test]
    fn an_unconfirmed_state_gets_a_note_and_a_first_boot() {
        let unknown = VmState::Unknown("unreachable".to_owned());
        for state in [Some(&unknown), None] {
            assert_eq!(
                up_plan(state, true),
                UpPlan {
                    note: Some(UpNote::Unconfirmed),
                    steps: vec![Boot, RefreshSecrets, Snapshot],
                },
                "{state:?}"
            );
        }
    }

    /// Runs `steps`, failing those in `failing` with the step's own
    /// name, and returns the outcome and the steps that ran.
    fn run_failing(
        steps: &[UpStep],
        failing: &[UpStep],
    ) -> (UpOutcome<UpStep>, Vec<UpStep>) {
        let mut ran = Vec::new();
        let outcome = run_up_steps(steps, |step| {
            ran.push(step);
            Ok::<_, ()>(if failing.contains(&step) {
                StepResult::Failed(step)
            } else {
                StepResult::Ok
            })
        })
        .expect("the fake runner returns no error");
        (outcome, ran)
    }

    const FIRST_UP: [UpStep; 3] = [Boot, RefreshSecrets, Snapshot];

    #[test]
    fn every_step_runs_when_none_fails() {
        assert_eq!(
            run_failing(&FIRST_UP, &[]),
            (UpOutcome::Ok, FIRST_UP.to_vec())
        );
    }

    #[test]
    fn a_failed_boot_stops_the_run() {
        assert_eq!(
            run_failing(&FIRST_UP, &[Boot]),
            (UpOutcome::Stopped(Boot), vec![Boot])
        );
    }

    #[test]
    fn a_failed_refresh_still_takes_the_snapshot() {
        // `reset` needs the baseline, and a later `up` will not take
        // it.
        for refresh in [RefreshSecrets, RefreshAfterProvisioning] {
            let steps = [Boot, refresh, Snapshot];
            assert_eq!(
                run_failing(&steps, &[refresh]),
                (UpOutcome::RefreshFailed, steps.to_vec())
            );
        }
    }

    #[test]
    fn a_failed_snapshot_stops_the_run_with_its_status() {
        assert_eq!(
            run_failing(&FIRST_UP, &[RefreshSecrets, Snapshot]),
            (UpOutcome::Stopped(Snapshot), FIRST_UP.to_vec())
        );
    }

    #[test]
    fn an_error_from_the_runner_stops_the_run() {
        let mut ran = Vec::new();
        let outcome = run_up_steps(&FIRST_UP, |step| {
            ran.push(step);
            Err::<StepResult<()>, _>("broken")
        });
        assert_eq!(outcome, Err("broken"));
        assert_eq!(ran, [Boot]);
    }
}
