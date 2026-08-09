//! `cargo xtask dep-preflight` -- pre-compile cooldown
//! remediation for the Rust dependency tree.
//!
//! The `dep-age-check` gate (see [`super::gate`]) runs *after*
//! `cargo` has already resolved, fetched, and compiled a
//! change -- so by the time it fails, a freshly-published
//! (possibly compromised) crate's build script has already run
//! on the build host. `dep-preflight` closes that window for
//! the crates a pending change introduces: it inspects the
//! newly-locked versions, and for each one still inside the
//! cooldown it pins the crate down to the newest version that
//! has cleared the window (`cargo update --precise`), looping
//! until the whole changed set is aged or no aged version
//! satisfies the resolved requirements.
//!
//! Every step touches only the registry *index* (metadata) and
//! the lockfile -- `cargo update --precise` neither fetches
//! crate tarballs nor compiles -- so no third-party build
//! script runs until the operator, seeing a clean tree, chooses
//! to build.
//!
//! **Front-door only.** This is protection you opt into by
//! running it *instead of* going straight to `cargo build`
//! after adding or bumping a dependency (add the dep with
//! `cargo add`, which updates the lockfile without compiling,
//! then run this). It cannot intercept a bare `cargo build`
//! that resolves-and-compiles in one shot -- only cargo's
//! in-resolver `-Zmin-publish-age` (nightly) does that
//! automatically. See the supply-chain notes in `CLAUDE.md`.
//!
//! Scope: Rust / crates.io only. The frontend (`npm`) tree has
//! its own resolution quirks (`ERESOLVE`, the `cd frontend`
//! trap) and is left to `/update-deps`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::process::Command;

use super::gate::{git_show, new_locked_versions, parse_cargo_lock};
use super::{
    COOLDOWN_DAYS, Ecosystem, age_in_days, cargo_versions, fetch_registry,
    today_days, version_key,
};
use crate::helpers::workspace_root;

/// Safety backstop on the pin-and-re-resolve loop. Each pin
/// permanently ages one crate, and the dependency tree is
/// finite, so convergence normally takes a handful of rounds;
/// this only guards a pathological pin cycle.
const MAX_ITERS: usize = 25;

/// What to do with one changed crate this round. Pure verdict
/// derived from the registry's version list -- the branchy
/// decision, unit-tested in isolation from any I/O.
#[derive(Debug, PartialEq, Eq)]
enum CrateStep {
    /// Already older than the cooldown -- leave it.
    Aged,
    /// Within the cooldown; pin down to this aged version.
    Pin(String),
    /// Within the cooldown and the registry lists no aged
    /// version -- nothing to pin to.
    DeadEnd,
    /// The locked version is absent from the registry listing
    /// (yanked / prerelease / unpublished) -- age can't be
    /// assessed, so the tree can't be certified clean.
    Unknown,
}

/// The highest aged version strictly *below* `current` (by
/// version order). Pure.
///
/// Preflight only ever pins a crate *down*: raising the version
/// to escape the cooldown would be a silent, possibly-major
/// upgrade the operator never chose. `super::latest_aged`
/// cannot be reused here -- it returns the highest aged version
/// *overall*, which can sit *above* a freshly-published
/// backport on an older line (e.g. `2.0.0` aged while the
/// locked `1.4.0` is fresh), so it would upgrade rather than
/// remediate. Filtering to versions below `current` keeps the
/// pin a genuine downgrade; when none exists the crate is a
/// dead end, reported for the operator to resolve by hand.
fn pin_target(
    current: &str,
    versions: &[(String, i64)],
    today: i64,
) -> Option<String> {
    let current_key = version_key(current);
    versions
        .iter()
        .filter(|(_, day)| age_in_days(*day, today) >= COOLDOWN_DAYS)
        .filter(|(v, _)| version_key(v) < current_key)
        .max_by(|a, b| version_key(&a.0).cmp(&version_key(&b.0)))
        .map(|(v, _)| v.clone())
}

