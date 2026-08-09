//! `cargo xtask audit` -- security-advisory gate.
//!
//! Runs `cargo audit` (RUSTSEC) over `Cargo.lock` and, when
//! a frontend is present, `npm audit` over it, and fails on
//! any *vulnerability*. Advisory *warnings*
//! (unsound / unmaintained / yanked) are reported but do
//! not fail the gate -- they are informational, not
//! exploitable defects.
//!
//! Both tools reach the network (the RUSTSEC advisory DB
//! and the npm registry). A *positive vulnerability finding*
//! is always fatal, but a connectivity / missing-tool
//! failure is surfaced as a non-fatal **warning** in the
//! `validate` context (so an offline machine or fresh CI run
//! is not blocked by a transient outage) while the
//! standalone `cargo xtask audit` still errors on it. Pure
//! JSON parsing and the fatal-vs-warning classification are
//! split out and unit-tested; the subprocess spawns are
//! thin.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::helpers::run_cargo_capture;

/// Parsed `cargo audit --json` result.
#[derive(Debug, PartialEq, Eq)]
pub struct CargoAudit {
    /// Number of vulnerabilities (fatal).
    pub vulnerabilities: u64,
    /// Number of advisory warnings (informational).
    pub warnings: u64,
}

/// Parsed `npm audit --json` result.
#[derive(Debug, PartialEq, Eq)]
pub struct NpmAudit {
    /// Total vulnerabilities across all severities.
    pub total: u64,
    /// High-severity count (for the summary line).
    pub high: u64,
    /// Critical-severity count (for the summary line).
    pub critical: u64,
}

/// Combined audit outcome for use by validate.
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

/// Parse `metadata.vulnerabilities` from `npm audit --json`.
fn parse_npm_audit(stdout: &str) -> Result<NpmAudit, String> {
    let j: Value = serde_json::from_str(stdout)
        .map_err(|e| format!("failed to parse npm audit JSON: {e}"))?;
    let v = &j["metadata"]["vulnerabilities"];
    let get = |k: &str| v[k].as_u64().unwrap_or(0);
    let total = v["total"]
        .as_u64()
        .ok_or("missing metadata.vulnerabilities.total in npm audit output")?;
    Ok(NpmAudit {
        total,
        high: get("high"),
        critical: get("critical"),
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

/// Run `npm audit --json` in `frontend/`.
fn run_npm_audit() -> Result<NpmAudit, String> {
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let output = Command::new(npm)
        .args(["--prefix", "frontend", "audit", "--json"])
        .output()
        .map_err(|e| format!("failed to run {npm}: {e}"))?;
    // npm audit exits non-zero when vulnerabilities exist,
    // but still writes the JSON report to stdout -- so parse
    // regardless of the exit status.
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_npm_audit(&stdout)
}

/// Run the combined security audit. `cargo audit` always
/// runs; `npm audit` runs only when `frontend/package.json`
/// exists. `Ok`/`Err` from the runners is fed to the pure
/// [`classify_audit`], which decides fatal-vs-warning.
pub fn audit_check() -> AuditResult {
    let cargo = run_cargo_audit();
    let npm = if Path::new("frontend/package.json").exists() {
        Some(run_npm_audit())
    } else {
        None
    };
    classify_audit(cargo, npm)
}

/// Classify per-source audit outcomes into the combined
/// result: a positive vulnerability count is fatal
/// (`error`); a runner `Err` (missing tool / unreachable
/// network / unparseable output) is a non-fatal `warning`.
/// Pure, so the fatal-vs-warning boundary is unit-tested.
pub fn classify_audit(
    cargo: Result<CargoAudit, String>,
    npm: Option<Result<NpmAudit, String>>,
) -> AuditResult {
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

    if let Some(npm) = npm {
        match npm {
            Ok(n) => {
                if n.total > 0 {
                    fatal.push(format!(
                        "{} npm ({} high, {} critical)",
                        n.total, n.high, n.critical
                    ));
                }
                parts.push(format!("npm: {} vuln", n.total));
            }
            Err(reason) => {
                warnings.push(format!("npm audit unavailable: {reason}"));
                parts.push("npm: unavailable".into());
            }
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
    fn parse_npm_audit_clean() {
        let json = r#"{"metadata":{"vulnerabilities":
            {"info":0,"low":0,"moderate":0,"high":0,
             "critical":0,"total":0}}}"#;
        assert_eq!(
            parse_npm_audit(json).unwrap(),
            NpmAudit {
                total: 0,
                high: 0,
                critical: 0
            }
        );
    }

    #[test]
    fn parse_npm_audit_with_vulns() {
        let json = r#"{"metadata":{"vulnerabilities":
            {"info":0,"low":1,"moderate":2,"high":3,
             "critical":1,"total":7}}}"#;
        let r = parse_npm_audit(json).unwrap();
        assert_eq!(r.total, 7);
        assert_eq!(r.high, 3);
        assert_eq!(r.critical, 1);
    }

    #[test]
    fn parse_npm_audit_missing_total_errors() {
        let json = r#"{"metadata":{"vulnerabilities":{}}}"#;
        assert!(parse_npm_audit(json).is_err());
    }

    fn clean_cargo() -> CargoAudit {
        CargoAudit {
            vulnerabilities: 0,
            warnings: 1,
        }
    }

    #[test]
    fn classify_clean_no_error_no_warnings() {
        let r = classify_audit(Ok(clean_cargo()), None);
        assert!(r.error.is_none());
        assert!(r.warnings.is_empty());
        assert!(r.detail.contains("cargo: 0 vuln, 1 warn"));
    }

    #[test]
    fn classify_cargo_vuln_is_fatal() {
        let r = classify_audit(
            Ok(CargoAudit {
                vulnerabilities: 2,
                warnings: 0,
            }),
            None,
        );
        assert!(r.error.as_ref().unwrap().contains("2 RUSTSEC"));
    }

    #[test]
    fn classify_npm_vuln_is_fatal() {
        let r = classify_audit(
            Ok(clean_cargo()),
            Some(Ok(NpmAudit {
                total: 3,
                high: 2,
                critical: 1,
            })),
        );
        assert!(r.error.as_ref().unwrap().contains("3 npm"));
    }

    #[test]
    fn classify_unavailable_is_warning_not_fatal() {
        // A runner Err (network/tool) must NOT be fatal --
        // this is the RT-1 fix: an offline machine warns, it
        // does not fail the gate.
        let r = classify_audit(
            Err("network down".into()),
            Some(Err("registry unreachable".into())),
        );
        assert!(r.error.is_none());
        assert_eq!(r.warnings.len(), 2);
        assert!(r.detail.contains("cargo: unavailable"));
    }

    #[test]
    fn classify_vuln_fatal_even_with_other_source_unavailable() {
        let r = classify_audit(
            Ok(CargoAudit {
                vulnerabilities: 1,
                warnings: 0,
            }),
            Some(Err("registry unreachable".into())),
        );
        assert!(r.error.is_some());
        assert_eq!(r.warnings.len(), 1);
    }
}
