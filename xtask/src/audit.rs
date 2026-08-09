//! `cargo xtask audit` -- security-advisory gate.
//!
//! Runs `cargo audit` (RUSTSEC) over `Cargo.lock` and fails
//! on any *vulnerability*. Advisory *warnings*
//! (unsound / unmaintained / yanked) are reported but do
//! not fail the gate -- they are informational, not
//! exploitable defects.
//!
//! This is a Rust-only project, so there is no second
//! ecosystem to audit. The tool reaches the network (the
//! RUSTSEC advisory DB). A *positive vulnerability finding*
//! is always fatal, but a connectivity / missing-tool
//! failure is surfaced as a non-fatal **warning** in the
//! `validate` context (so an offline machine or fresh CI run
//! is not blocked by a transient outage) while the
//! standalone `cargo xtask audit` still errors on it. Pure
//! JSON parsing and the fatal-vs-warning classification are
//! split out and unit-tested; the subprocess spawns are
//! thin.

use serde_json::Value;

use crate::helpers::run_cargo_capture;

/// Parsed `cargo audit --json` result.
#[derive(Debug, PartialEq, Eq)]
struct CargoAudit {
    /// Number of vulnerabilities (fatal).
    vulnerabilities: u64,
    /// Number of advisory warnings (informational).
    warnings: u64,
}

/// Audit outcome for use by validate.
pub struct AuditResult {
    /// Human-readable detail for the step line.
    pub detail: String,
    /// Error message -- `Some` **only** on a positive
    /// vulnerability finding (always fatal).
    pub error: Option<String>,
    /// Non-fatal problems (a tool missing, the network /
    /// advisory DB unreachable, an unparseable response).
    /// A warning in `validate`; an error for the standalone
    /// command.
    pub warnings: Vec<String>,
}

/// Parse the `vulnerabilities.count` and total `warnings`
/// from `cargo audit --json` stdout.
fn parse_cargo_audit(stdout: &str) -> Result<CargoAudit, String> {
    let j: Value = serde_json::from_str(stdout)
        .map_err(|e| format!("failed to parse cargo audit JSON: {e}"))?;
    let vulnerabilities = j["vulnerabilities"]["count"]
        .as_u64()
        .ok_or("missing vulnerabilities.count in cargo audit output")?;
    // `warnings` is an object keyed by kind (unsound,
    // unmaintained, yanked, ...), each a list. Sum the lists.
    let warnings = j["warnings"].as_object().map_or(0, |o| {
        o.values()
            .map(|v| v.as_array().map_or(0, Vec::len) as u64)
            .sum()
    });
    Ok(CargoAudit {
        vulnerabilities,
        warnings,
    })
}

/// Run `cargo audit --json`. Distinguishes "cargo-audit not
/// installed" (an actionable install hint) from a real
/// parse of the advisory results.
fn run_cargo_audit() -> Result<CargoAudit, String> {
    let output = run_cargo_capture(&["audit", "--json"])?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("no such") || stderr.contains("not installed") {
            return Err("cargo-audit is not installed -- \
                 install with: cargo install cargo-audit"
                .into());
        }
        return Err(format!("cargo audit produced no output:\n{stderr}"));
    }
    parse_cargo_audit(&stdout)
}

/// Run the security audit and classify the outcome.
pub fn audit_check() -> AuditResult {
    classify_audit(run_cargo_audit())
}

/// Classify the audit outcome: a positive vulnerability
/// count is fatal (`error`); a runner `Err` (missing tool /
/// unreachable network / unparseable output) is a non-fatal
/// `warning`. Pure, so the fatal-vs-warning boundary is
/// unit-tested.
fn classify_audit(cargo: Result<CargoAudit, String>) -> AuditResult {
    let mut parts: Vec<String> = Vec::new();
    let mut fatal: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    match cargo {
        Ok(c) => {
            if c.vulnerabilities > 0 {
                fatal.push(format!("{} RUSTSEC", c.vulnerabilities));
            }
            parts.push(format!(
                "cargo: {} vuln, {} warn",
                c.vulnerabilities, c.warnings
            ));
        }
        Err(reason) => {
            warnings.push(format!("cargo audit unavailable: {reason}"));
            parts.push("cargo: unavailable".into());
        }
    }

    AuditResult {
        detail: parts.join(", "),
        error: if fatal.is_empty() {
            None
        } else {
            Some(format!("vulnerabilities found: {}", fatal.join("; ")))
        },
        warnings,
    }
}

