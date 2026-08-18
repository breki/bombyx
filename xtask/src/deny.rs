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
#[derive(Debug, PartialEq, Eq)]
pub struct DenyResult {
    /// True when every check passed.
    pub ok: bool,
    /// The tool's own output, for the failure message.
    pub detail: String,
}

/// Classifies a `cargo deny` run.
///
/// Split from the subprocess so the "missing tool" wording is
/// unit-tested rather than only reachable by uninstalling cargo-deny.
#[must_use]
pub fn classify(status_ok: bool, stderr: &str) -> DenyResult {
    // cargo itself reports an absent subcommand this way, and the
    // message is unhelpful on its own -- it names `deny` without
    // saying how to get it.
    if crate::helpers::is_missing_subcommand(stderr) {
        return DenyResult {
            ok: false,
            detail: "cargo-deny is not installed: \
                     cargo install --locked cargo-deny"
                .to_owned(),
        };
    }
    DenyResult {
        ok: status_ok,
        detail: stderr.trim().to_owned(),
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
    if r.ok {
        println!("Deny OK (licenses, bans, sources)");
        return Ok(());
    }
    if r.detail.is_empty() {
        return Err(format!(
            "cargo deny failed ({}) and printed nothing",
            out.status
        ));
    }
    Err(r.detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_passing_run_is_ok() {
        assert!(classify(true, "").ok);
    }

    #[test]
    fn a_failing_check_carries_the_tools_output() {
        let r = classify(false, "error[L001]: failed to satisfy license");
        assert!(!r.ok);
        assert!(r.detail.contains("L001"), "{}", r.detail);
    }

    #[test]
    fn a_missing_tool_says_how_to_install_it() {
        // cargo's own wording, which names the subcommand and
        // nothing else. The replacement has to be actionable.
        let r = classify(false, "error: no such command: `deny`");
        assert!(!r.ok);
        assert!(r.detail.contains("cargo install"), "{}", r.detail);
        assert!(r.detail.contains("cargo-deny"), "{}", r.detail);
    }

    #[test]
    fn a_missing_tool_is_never_reported_as_a_pass() {
        // The distinction from `audit`, asserted: there is no
        // network to be down here, so a missing tool cannot be
        // waved through.
        assert!(!classify(true, "error: no such command: `deny`").ok);
    }
}
