use std::time::Instant;

use crate::audit;
use crate::canon;
use crate::clippy_cmd;
use crate::coverage;
use crate::dep_age;
use crate::doc_cmd;
use crate::dupes;
use crate::fmt_cmd;
use crate::helpers::{elapsed_str, step_output};
use crate::test_cmd;

/// One gate: its label, the standalone subcommand that re-runs
/// just it, and the work.
///
/// The step numbers and the total are **derived** from the table's
/// order and length; do not reintroduce literals. Hand-written
/// indices plus a matching `TOTAL_STEPS` const had nothing checking
/// the two, so inserting a gate and missing one call site printed
/// `[5/9]` twice with every test still green.
struct Step {
    /// Label printed in the `[N/T]` line.
    name: &'static str,
    /// The `cargo xtask <cmd>` that runs this gate alone.
    cmd: &'static str,
    /// The gate itself, returning its detail string.
    run: Box<dyn FnOnce() -> Result<String, String>>,
}

/// Run all validation steps with concise stepwise
/// output.
///
/// Dep-age runs first so a dependency adopted within the
/// cooldown fails the gate *before* the compile steps
/// (Clippy, Test, Coverage) build -- and run its build
/// script -- on the too-new crate. It is free when the
/// lockfile is unchanged (no network, ~0.1s), so on the
/// common no-dep-change commit its early placement costs
/// nothing; it only reaches the network on a commit that
/// actually adopts a dependency, and even then a
/// connectivity failure degrades to a warning (a genuine
/// cooldown breach still fails; a transient outage does not),
/// so it never blocks the local gates behind the network.
///
/// After Dep-age, steps run cheap static gates first and the
/// expensive dynamic gates (Test, Coverage) last, so a fast
/// check's failure is not gated behind the multi-minute
/// instrumented Coverage run. Fmt leads the static gates
/// because it rewrites whitespace that later checks read. Doc
/// closes them: rustdoc reuses the metadata Clippy's check has
/// just produced, so it costs about a second. The
/// Audit step is network-dependent, so it runs last (after
/// Coverage) with the same degrade-to-warning treatment as
/// Dep-age -- a positive vulnerability finding still fails the
/// gate, but a transient outage does not.
///
/// `check` selects fmt's mode: `false` (default) auto-fixes
/// formatting in place; `true` runs the read-only
/// `fmt --check` for CI or before partial staging, where
/// an in-place rewrite would sweep unrelated drift into the
/// working tree.
pub fn validate(check: bool) -> Result<(), String> {
    let overall_start = Instant::now();
    let steps = steps(check);
    let total = steps.len();
    for (i, s) in steps.into_iter().enumerate() {
        run_step(i + 1, total, s)?;
    }

    println!("Validate OK ({})", elapsed_str(overall_start));
    Ok(())
}

/// The gate table, in the order they run.
///
/// A function rather than an expression inside [`validate`], so the
/// table is data a test can look at. What that buys: the `cmd`
/// strings are the `iterate with: cargo xtask <cmd>` hints, and
/// nothing else ties them to the clap subcommands in
/// [`crate::XCommand`] -- a renamed subcommand would leave a hint
/// telling the operator to run a command that does not exist, with
/// every test green. That is the same drift the derived step
/// numbers were introduced to remove.
fn steps(check: bool) -> Vec<Step> {
    vec![
        // Fail fast on a within-cooldown dependency before
        // compiling it.
        step("Dep-age", "dep-age-check", run_dep_age),
        // Cheap static gates first ...
        step("Fmt", "fmt", move || run_fmt(check)),
        // Reads markdown only, so it needs no compilation and
        // sits ahead of every gate that does.
        step("Canon", "canon-check", run_canon),
        step("Duplication", "dupes", run_duplication),
        step("Deny", "deny", run_deny),
        step("Clippy", "clippy", run_clippy),
        step("Doc", "doc", run_doc),
        // ... expensive dynamic gates last.
        step("Test (xtask only)", "test", run_test),
        step("Coverage", "coverage", run_coverage),
        step("Audit", "audit", run_audit),
    ]
}

/// Builds one [`Step`], so the table reads as a list rather than as
/// a column of `Box::new` calls.
fn step<F>(name: &'static str, cmd: &'static str, run: F) -> Step
where
    F: FnOnce() -> Result<String, String> + 'static,
{
    Step {
        name,
        cmd,
        run: Box::new(run),
    }
}

/// Run a single step, printing the `[N/T]` result line.
///
/// `cmd` is the standalone xtask subcommand for this step;
/// on failure it is printed as an iterate-with hint so the
/// user re-runs the single failing gate (seconds) instead
/// of the whole pipeline (minutes).
fn run_step(index: usize, total: usize, s: Step) -> Result<(), String> {
    let start = Instant::now();
    match (s.run)() {
        Ok(detail) => {
            let time = elapsed_str(start);
            let full = if detail.is_empty() {
                time
            } else {
                format!("{detail}, {time}")
            };
            step_output(index, total, s.name, "OK", &full);
            Ok(())
        }
        Err(e) => {
            step_output(index, total, s.name, "FAILED", "");
            eprintln!("  -> iterate with: cargo xtask {}", s.cmd);
            Err(e)
        }
    }
}