/// Standalone `cargo xtask audit` entry point. Unlike the
/// `validate` step, this errors when the audit could not
/// complete (a human who ran it explicitly wants to know).
pub fn audit() -> Result<(), String> {
    let r = audit_check();
    if let Some(err) = r.error {
        eprintln!("  {}", r.detail);
        return Err(err);
    }
    for w in &r.warnings {
        eprintln!("  warning: {w}");
    }
    if r.warnings.is_empty() {
        println!("Audit OK ({})", r.detail);
        Ok(())
    } else {
        Err("audit could not complete (see warnings above)".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cargo_audit_clean() {
        let json = r#"{"vulnerabilities":{"found":false,"count":0},
            "warnings":{}}"#;
        assert_eq!(
            parse_cargo_audit(json).unwrap(),
            CargoAudit {
                vulnerabilities: 0,
                warnings: 0
            }
        );
    }

    #[test]
    fn parse_cargo_audit_counts_warnings_across_kinds() {
        let json = r#"{"vulnerabilities":{"found":false,"count":0},
            "warnings":{"unsound":[{"a":1}],
            "unmaintained":[{"b":2},{"c":3}]}}"#;
        let r = parse_cargo_audit(json).unwrap();
        assert_eq!(r.vulnerabilities, 0);
        assert_eq!(r.warnings, 3);
    }

    #[test]
    fn parse_cargo_audit_with_vulns() {
        let json = r#"{"vulnerabilities":{"found":true,"count":2},
            "warnings":{}}"#;
        assert_eq!(parse_cargo_audit(json).unwrap().vulnerabilities, 2);
    }

    #[test]
    fn parse_cargo_audit_rejects_garbage() {
        assert!(parse_cargo_audit("not json").is_err());
    }

    #[test]
    fn classify_advisory_warnings_are_reported_not_fatal() {
        // Unsound / unmaintained / yanked are informational:
        // they reach `detail` but must not fail the gate.
        let r = classify_audit(Ok(CargoAudit {
            vulnerabilities: 0,
            warnings: 1,
        }));
        assert!(r.error.is_none());
        assert!(r.warnings.is_empty());
        assert!(r.detail.contains("cargo: 0 vuln, 1 warn"));
    }

    #[test]
    fn classify_cargo_vuln_is_fatal() {
        let r = classify_audit(Ok(CargoAudit {
            vulnerabilities: 2,
            warnings: 0,
        }));
        assert!(r.error.as_ref().unwrap().contains("2 RUSTSEC"));
    }

    #[test]
    fn classify_vuln_is_fatal_and_still_reports_warnings() {
        // The two counts are independent: a fatal
        // vulnerability must not suppress the advisory count
        // in the step detail.
        let r = classify_audit(Ok(CargoAudit {
            vulnerabilities: 1,
            warnings: 3,
        }));
        assert!(r.error.as_ref().unwrap().contains("1 RUSTSEC"));
        assert!(r.detail.contains("1 vuln, 3 warn"));
    }

    #[test]
    fn classify_unavailable_is_warning_not_fatal() {
        // A runner Err (network/tool) must NOT be fatal: an
        // offline machine warns, it does not fail the gate.
        let r = classify_audit(Err("network down".into()));
        assert!(r.error.is_none());
        assert_eq!(r.warnings.len(), 1);
        assert!(r.detail.contains("cargo: unavailable"));
    }
}
