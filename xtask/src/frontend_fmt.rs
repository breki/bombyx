//! Runs Prettier against the frontend (via the `format` /
//! `format:check` npm scripts). Auto-fixes formatting in
//! place by default; read-only under `--check` (for CI or
//! before partial staging), mirroring the Rust `fmt` gate.

use crate::frontend::{FrontendResult, report_cmd, run_npm_script};

/// Run Prettier. `check` selects the read-only
/// `format:check`; otherwise `format` auto-fixes in place.
pub fn frontend_fmt(check: bool) -> Result<FrontendResult, String> {
    if check {
        run_npm_script("format:check", "", "prettier found unformatted files")
    } else {
        run_npm_script("format", "", "prettier failed")
    }
}

/// Standalone `cargo xtask frontend-fmt [--check]`.
pub fn frontend_fmt_cmd(check: bool) -> Result<(), String> {
    report_cmd(frontend_fmt(check)?, "Frontend fmt")
}
