//! Gates frontend code duplication with jscpd (via the
//! `dupes` npm script), mirroring the Rust `dupes` gate.
//!
//! jscpd self-enforces the threshold from
//! `frontend/.jscpd.json` (it exits non-zero when
//! duplication exceeds it), so this wrapper only maps the
//! exit status -- unlike the Rust `dupes.rs`, no JSON is
//! parsed here.

use crate::frontend::{
    FrontendResult, report_cmd, run_npm_script, step_result,
};
use rustbase_xtask::step::Step;

/// The `validate` step.
pub fn step() -> Step {
    Step::new("Frontend dupes", "frontend-dupes", || {
        step_result(frontend_dupes()?)
    })
}

/// Run jscpd over the frontend sources.
pub fn frontend_dupes() -> Result<FrontendResult, String> {
    run_npm_script(
        "dupes",
        "<= threshold",
        "frontend duplication exceeds threshold",
    )
}

/// Standalone `cargo xtask frontend-dupes` entry point.
pub fn frontend_dupes_cmd() -> Result<(), String> {
    report_cmd(frontend_dupes()?, "Frontend dupes")
}
