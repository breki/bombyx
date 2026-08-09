//! Shared helpers for the frontend xtask wrappers
//! (`frontend-check`, `frontend-fmt`, `frontend-dupes`).
//!
//! Each runs an npm script in `frontend/`. All skip cleanly
//! (pass) when there is genuinely no frontend, but error --
//! never silently skip -- when the frontend exists with its
//! `node_modules` not installed, so a missing install can
//! never be mistaken for a pass.

use std::path::Path;
use std::process::Command;

/// Maximum lines of failing script output to echo.
const MAX_OUTPUT_LINES: usize = 10;

/// Outcome of a frontend npm-script run.
pub struct FrontendResult {
    /// Human-readable detail for the step line.
    pub detail: String,
    /// Error message if the script failed.
    pub error: Option<String>,
    /// True when the run was skipped (no frontend).
    pub skipped: bool,
}

/// What to do about the frontend, given what is on disk.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    /// No frontend in this project (CLI-only). Skip, pass.
    Skip,
    /// Frontend exists but its deps are not installed --
    /// error, not skip (a skip that exits 0 reads as a pass).
    NotInstalled,
    /// Frontend is present and installed. Run the script.
    Run,
}

/// Decide the action from the two on-disk facts. Pure, so
/// the skip-vs-error boundary is unit-tested without npm.
fn classify(has_package_json: bool, has_node_modules: bool) -> Action {
    match (has_package_json, has_node_modules) {
        (false, _) => Action::Skip,
        (true, false) => Action::NotInstalled,
        (true, true) => Action::Run,
    }
}

/// Run `npm run <script>` in `frontend/`.
///
/// On success returns `ok_detail` as the step detail; on a
/// non-zero exit, echoes the first several output lines and
/// returns `err_msg` as the error.
pub fn run_npm_script(
    script: &str,
    ok_detail: &str,
    err_msg: &str,
) -> Result<FrontendResult, String> {
    let has_pkg = Path::new("frontend/package.json").exists();
    let has_modules = Path::new("frontend/node_modules").exists();
    match classify(has_pkg, has_modules) {
        Action::Skip => {
            return Ok(FrontendResult {
                detail: "no frontend".into(),
                error: None,
                skipped: true,
            });
        }
        Action::NotInstalled => {
            return Err("frontend/ exists but node_modules is \
                 missing -- run `npm --prefix frontend install`"
                .into());
        }
        Action::Run => {}
    }

    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let output = Command::new(npm)
        .args(["--prefix", "frontend", "run", script])
        .output()
        .map_err(|e| format!("failed to run {npm}: {e}"))?;

    if output.status.success() {
        Ok(FrontendResult {
            detail: ok_detail.to_string(),
            error: None,
            skipped: false,
        })
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}{stderr}");
        for line in combined.lines().take(MAX_OUTPUT_LINES) {
            eprintln!("  {line}");
        }
        Ok(FrontendResult {
            detail: String::new(),
            error: Some(err_msg.to_string()),
            skipped: false,
        })
    }
}

/// Print a standalone-command result and map it to the
/// `Result` a subcommand returns. Shared by every
/// `frontend-*` command so the skipped / OK / error shape
/// stays identical across them.
pub fn report_cmd(r: FrontendResult, label: &str) -> Result<(), String> {
    match r.error {
        None if r.skipped => {
            println!("{label} skipped: {}", r.detail);
            Ok(())
        }
        None => {
            println!("{label} OK");
            Ok(())
        }
        Some(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_package_json_skips() {
        assert_eq!(classify(false, false), Action::Skip);
        assert_eq!(classify(false, true), Action::Skip);
    }

    #[test]
    fn package_json_without_modules_is_error() {
        assert_eq!(classify(true, false), Action::NotInstalled);
    }

    #[test]
    fn installed_frontend_runs() {
        assert_eq!(classify(true, true), Action::Run);
    }
}
