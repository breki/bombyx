//! `cargo xtask deny` -- the licence gate.
//!
//! Runs `cargo deny check licenses bans sources` over `Cargo.lock`,
//! configured by `deny.toml` at the workspace root.
//!
//! **Offline, and that is the point.** These three checks read the
//! lockfile and the crate metadata already on disk; none of them
//! reaches the network. So unlike [`crate::audit`] this can run on
//! every push without failing for reasons unrelated to the commit.
//! Advisories stay with `cargo-audit`, which does need the RUSTSEC
//! database.
//!
//! A missing `cargo-deny` is an **error**, not a warning. The
//! contrast with `audit` is deliberate: `audit`'s leniency buys an
//! offline laptop the ability to finish `validate`, and it costs the
//! guarantee that the advisory DB was consulted. Here there is
//! nothing to trade -- the check needs no network, so a machine that
//! cannot run it is simply missing a tool, and saying so is more use
//! than passing quietly.

use crate::helpers::run_cargo_capture;

/// What `cargo deny` reported.
///
/// Three named outcomes rather than a bool and a string, because a
/// missing tool must never be spelled as a failure that happens to
/// carry install text in its detail field -- that shape let the
/// distinction live in a string comparison.
#[derive(Debug, PartialEq, Eq)]
enum DenyResult {
    /// Every check passed.
    Passed,
    /// A check failed; the string is the tool's own output, which
    /// can be empty when cargo-deny fails and prints nothing.
    Failed(String),
    /// `cargo-deny` is not installed.
    ToolMissing,
}

/// What to tell the operator when cargo-deny is not installed.
const MISSING_TOOL: &str =
    "cargo-deny is not installed: cargo install --locked cargo-deny";

/// Classifies a `cargo deny` run.
///
/// Split from the subprocess so the "missing tool" wording is
/// unit-tested rather than only reachable by uninstalling cargo-deny.
fn classify(status_ok: bool, stderr: &str) -> DenyResult {
    // cargo itself reports an absent subcommand this way, and the
    // message is unhelpful on its own -- it names `deny` without
    // saying how to get it. Checked before `status_ok`, because a
    // missing tool must never come back as a pass: there is no
    // network to be down here, unlike in `audit`.
    if crate::helpers::is_missing_subcommand(stderr) {
        return DenyResult::ToolMissing;
    }
    if status_ok {
        DenyResult::Passed
    } else {
        DenyResult::Failed(stderr.trim().to_owned())
    }
}

/// Turns an outcome into the error the caller returns, or `None`
/// when the gate passed.
///
/// Split out for the same reason [`classify`] is: the branch it
/// exists for -- cargo-deny failing while printing nothing -- sits
/// behind the subprocess and is otherwise unreachable from a test,
/// and a blocked gate reporting an empty reason costs a whole
/// re-run to diagnose.
fn failure(r: DenyResult, status: &str) -> Option<String> {
    match r {
        DenyResult::Passed => None,
        DenyResult::Failed(d) if d.is_empty() => {
            Some(format!("cargo deny failed ({status}) and printed nothing"))
        }
        DenyResult::Failed(d) => Some(d),
        DenyResult::ToolMissing => Some(MISSING_TOOL.to_owned()),
    }
}

/// Runs the gate.
///
/// # Errors
///
/// Returns the tool's output when a check fails or the tool is
/// missing.
pub fn deny() -> Result<(), String> {
    let out = run_cargo_capture(&[
        "deny",
        // `--offline` is what makes the "reads what is already on
        // disk" claim true rather than asserted. Without it
        // cargo-deny resolves and *fetches* the tree: measured at
        // 166 MB into an empty CARGO_HOME. The whole reason this
        // gate runs on every push, where `audit` does not, is that
        // it needs no network -- so the flag is the guarantee.
        "--offline",
        "--locked",
        "--all-features",
        "check",
        "licenses",
        "bans",
        "sources",
    ])?;
    // Both streams. cargo-deny puts its diagnostics on stderr, but a
    // malformed `deny.toml` or another version's wording can land on
    // stdout, and a blocked gate reporting an empty reason costs a
    // whole re-run to diagnose.
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let both = format!("{}\n{}", stderr.trim(), stdout.trim());
    let r = classify(out.status.success(), &both);
    match failure(r, &out.status.to_string()) {
        None => {
            println!("Deny OK (licenses, bans, sources)");
            Ok(())
        }
        Some(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_passing_run_is_ok() {
        assert_eq!(classify(true, ""), DenyResult::Passed);
        assert_eq!(failure(DenyResult::Passed, "exit code: 0"), None);
    }

    #[test]
    fn a_failing_check_carries_the_tools_output() {
        let r = classify(false, "error[L001]: failed to satisfy license");
        assert!(
            matches!(&r, DenyResult::Failed(d) if d.contains("L001")),
            "{r:?}"
        );
    }

    #[test]
    fn a_missing_tool_says_how_to_install_it() {
        // cargo's own wording, which names the subcommand and
        // nothing else. The replacement has to be actionable.
        let r = classify(false, "error: no such command: `deny`");
        assert_eq!(r, DenyResult::ToolMissing);
        let e = failure(r, "exit code: 101").unwrap();
        assert!(e.contains("cargo install"), "{e}");
        assert!(e.contains("cargo-deny"), "{e}");
    }

    #[test]
    fn a_missing_tool_is_never_reported_as_a_pass() {
        // The distinction from `audit`, asserted: there is no
        // network to be down here, so a missing tool cannot be
        // waved through -- even when the exit status says success.
        assert_eq!(
            classify(true, "error: no such command: `deny`"),
            DenyResult::ToolMissing
        );
    }

    #[test]
    fn a_silent_failure_still_names_the_exit_status() {
        // The branch this test exists for: cargo-deny fails and
        // prints nothing on either stream. Reporting an empty
        // reason there costs a whole re-run to diagnose, so the
        // status is the one fact available and it has to appear.
        let e = failure(DenyResult::Failed(String::new()), "exit code: 2")
            .expect("a failure must produce a message");
        assert!(e.contains("exit code: 2"), "{e}");
        assert!(e.contains("printed nothing"), "{e}");
    }
}
