use crate::helpers::{run_cargo_capture, run_cargo_stream};

/// Maximum failure detail lines per test.
const MAX_DETAIL_LINES: usize = 5;

/// Test invocation scope.
#[derive(Clone, Copy)]
enum Scope {
    /// `--workspace` -- every crate in the workspace.
    Workspace,
    /// `-p xtask` -- only the xtask crate. Used by
    /// validate's Test step (see module docs there).
    XtaskOnly,
}

/// Options for [`test`]. Grouped so adding the next flag
/// (`--locked`, `--no-fail-fast`, ...) doesn't widen the
/// function signature.
#[derive(Clone, Copy, Default)]
pub struct TestOptions<'a> {
    pub filter: Option<&'a str>,
    pub verbose: bool,
    pub ignored: bool,
}

/// Run tests with concise output.
///
/// Prints `Test OK` on success. On failure, shows only
/// the failing test names and assertion details.
/// With `verbose`, streams raw cargo test output.
/// With `ignored`, runs `#[ignore]`-tagged tests instead
/// of the default set.
pub fn test(opts: TestOptions<'_>) -> Result<(), String> {
    let args = build_args(Scope::Workspace, opts.filter, opts.ignored)?;

    if opts.verbose {
        // Verbose streams raw cargo output live, so the
        // filtered-zero-tests guard below is intentionally
        // skipped: a human watching the stream sees
        // `running 0 tests` directly, and the streaming
        // path captures no stdout to count. The guard's
        // false-green risk only bites the condensed
        // capture path, which prints a bare `Test OK`.
        return run_cargo_stream(&args);
    }

    let output = run_cargo_capture(&args)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        // `cargo test <filter>` exits 0 when the filter
        // matches nothing, so a typo'd or over-specific
        // filter would print "Test OK" having run nothing
        // -- a false green. A bare (unfiltered) run can
        // legitimately execute zero tests in an empty
        // crate, so scope the guard to filtered runs.
        if let Some(f) = opts.filter
            && count_tests_run(&stdout) == 0
        {
            return Err(format!(
                "filter {f:?} matched zero tests -- nothing ran"
            ));
        }
        println!("Test OK");
        return Ok(());
    }

    report_failure(&stdout, &stderr)
}

/// Sum the `running N tests` counts cargo prints, one per
/// test binary (`running 1 test` / `running 3 tests`).
/// Used to detect a filtered invocation that matched
/// nothing even though cargo exited 0.
fn count_tests_run(stdout: &str) -> u64 {
    stdout
        .lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("running ")?;
            let n = rest
                .strip_suffix(" tests")
                .or_else(|| rest.strip_suffix(" test"))?;
            n.trim().parse::<u64>().ok()
        })
        .sum()
}

/// Run only xtask's own tests quietly.
///
/// Used by validate's Test step. Coverage runs the
/// workspace test suite under llvm-cov instrumentation
/// (with `--exclude xtask`), so running the full
/// workspace tests separately in Test would duplicate
/// the same passes. Restricting Test to `-p xtask`
/// keeps xtask covered without re-running every other
/// crate.
///
/// On failure, prints the same rich diagnostics as
/// `test()` to stderr (failing names, assertion
/// details, or compile errors) before returning.
pub fn test_check_xtask() -> Result<(), String> {
    let args = build_args(Scope::XtaskOnly, None, false)?;
    let output = run_cargo_capture(&args)?;

    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    report_failure(&stdout, &stderr)
}

