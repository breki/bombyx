use std::time::Instant;

use crate::audit;
use crate::clippy_cmd;
use crate::coverage;
use crate::dep_age;
use crate::dupes;
use crate::fmt_cmd;
use crate::helpers::{elapsed_str, step_output};
use crate::test_cmd;

/// Total number of validation steps.
const TOTAL_STEPS: usize = 7;

/// Run all validation steps with concise stepwise
/// output.
///
/// Dep-age runs first so a dependency adopted within the
/// cooldown fails the gate *before* the compile steps
/// (Clippy, Test, Coverage) build -- and run its build
/// script -- on the too-new crate. It is free when the
/// lockfiles are unchanged (no network, ~0.1s), so on the
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
/// because it rewrites whitespace that later checks read. The
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

    // Fail fast on a within-cooldown dependency before compiling it.
    run_step(1, "Dep-age", "dep-age-check", run_dep_age)?;

    // Cheap static gates first ...
    run_step(2, "Fmt", "fmt", || run_fmt(check))?;
    run_step(3, "Duplication", "dupes", run_duplication)?;
    run_step(4, "Clippy", "clippy", run_clippy)?;

    // ... expensive dynamic gates last.
    run_step(5, "Test (xtask only)", "test", run_test)?;
    run_step(6, "Coverage", "coverage", run_coverage)?;
    run_step(7, "Audit", "audit", run_audit)?;

    println!("Validate OK ({})", elapsed_str(overall_start));
    Ok(())
}

/// Run a single step, printing the `[N/T]` result line.
///
/// `cmd` is the standalone xtask subcommand for this step;
/// on failure it is printed as an iterate-with hint so the
/// user re-runs the single failing gate (seconds) instead
/// of the whole pipeline (minutes).
fn run_step<F>(step: usize, name: &str, cmd: &str, f: F) -> Result<(), String>
where
    F: FnOnce() -> Result<String, String>,
{
    let start = Instant::now();
    match f() {
        Ok(detail) => {
            let time = elapsed_str(start);
            let full = if detail.is_empty() {
                time
            } else {
                format!("{detail}, {time}")
            };
            step_output(step, TOTAL_STEPS, name, "OK", &full);
            Ok(())
        }
        Err(e) => {
            step_output(step, TOTAL_STEPS, name, "FAILED", "");
            eprintln!("  -> iterate with: cargo xtask {cmd}");
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

/// Security-advisory step -- fails on any vulnerability
/// (RUSTSEC or npm). Advisory warnings are informational,
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
fn run_duplication() -> Result<String, String> {
    let r = dupes::dupes_check()?;
    if let Some(err) = r.error {
        Err(err)
    } else {
        Ok(r.detail)
    }
}