/// Decide the step for one crate given every `(version,
/// publish-day)` the registry lists for it. Pure.
fn crate_step(
    current: &str,
    versions: &[(String, i64)],
    today: i64,
) -> CrateStep {
    let Some((_, day)) = versions.iter().find(|(v, _)| v == current) else {
        return CrateStep::Unknown;
    };
    if age_in_days(*day, today) >= COOLDOWN_DAYS {
        return CrateStep::Aged;
    }
    match pin_target(current, versions, today) {
        Some(target) => CrateStep::Pin(target),
        None => CrateStep::DeadEnd,
    }
}

/// Read the working-tree `Cargo.lock` text.
type ReadLock<'a> = &'a dyn Fn() -> Result<String, String>;
/// Fetch the `HEAD:Cargo.lock` baseline, or `None` if absent.
type BaselineLock<'a> = &'a dyn Fn() -> Option<String>;
/// `(version, publish-day)` list for a crate, or an error
/// string when the registry is unreachable / unparseable.
type FetchVersions<'a> = &'a dyn Fn(&str) -> Result<Vec<(String, i64)>, String>;
/// Pin `name@current` to a target version; `Err` when cargo
/// refuses the pin (target outside the requirement range).
type Pin<'a> = &'a dyn Fn(&str, &str, &str) -> Result<(), String>;

/// Injected I/O so the loop [`run`] is fully unit-testable with
/// fakes. The real implementations ([`real_io`]) shell out to
/// git, curl, and `cargo update`; the tests substitute
/// closures.
pub(super) struct Io<'a> {
    /// Read the working-tree `Cargo.lock` text.
    pub read_lock: ReadLock<'a>,
    /// The `HEAD:Cargo.lock` baseline, or `None` if absent.
    pub baseline_lock: BaselineLock<'a>,
    /// `(version, publish-day)` list for a crate.
    pub fetch_versions: FetchVersions<'a>,
    /// Pin a crate down to a target version. Treated as a dead
    /// end, not a fatal error, when cargo refuses the pin.
    pub pin: Pin<'a>,
    /// Today as days since the Unix epoch (injected so the
    /// verdict is deterministic under test).
    pub today: i64,
}

/// Outcome of a preflight run.
#[derive(Debug)]
pub(super) struct Report {
    /// True when the changed set is entirely aged (or empty)
    /// and every crate could be assessed -- safe to build.
    pub clean: bool,
    /// `name: old -> new` for each crate pinned down.
    pub pinned: Vec<String>,
    /// `name@version` crates within the cooldown that could not
    /// be aged (no aged version, or cargo refused the pin).
    pub dead_ends: Vec<String>,
    /// Count of changed crates whose age could not be checked
    /// (unreachable registry, or version absent from listing).
    pub unresolved: usize,
    /// Human-readable non-fatal notes (network blips, failed
    /// pins, non-convergence).
    pub warnings: Vec<String>,
    /// Loop rounds taken.
    pub iterations: usize,
}

