//! Runs the frontend unit suite (Vitest, via the `test` npm
//! script). Skipped gracefully when the frontend is absent;
//! errors when it exists but `node_modules` is not installed.
//!
//! `svelte-check` (frontend-check) only type-checks -- it
//! does not run component tests -- so without this gate a
//! broken assertion or a runtime error in a `.test` file
//! passes `validate` unnoticed.

use crate::frontend::{FrontendResult, report_cmd, run_npm_script};

/// Run `npm run test` (vitest) in `frontend/`.
pub fn frontend_test() -> Result<FrontendResult, String> {
    run_npm_script("test", "", "frontend unit tests failed")
}

/// Standalone `cargo xtask frontend-test` entry point.
pub fn frontend_test_cmd() -> Result<(), String> {
    report_cmd(frontend_test()?, "Frontend test")
}