/// Print failure diagnostics to stderr and return the
/// corresponding error string. Shared between `test()`
/// and `test_check_xtask()`.
fn report_failure(stdout: &str, stderr: &str) -> Result<(), String> {
    // Compilation error -- show first error lines.
    if stderr.contains("could not compile") {
        let errors: Vec<&str> = extract_compile_error_lines(stderr)
            .into_iter()
            .take(10)
            .collect();
        eprintln!("FAILED: compilation error\n");
        for line in &errors {
            eprintln!("  {line}");
        }
        return Err("compilation failed".into());
    }

    // Test failures -- show failing names + details.
    let failed_names = extract_failed_names(stdout);
    let failures = extract_failure_details(stdout, stderr);

    eprintln!("FAILED\n");
    if failures.is_empty() {
        for name in &failed_names {
            eprintln!("  {name}");
        }
    } else {
        for f in &failures {
            eprintln!("  {}", f.name);
            for d in f.details.iter().take(MAX_DETAIL_LINES) {
                eprintln!("    {d}");
            }
        }
    }
    Err("test(s) failed".into())
}

/// Build the cargo test argument list.
///
/// Centralised so both the CLI `test` command and
/// validate's xtask-only test step go through the
/// same arg-construction path. Add new shared flags
/// (e.g. `--locked`, `--no-fail-fast`) here.
fn build_args(
    scope: Scope,
    filter: Option<&str>,
    ignored: bool,
) -> Result<Vec<&str>, String> {
    let mut args = vec!["test"];
    match scope {
        Scope::Workspace => args.push("--workspace"),
        Scope::XtaskOnly => {
            args.push("-p");
            args.push("xtask");
        }
    }
    // Everything after `--` is forwarded to the test
    // harness. Both `--ignored` and a positional filter
    // must live there, so emit the separator if either
    // is set.
    if filter.is_some() || ignored {
        args.push("--");
    }
    if let Some(f) = filter {
        if f.is_empty() {
            return Err("test filter must not be empty".into());
        }
        args.push(f);
    }
    if ignored {
        args.push("--ignored");
    }
    Ok(args)
}

/// Extract compile-error lines from `cargo test` stderr,
/// keeping each `error...` line together with the `-->`
/// source-location line that immediately follows it.
/// `cargo test` uses the default (long) diagnostic format,
/// where the location lives on the next line; keeping only
/// the message line left the caller with no `file:line`.
fn extract_compile_error_lines(stderr: &str) -> Vec<&str> {
    crate::helpers::pair_with_locations(stderr, |l| l.starts_with("error"))
}

/// Extract test names from `test foo ... FAILED` lines.
fn extract_failed_names(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter(|l| l.trim().ends_with("... FAILED"))
        .map(|l| {
            l.trim()
                .strip_prefix("test ")
                .unwrap_or(l.trim())
                .strip_suffix(" ... FAILED")
                .unwrap_or(l.trim())
                .to_string()
        })
        .collect()
}

/// A single test failure with detail lines.
struct FailureDetail {
    /// Fully qualified test name.
    name: String,
    /// Assertion detail lines (panic message, etc.).
    details: Vec<String>,
}

