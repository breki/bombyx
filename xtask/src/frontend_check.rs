//! Runs `svelte-check` against the frontend TypeScript +
//! Svelte sources (via the `check` npm script). Skipped
//! gracefully when the frontend is absent (the template's
//! web crate is optional); errors when the frontend exists
//! but its `node_modules` is not installed.

use crate::frontend::{FrontendResult, report_cmd, run_npm_script};

/// Run `npm run check` in `frontend/`.
pub fn frontend_check() -> Result<FrontendResult, String> {
    run_npm_script("check", "", "svelte-check reported errors")
}

/// Standalone `cargo xtask frontend-check` entry point.
pub fn frontend_check_cmd() -> Result<(), String> {
    report_cmd(frontend_check()?, "Frontend check")
}