/// Fmt step -- returns empty detail on success. Auto-fixes
/// unless `check` selects the read-only path.
fn run_fmt(check: bool) -> Result<String, String> {
    if check {
        fmt_cmd::fmt_check()?;
    } else {
        fmt_cmd::fmt()?;
    }
    Ok(String::new())
}

/// Clippy step -- returns empty detail on success.
fn run_clippy() -> Result<String, String> {
    let r = clippy_cmd::clippy_check()?;
    match r.error {
        None => Ok(String::new()),
        Some(err) => {
            for line in r.items.iter().take(5) {
                eprintln!("  {line}");
            }
            Err(err)
        }
    }
}

/// Doc step -- returns empty detail on success.
///
/// Grouped with the cheap static gates: rustdoc reuses the
/// metadata `cargo check` already produced, so both passes
/// together cost about a second on a warm target directory.
fn run_doc() -> Result<String, String> {
    match doc_cmd::doc_check()? {
        None => Ok(String::new()),
        Some(failure) => {
            for line in failure.items.iter().take(5) {
                eprintln!("  {line}");
            }
            Err(failure.summary)
        }
    }
}

/// Test step -- runs xtask's own tests only.
///
/// The coverage step runs `--workspace --exclude xtask`
/// under llvm-cov instrumentation, which executes every
/// non-xtask test. Running the full workspace tests
/// here too would duplicate that work. Restricting to
/// `-p xtask` keeps validate a full quality gate
/// without paying the duplication cost.
fn run_test() -> Result<String, String> {
    test_cmd::test_check_xtask()?;
    Ok(String::new())
}

/// The licence, bans and sources gate -- see [`crate::deny`] for
/// why it is offline and why a missing tool is an error here.
fn run_deny() -> Result<String, String> {
    crate::deny::deny().map(|()| String::from("licenses, bans, sources"))
}

/// Security-advisory step -- fails on any vulnerability
/// (RUSTSEC). Advisory warnings are informational,
/// and a connectivity / missing-tool failure degrades to a
/// printed warning rather than failing the gate, so an
/// offline machine or fresh CI run is not blocked by a
/// transient outage.
fn run_audit() -> Result<String, String> {
    let r = audit::audit_check();
    if let Some(err) = r.error {
        return Err(err);
    }
    for w in &r.warnings {
        eprintln!("  warning: {w}");
    }
    Ok(r.detail)
}

/// Dep-age step -- cooldown-checks only dependencies added or
/// bumped in the working tree versus HEAD. A within-cooldown
/// dependency fails the gate; a missing baseline or
/// unreachable registry degrades to a printed warning (same
/// treatment as Audit), so an offline run is not blocked.
fn run_dep_age() -> Result<String, String> {
    let r = dep_age::check_changed_deps();
    if let Some(err) = r.error {
        return Err(err);
    }
    for w in &r.warnings {
        eprintln!("  warning: {w}");
    }
    Ok(r.detail)
}

/// Coverage step -- returns "N.N% >= 90%" detail.
fn run_coverage() -> Result<String, String> {
    let r = coverage::coverage_check()?;
    match r.error {
        None => Ok(format!(
            "{:.1}% >= {}%",
            r.line_pct,
            coverage::OVERALL_THRESHOLD,
        )),
        Some(failure) => Err(coverage::format_failure(&failure)),
    }
}

/// Duplication step -- returns detail string.
fn run_canon() -> Result<String, String> {
    canon::canon_check_detail()
}

fn run_duplication() -> Result<String, String> {
    let r = dupes::dupes_check()?;
    if let Some(err) = r.error {
        Err(err)
    } else {
        Ok(r.detail)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use clap::CommandFactory;

    use super::*;

    #[test]
    fn every_iterate_hint_names_a_real_subcommand() {
        // The hint printed on failure is `cargo xtask <cmd>`, and
        // nothing else connects those strings to the clap surface.
        // Renaming a subcommand would otherwise leave the gate
        // advising a command that does not exist, with the whole
        // suite green.
        let cli = crate::Cli::command();
        for s in steps(false) {
            assert!(
                cli.find_subcommand(s.cmd).is_some(),
                "no `cargo xtask {}` subcommand for the {} gate",
                s.cmd,
                s.name
            );
        }
    }

    #[test]
    fn the_gates_are_distinct_and_named() {
        let steps = steps(false);
        let names: BTreeSet<&str> = steps.iter().map(|s| s.name).collect();
        assert_eq!(names.len(), steps.len(), "a gate label is repeated");
        assert!(steps.iter().all(|s| !s.name.is_empty()));
    }

    #[test]
    fn the_order_is_the_documented_one() {
        // Cheapest first, network last. Asserted rather than left to
        // the prose above: the ordering is the reason a failing
        // clippy run does not cost a coverage run first, and a
        // reordering that undoes that is invisible otherwise.
        let order: Vec<&str> = steps(false).iter().map(|s| s.name).collect();
        assert_eq!(
            order,
            vec![
                "Dep-age",
                "Fmt",
                "Canon",
                "Duplication",
                "Deny",
                "Clippy",
                "Doc",
                "Test (xtask only)",
                "Coverage",
                "Audit",
            ]
        );
    }
}