/// Extract failing test details from
/// `---- name stdout ----` sections.
fn extract_failure_details(stdout: &str, stderr: &str) -> Vec<FailureDetail> {
    let mut failures = Vec::new();
    let combined = format!("{stdout}\n{stderr}");

    let mut current_name: Option<String> = None;
    let mut current_details: Vec<String> = Vec::new();

    for line in combined.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("---- ") {
            if let Some(name) = current_name.take() {
                failures.push(FailureDetail {
                    name,
                    details: std::mem::take(&mut current_details),
                });
            }
            if let Some(name) = rest.strip_suffix(" stdout ----") {
                current_name = Some(name.to_string());
            }
        } else if current_name.is_some()
            && !trimmed.is_empty()
            && !trimmed.starts_with("thread '")
            && !trimmed.starts_with("note: run with")
        {
            current_details.push(trimmed.to_string());
        }
    }

    if let Some(name) = current_name.take() {
        failures.push(FailureDetail {
            name,
            details: current_details,
        });
    }

    failures
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_failed_names_from_output() {
        let stdout = "\
test foo::bar ... ok
test baz::qux ... FAILED
test another::test ... ok
test third::fail ... FAILED";
        let names = extract_failed_names(stdout);
        assert_eq!(names.len(), 2);
        assert_eq!(names[0], "baz::qux");
        assert_eq!(names[1], "third::fail");
    }

    #[test]
    fn extract_failed_names_none() {
        let stdout = "test foo::bar ... ok\n\
            test result: ok. 1 passed";
        let names = extract_failed_names(stdout);
        assert!(names.is_empty());
    }

    #[test]
    fn extract_details_from_output() {
        let stdout = "\
test api::tests::my_test ... FAILED

failures:

---- api::tests::my_test stdout ----
thread 'api::tests::my_test' panicked at 'msg'
assertion `left == right` failed
  left: 1
 right: 2
note: run with RUST_BACKTRACE=1

failures:
    api::tests::my_test
";
        let failures = extract_failure_details(stdout, "");
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].name, "api::tests::my_test");
        assert!(
            failures[0].details.iter().any(|d| d.contains("assertion")),
            "should contain assertion detail"
        );
        assert!(
            !failures[0]
                .details
                .iter()
                .any(|d| d.starts_with("thread '")),
            "should not contain thread line"
        );
    }

    #[test]
    fn build_args_workspace_no_filter() {
        let args = build_args(Scope::Workspace, None, false).unwrap();
        assert_eq!(args, vec!["test", "--workspace"]);
    }

    #[test]
    fn build_args_xtask_only() {
        let args = build_args(Scope::XtaskOnly, None, false).unwrap();
        assert_eq!(args, vec!["test", "-p", "xtask"]);
    }

    #[test]
    fn build_args_with_filter() {
        let args = build_args(Scope::Workspace, Some("foo"), false).unwrap();
        assert_eq!(args, vec!["test", "--workspace", "--", "foo"]);
    }

    #[test]
    fn build_args_empty_filter_errors() {
        let err = build_args(Scope::Workspace, Some(""), false).unwrap_err();
        assert!(err.contains("must not be empty"));
    }

    #[test]
    fn build_args_ignored_no_filter() {
        let args = build_args(Scope::Workspace, None, true).unwrap();
        assert_eq!(args, vec!["test", "--workspace", "--", "--ignored"]);
    }

    #[test]
    fn extract_compile_error_lines_keeps_locations() {
        let stderr = "\
error[E0425]: cannot find value `foo`
  --> crates/rustbase/src/lib.rs:10:5
warning: unused variable: `x`
  --> crates/rustbase/src/lib.rs:99:9
   = note: some trailing context
error: could not compile `rustbase` (lib) due to 1 previous error";
        let lines = extract_compile_error_lines(stderr);
        // The error and its `-->` are kept; the warning and
        // its `-->` are dropped (exercises the drop/reset
        // branch), the `= note` non-matching line is
        // ignored, and the final summary error is kept.
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("E0425"));
        assert!(lines[1].contains("--> crates/rustbase/src/lib.rs:10:5"));
        assert!(lines[2].contains("could not compile"));
        assert!(!lines.iter().any(|l| l.contains("99:9")));
        assert!(!lines.iter().any(|l| l.contains("unused variable")));
    }

    #[test]
    fn count_tests_run_sums_multiple_binaries() {
        let stdout = "\
running 3 tests
test a ... ok
running 1 test
test b ... ok
running 0 tests";
        assert_eq!(count_tests_run(stdout), 4);
    }

    #[test]
    fn count_tests_run_handles_singular_and_plural() {
        assert_eq!(count_tests_run("running 1 test"), 1);
        assert_eq!(count_tests_run("running 12 tests"), 12);
    }

    #[test]
    fn count_tests_run_zero_when_nothing_ran() {
        let stdout = "running 0 tests\n\ntest result: ok. 0 passed";
        assert_eq!(count_tests_run(stdout), 0);
    }

    #[test]
    fn build_args_ignored_with_filter() {
        let args = build_args(Scope::Workspace, Some("foo"), true).unwrap();
        assert_eq!(
            args,
            vec!["test", "--workspace", "--", "foo", "--ignored",]
        );
    }
}