/// Run the pin-and-re-resolve loop over the injected I/O.
/// Returns `Err` only when the state needed to start is missing
/// (no lockfile, no `HEAD` baseline); a dead end or an
/// unreachable registry is reported in the [`Report`], not as
/// an error, so the caller controls the exit message.
pub(super) fn run(io: &Io) -> Result<Report, String> {
    let Some(baseline) = (io.baseline_lock)() else {
        return Err("no HEAD baseline for Cargo.lock -- commit or stash \
                    the current tree so preflight can tell which \
                    dependencies the pending change adds"
            .into());
    };
    let base_pairs = parse_cargo_lock(&baseline);

    let mut pinned: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut iterations = 0usize;
    // A crate's published version set is stable for the run, so
    // fetch each distinct crate at most once across rounds.
    let mut versions_of: HashMap<String, Result<Vec<(String, i64)>, String>> =
        HashMap::new();
    // `(name, target)` pins already applied. Re-reaching the
    // same pin means another crate keeps forcing this one fresh
    // -- an unsatisfiable cooldown cycle, not progress; treat it
    // as a dead end so the loop stops instead of spinning to
    // `MAX_ITERS` re-printing the same line.
    let mut applied: HashSet<(String, String)> = HashSet::new();

    loop {
        iterations += 1;
        let current = (io.read_lock)()?;
        let mut changed =
            new_locked_versions(&base_pairs, &parse_cargo_lock(&current));
        changed.sort();

        // The terminal state is recomputed fresh each round so
        // the report reflects the converged tree, not history;
        // only `pinned` / `warnings` accumulate.
        let mut dead_ends: Vec<String> = Vec::new();
        let mut unresolved = 0usize;
        let mut progressed = false;

        for (name, ver) in &changed {
            if !versions_of.contains_key(name) {
                versions_of.insert(name.clone(), (io.fetch_versions)(name));
            }
            let versions = match &versions_of[name] {
                Ok(v) => v,
                Err(e) => {
                    warnings.push(format!("{name}@{ver}: {e}"));
                    unresolved += 1;
                    continue;
                }
            };
            match crate_step(ver, versions, io.today) {
                CrateStep::Aged => {}
                CrateStep::Unknown => {
                    warnings.push(format!(
                        "{name}@{ver}: not in the registry listing -- \
                         cannot assess its age"
                    ));
                    unresolved += 1;
                }
                CrateStep::DeadEnd => dead_ends.push(format!("{name}@{ver}")),
                CrateStep::Pin(target)
                    if applied.contains(&(name.clone(), target.clone())) =>
                {
                    dead_ends.push(format!("{name}@{ver}"));
                }
                CrateStep::Pin(target) => match (io.pin)(name, ver, &target) {
                    Ok(()) => {
                        applied.insert((name.clone(), target.clone()));
                        pinned.push(format!("{name}: {ver} -> {target}"));
                        progressed = true;
                    }
                    Err(e) => {
                        warnings.push(format!(
                            "{name}@{ver}: pin to {target} failed: {e}"
                        ));
                        dead_ends.push(format!("{name}@{ver}"));
                    }
                },
            }
        }

        if !progressed {
            let clean = dead_ends.is_empty() && unresolved == 0;
            return Ok(Report {
                clean,
                pinned,
                dead_ends,
                unresolved,
                warnings,
                iterations,
            });
        }
        if iterations >= MAX_ITERS {
            warnings
                .push(format!("did not converge within {MAX_ITERS} rounds"));
            return Ok(Report {
                clean: false,
                pinned,
                dead_ends,
                unresolved,
                warnings,
                iterations,
            });
        }
    }
}

/// Read the working-tree `Cargo.lock`.
fn real_read_lock() -> Result<String, String> {
    let path = workspace_root().join("Cargo.lock");
    fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))
}

/// `(version, publish-day)` for a crate from crates.io.
fn real_fetch_versions(name: &str) -> Result<Vec<(String, i64)>, String> {
    let json = fetch_registry(Ecosystem::Cargo, name)?;
    Ok(cargo_versions(&json))
}

