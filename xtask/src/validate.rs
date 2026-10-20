use std::path::Path;

use rustbase_xtask::step::{self, Step};
use rustbase_xtask::tee::{OpenError, Tee};

use crate::helpers::tasks_config;
#[cfg(feature = "frontend")]
use crate::{frontend_check, frontend_dupes, frontend_fmt, frontend_test};

/// The library's standard gates (`step::standard`, which gives their
/// order), with this project's frontend gates after Deny: three
/// static ones, then the frontend tests, ahead of the Rust tests.
///
/// The `[N/M]` numbering follows whichever gates are compiled in.
// Without the frontend feature nothing is inserted, so nothing fails.
#[cfg_attr(not(feature = "frontend"), allow(clippy::unnecessary_wraps))]
fn steps(check: bool) -> Result<Vec<Step>, String> {
    #[cfg_attr(not(feature = "frontend"), allow(unused_mut))]
    let mut steps = step::standard(&tasks_config(), check);
    #[cfg(feature = "frontend")]
    step::insert_after(
        &mut steps,
        step::names::DENY,
        [
            frontend_fmt::step(check),
            frontend_check::step(),
            frontend_dupes::step(),
            frontend_test::step(),
        ],
    )?;
    Ok(steps)
}

/// Run every gate, printing one `[N/M]` line each. `check` makes the
/// format gates read-only instead of fixing in place, for CI or
/// before partial staging, where a rewrite would sweep unrelated
/// drift into the working tree.
pub fn validate(check: bool) -> Result<(), String> {
    // Opened first, so a failure at any later point reaches the log.
    let tee = open_log(&tasks_config().root)?;
    let result = (|| {
        frontend_feature_check(
            cfg!(feature = "frontend"),
            Path::new("frontend/package.json").exists(),
        )?;
        step::run_all(steps(check)?, &tee)
    })();
    record_failure(&tee, &result);
    result
}

/// `target/validate.log`. While a first run holds it, a second run is
/// refused: it could not write the log, which would keep the first
/// run's output, and a reader would take that as the second run's
/// result. Any other failure to open it warns and falls back to
/// terminal-only output, since the log is a side record.
fn open_log(root: &Path) -> Result<Tee, String> {
    match Tee::open(root, "validate") {
        Ok(tee) => Ok(tee),
        Err(OpenError::Held(path)) => Err(format!(
            "another validate is running in this checkout ({} is held); \
             wait for it to finish",
            path.display()
        )),
        Err(e) => {
            eprintln!("warning: {e}; this run is not logged");
            Ok(Tee::terminal())
        }
    }
}

/// Write a failure to the log as `main` prints it.
fn record_failure(tee: &Tee, result: &Result<(), String>) {
    if let Err(e) = result {
        tee.record(&crate::error_line(e));
    }
}

/// Refuse a frontend on disk when the `frontend` feature is off:
/// its gates are then compiled out, and a run with none of them
/// would read as a pass.
fn frontend_feature_check(
    feature_on: bool,
    has_frontend: bool,
) -> Result<(), String> {
    if has_frontend && !feature_on {
        return Err("frontend/ exists but xtask's `frontend` feature is \
             off, so validate would run none of its gates -- add \
             \"frontend\" to `default` in xtask/Cargo.toml, or delete \
             frontend/"
            .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn every_step_hint_resolves_to_a_subcommand() {
        // Each Step's `cmd` is printed as the
        // `iterate with: cargo xtask <cmd>` hint on failure.
        // Resolve every one through the actual CLI so a renamed
        // or removed subcommand cannot leave the hint pointing at
        // a command that does not exist. `check` does not affect
        // the cmd strings, so either value exercises them all.
        let cli = crate::Cli::command();
        for step in steps(false).unwrap() {
            assert!(
                cli.find_subcommand(step.cmd()).is_some(),
                "validate step {:?} hints `cargo xtask {}`, \
                 which is not a real subcommand",
                step.name(),
                step.cmd(),
            );
        }
    }

    #[test]
    fn frontend_steps_follow_the_feature_and_sit_after_deny() {
        // The frontend gates are in the list exactly when the
        // feature compiles them in, so the `[N/M]` total counts
        // only gates that exist.
        let all = steps(false).unwrap();
        let names: Vec<&str> = all.iter().map(Step::name).collect();
        let deny = names.iter().position(|n| *n == step::names::DENY).unwrap();
        let first = names.iter().position(|n| n.starts_with("Frontend"));
        if cfg!(feature = "frontend") {
            assert_eq!(first, Some(deny + 1), "{names:?}");
        } else {
            assert_eq!(first, None, "{names:?}");
        }
    }

    #[test]
    fn the_help_carries_no_program_description() {
        // `about = None` on `Cli` keeps the doc comments of `Cli`,
        // `XCommand` and the flattened `LibCommand` out of `--help`.
        assert!(crate::Cli::command().get_about().is_none());
    }

    #[test]
    fn a_log_that_cannot_open_falls_back_to_the_terminal() {
        let root = std::env::temp_dir()
            .join(format!("xtask-validate-nolog-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("target"), "a file, not a dir").unwrap();
        let tee = open_log(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(tee.unwrap().path(), None);
    }

    #[test]
    fn a_second_run_while_one_holds_the_log_is_refused() {
        // Its fallback would leave the first run's result in the log,
        // read as this run's.
        let root = std::env::temp_dir()
            .join(format!("xtask-validate-held-{}", std::process::id()));
        let first = Tee::open(&root, "validate").unwrap();
        let e = open_log(&root).unwrap_err();
        drop(first);
        let _ = std::fs::remove_dir_all(&root);
        assert!(e.contains("another validate is running"), "{e}");
    }

    #[test]
    fn a_failure_is_logged_as_main_prints_it() {
        let root = std::env::temp_dir()
            .join(format!("xtask-validate-log-{}", std::process::id()));
        let tee = Tee::open(&root, "validate").unwrap();
        record_failure(&tee, &Ok(()));
        record_failure(&tee, &Err("boom".into()));
        drop(tee);
        let log = std::fs::read_to_string(root.join("target/validate.log"));
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(log.unwrap(), format!("{}\n", crate::error_line("boom")));
    }

    #[test]
    fn frontend_on_disk_needs_the_frontend_feature() {
        // With the feature off its gates are compiled out, so a
        // frontend on disk would pass validate unchecked.
        let err = frontend_feature_check(false, true).unwrap_err();
        assert!(err.contains("`frontend` feature"), "{err}");
        assert!(frontend_feature_check(true, true).is_ok());
        assert!(frontend_feature_check(true, false).is_ok());
        assert!(frontend_feature_check(false, false).is_ok());
    }
}
