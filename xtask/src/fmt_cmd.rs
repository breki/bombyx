use crate::helpers::{run_cargo_capture, run_cargo_stream};

/// Run `cargo fmt --all`.
pub fn fmt() -> Result<(), String> {
    run_cargo_stream(&["fmt", "--all"])
}

/// Check formatting with concise output.
///
/// Returns `Ok(())` on success or an error with details
/// on failure.
pub fn fmt_check() -> Result<(), String> {
    let output = run_cargo_capture(&["fmt", "--all", "--", "--check"])?;

    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("FAILED: formatting issues\n");
    for line in stdout.lines().take(20) {
        eprintln!("  {line}");
    }
    let total = stdout.lines().count();
    if total > 20 {
        eprintln!("  ... and {} more lines", total - 20);
    }
    Err("formatting issues".into())
}