/// Pin `name@current` to `target` via `cargo update --precise`
/// (index-only; no fetch of tarballs, no compile). Run through
/// the process working directory, not `git -C`/`cargo -C`, to
/// stay coverable by a blanket permission rule.
fn real_pin(name: &str, current: &str, target: &str) -> Result<(), String> {
    let spec = format!("{name}@{current}");
    let output = Command::new("cargo")
        .current_dir(workspace_root())
        .args(["update", "-p", &spec, "--precise", target])
        .output()
        .map_err(|e| format!("failed to run cargo update: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// The `HEAD:Cargo.lock` baseline, or `None` when it is absent
/// (no commit yet, path not tracked, git unavailable).
fn real_baseline_lock() -> Option<String> {
    git_show("Cargo.lock")
}

/// Wire the real shell-out I/O.
fn real_io() -> Io<'static> {
    Io {
        read_lock: &real_read_lock,
        baseline_lock: &real_baseline_lock,
        fetch_versions: &real_fetch_versions,
        pin: &real_pin,
        today: today_days(),
    }
}

/// `cargo xtask dep-preflight` entry point. Prints what was
/// pinned and any warnings; errors when the tree cannot be
/// certified clear of the cooldown so the operator does not
/// build on an unremediated (or unverified) change.
pub fn dep_preflight() -> Result<(), String> {
    let report = run(&real_io())?;

    for p in &report.pinned {
        println!("  pinned {p}");
    }
    for w in &report.warnings {
        eprintln!("  warning: {w}");
    }

    if report.clean {
        println!(
            "Dep-preflight OK: {} crate(s) pinned, tree clear of the \
             {COOLDOWN_DAYS}-day cooldown in {} round(s) -- safe to build",
            report.pinned.len(),
            report.iterations,
        );
        return Ok(());
    }

    let mut parts: Vec<String> = Vec::new();
    if !report.dead_ends.is_empty() {
        parts.push(format!(
            "no version outside the {COOLDOWN_DAYS}-day cooldown satisfies \
             the resolved requirements for: {}",
            report.dead_ends.join(", ")
        ));
    }
    if report.unresolved > 0 {
        parts.push(format!(
            "{} changed crate(s) could not be checked (see warnings)",
            report.unresolved
        ));
    }
    Err(format!(
        "dep-preflight could not certify the tree -- {}.\n  Wait out the \
         cooldown, adopt the fresh version deliberately (record it in \
         RUSTBASE_DEP_AGE_ALLOW when you commit), or drop/adjust the \
         dependency.",
        parts.join("; ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::fmt::Write as _;

    use crate::dep_age::days_from_civil;

    const TODAY: fn() -> i64 = || days_from_civil(2026, 7, 15);

    /// A minimal `Cargo.lock` listing the given registry crates.
    fn lock(pkgs: &[(&str, &str)]) -> String {
        let mut s = String::from("version = 4\n");
        for (name, version) in pkgs {
            let _ = write!(
                s,
                "\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n\
                 source = \"registry+https://github.com/rust-lang/\
                 crates.io-index\"\n"
            );
        }
        s
    }

    fn day(y: i64, m: u32, d: u32) -> i64 {
        days_from_civil(y, m, d)
    }

    #[test]
    fn crate_step_aged_when_current_is_old() {
        let versions = vec![("1.0.0".to_string(), day(2026, 1, 1))]; // ~6 months old
        assert_eq!(crate_step("1.0.0", &versions, TODAY()), CrateStep::Aged);
    }

    #[test]
    fn crate_step_pins_fresh_to_latest_aged() {
        let versions = vec![
            ("1.4.0".to_string(), day(2026, 6, 1)), // aged
            ("1.5.0".to_string(), day(2026, 7, 14)), // fresh
        ];
        assert_eq!(
            crate_step("1.5.0", &versions, TODAY()),
            CrateStep::Pin("1.4.0".to_string())
        );
    }

    #[test]
    fn crate_step_dead_end_when_no_aged_version() {
        let versions = vec![
            ("2.0.0".to_string(), day(2026, 7, 10)), // fresh
            ("2.0.1".to_string(), day(2026, 7, 14)), // fresh
        ];
        assert_eq!(crate_step("2.0.1", &versions, TODAY()), CrateStep::DeadEnd);
    }

    #[test]
    fn crate_step_picks_highest_aged_below_current() {
        // A higher aged release (2.0.0) exists, but the locked
        // 1.5.0 is fresh; the pin must go *down* to 1.4.0, never
        // up to 2.0.0 (RT-1).
        let versions = vec![
            ("2.0.0".to_string(), day(2026, 6, 1)), // aged, higher
            ("1.4.0".to_string(), day(2026, 6, 1)), // aged, lower
            ("1.5.0".to_string(), day(2026, 7, 14)), // fresh, current
        ];
        assert_eq!(
            crate_step("1.5.0", &versions, TODAY()),
            CrateStep::Pin("1.4.0".to_string())
        );
    }

    #[test]
    fn crate_step_dead_end_not_upgrade_on_fresh_backport() {
        // The only aged version (2.0.0) sits *above* the fresh
        // locked backport (1.4.0). Pinning up would be a silent
        // upgrade, so this is a dead end -- not a Pin (RT-1).
        let versions = vec![
            ("2.0.0".to_string(), day(2026, 6, 1)), // aged, higher
            ("1.4.0".to_string(), day(2026, 7, 14)), // fresh, current
        ];
        assert_eq!(crate_step("1.4.0", &versions, TODAY()), CrateStep::DeadEnd);
    }

    #[test]
    fn crate_step_unknown_when_version_absent() {
        let versions = vec![("1.0.0".to_string(), day(2026, 1, 1))];
        assert_eq!(crate_step("9.9.9", &versions, TODAY()), CrateStep::Unknown);
    }

    #[test]
    fn run_clean_when_no_changes() {
        let same = lock(&[("serde", "1.0.100")]);
        let read = || Ok(same.clone());
        let base = || Some(same.clone());
        let fetch = |_: &str| -> Result<Vec<(String, i64)>, String> {
            panic!("must not fetch when nothing changed")
        };
        let pin = |_: &str, _: &str, _: &str| Ok(());
        let io = Io {
            read_lock: &read,
            baseline_lock: &base,
            fetch_versions: &fetch,
            pin: &pin,
            today: TODAY(),
        };
        let r = run(&io).unwrap();
        assert!(r.clean);
        assert!(r.pinned.is_empty());
        assert_eq!(r.iterations, 1);
    }

    #[test]
    fn run_pins_fresh_crate_then_converges() {
        // foo@1.5.0 is fresh; latest aged is 1.4.0. Pinning
        // rewrites the lock, and the next round sees it aged.
        let state = RefCell::new(lock(&[("foo", "1.5.0")]));
        let read = || Ok(state.borrow().clone());
        let base = || Some(lock(&[]));
        let fetch = |name: &str| -> Result<Vec<(String, i64)>, String> {
            assert_eq!(name, "foo");
            Ok(vec![
                ("1.4.0".to_string(), day(2026, 6, 1)),
                ("1.5.0".to_string(), day(2026, 7, 14)),
            ])
        };
        let pin = |name: &str, _cur: &str, target: &str| {
            assert_eq!(name, "foo");
            *state.borrow_mut() = lock(&[("foo", target)]);
            Ok(())
        };
        let io = Io {
            read_lock: &read,
            baseline_lock: &base,
            fetch_versions: &fetch,
            pin: &pin,
            today: TODAY(),
        };
        let r = run(&io).unwrap();
        assert!(r.clean);
        assert_eq!(r.pinned, vec!["foo: 1.5.0 -> 1.4.0".to_string()]);
        assert!(r.dead_ends.is_empty());
        assert_eq!(r.iterations, 2);
    }

    #[test]
    fn run_reports_dead_end_when_all_versions_fresh() {
        let cur = lock(&[("bar", "2.0.1")]);
        let read = || Ok(cur.clone());
        let base = || Some(lock(&[]));
        let fetch = |_: &str| {
            Ok(vec![
                ("2.0.0".to_string(), day(2026, 7, 10)),
                ("2.0.1".to_string(), day(2026, 7, 14)),
            ])
        };
        let pin = |_: &str, _: &str, _: &str| Ok(());
        let io = Io {
            read_lock: &read,
            baseline_lock: &base,
            fetch_versions: &fetch,
            pin: &pin,
            today: TODAY(),
        };
        let r = run(&io).unwrap();
        assert!(!r.clean);
        assert_eq!(r.dead_ends, vec!["bar@2.0.1".to_string()]);
        assert_eq!(r.iterations, 1);
    }

    #[test]
    fn run_treats_failed_pin_as_dead_end() {
        // An aged version exists but cargo refuses the pin (out
        // of the resolved requirement range) -> dead end, and
        // the run does not error.
        let cur = lock(&[("baz", "3.1.0")]);
        let read = || Ok(cur.clone());
        let base = || Some(lock(&[]));
        let fetch = |_: &str| {
            Ok(vec![
                ("3.0.0".to_string(), day(2026, 6, 1)), // aged
                ("3.1.0".to_string(), day(2026, 7, 14)), // fresh
            ])
        };
        let pin = |_: &str, _: &str, _: &str| Err("not in range".to_string());
        let io = Io {
            read_lock: &read,
            baseline_lock: &base,
            fetch_versions: &fetch,
            pin: &pin,
            today: TODAY(),
        };
        let r = run(&io).unwrap();
        assert!(!r.clean);
        assert_eq!(r.dead_ends, vec!["baz@3.1.0".to_string()]);
        assert!(r.warnings.iter().any(|w| w.contains("pin to 3.0.0 failed")));
    }

    #[test]
    fn run_unreachable_registry_is_unresolved_not_clean() {
        let cur = lock(&[("qux", "1.0.0")]);
        let read = || Ok(cur.clone());
        let base = || Some(lock(&[]));
        let fetch = |_: &str| -> Result<Vec<(String, i64)>, String> {
            Err("network down".to_string())
        };
        let pin = |_: &str, _: &str, _: &str| Ok(());
        let io = Io {
            read_lock: &read,
            baseline_lock: &base,
            fetch_versions: &fetch,
            pin: &pin,
            today: TODAY(),
        };
        let r = run(&io).unwrap();
        assert!(!r.clean);
        assert_eq!(r.unresolved, 1);
        assert!(r.dead_ends.is_empty());
        assert!(r.warnings.iter().any(|w| w.contains("network down")));
    }

    #[test]
    fn run_breaks_pin_cycle_without_reprinting() {
        // The pin "succeeds" but never actually ages the crate
        // (a stand-in for an oscillation where another crate
        // keeps forcing this one fresh). The applied-pins guard
        // must stop after the second round -- reporting a dead
        // end, with the pin listed exactly once, not spinning to
        // MAX_ITERS re-printing it (RT-2).
        let cur = lock(&[("loop", "1.0.0")]);
        let read = || Ok(cur.clone());
        let base = || Some(lock(&[]));
        let fetch = |_: &str| {
            Ok(vec![
                ("0.9.0".to_string(), day(2026, 6, 1)), // aged, lower
                ("1.0.0".to_string(), day(2026, 7, 14)), // fresh
            ])
        };
        // Pin reports success but leaves the lock unchanged.
        let pin = |_: &str, _: &str, _: &str| Ok(());
        let io = Io {
            read_lock: &read,
            baseline_lock: &base,
            fetch_versions: &fetch,
            pin: &pin,
            today: TODAY(),
        };
        let r = run(&io).unwrap();
        assert!(!r.clean);
        assert_eq!(r.pinned, vec!["loop: 1.0.0 -> 0.9.0".to_string()]);
        assert_eq!(r.dead_ends, vec!["loop@1.0.0".to_string()]);
        assert_eq!(r.iterations, 2);
    }

    #[test]
    fn run_errors_without_baseline() {
        let cur = lock(&[]);
        let read = || Ok(cur.clone());
        let base = || None;
        let fetch = |_: &str| Ok(Vec::new());
        let pin = |_: &str, _: &str, _: &str| Ok(());
        let io = Io {
            read_lock: &read,
            baseline_lock: &base,
            fetch_versions: &fetch,
            pin: &pin,
            today: TODAY(),
        };
        let err = run(&io).unwrap_err();
        assert!(err.contains("no HEAD baseline"));
    }
}
