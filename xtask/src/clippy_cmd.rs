use crate::helpers::run_cargo_capture;

/// Clippy argument list, shared between standalone and
/// check modes.
const CLIPPY_ARGS: &[&str] = &[
    "clippy",
    "--workspace",
    "--all-targets",
    "--",
    "-D",
    "warnings",
];

/// Maximum number of warning lines to display.
const MAX_WARNING_LINES: usize = 10;

/// Run clippy with concise output.
///
/// Prints `Clippy OK` on success or `FAILED` with
/// the first few warning/error lines on failure.
pub fn clippy() -> Result<(), String> {
    let r = clippy_check()?;
    match r.error {
        None => {
            println!("Clippy OK");
            Ok(())
        }
        Some(err) => {
            eprintln!("FAILED: clippy warning(s)\n");
            for line in r.items.iter().take(MAX_WARNING_LINES) {
                eprintln!("  {line}");
            }
            if r.items.len() > MAX_WARNING_LINES {
                eprintln!(
                    "  ... and {} more",
                    r.items.len() - MAX_WARNING_LINES
                );
            }
            Err(err)
        }
    }
}

/// Result from a clippy run, for use by validate.
pub struct ClippyResult {
    /// The warning/error lines (empty on success).
    pub items: Vec<String>,
    /// Error message if failed.
    pub error: Option<String>,
}

/// Run clippy and return structured result without
/// printing.
pub fn clippy_check() -> Result<ClippyResult, String> {
    let output = run_cargo_capture(CLIPPY_ARGS)?;

    if output.status.success() {
        return Ok(ClippyResult {
            items: vec![],
            error: None,
        });
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let items: Vec<String> = extract_warning_lines(&stderr)
        .into_iter()
        .map(String::from)
        .collect();

    Ok(ClippyResult {
        error: Some("clippy warning(s)".into()),
        items,
    })
}

/// Extract clippy diagnostics from stderr, filtering out
/// cargo/rustc summary noise, and keep the `-->`
/// source-location line that immediately follows each
/// diagnostic.
///
/// Captures three diagnostic shapes: `warning:`, `error[`
/// (coded errors), and bare `error:` -- the last is how a
/// *denied* lint is reported under `-D warnings`
/// (no `[Exxx]` code), so dropping it left a clippy failure
/// with an empty body. Without the paired `-->` line a
/// failure names the lint but not where it fired, forcing a
/// raw `cargo clippy` re-run.
fn extract_warning_lines(stderr: &str) -> Vec<&str> {
    crate::helpers::pair_with_locations(stderr, is_diagnostic_line)
}

/// True for a real clippy diagnostic line, false for the
/// cargo/rustc summary lines that share the same prefix.
fn is_diagnostic_line(line: &str) -> bool {
    // Summary noise. `") generated "` is anchored to its
    // grammar (`crate (target) generated N warnings`) so a
    // real lint message merely containing the word
    // "generated" is not dropped.
    let is_summary = line.contains("could not compile")
        || line.contains("aborting due to")
        || line.contains("build failed")
        || line.contains(") generated ")
        || line.contains(" warning emitted")
        || line.contains(" warnings emitted");
    if is_summary {
        return false;
    }
    line.starts_with("warning:")
        || line.starts_with("error[")
        || line.starts_with("error:")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_STDERR: &str = "\
warning: used `sort` on primitive type `str`
    --> crates/rustbase/src/lib.rs:10:9
warning: `rustbase` (bin \"rustbase\" test) \
generated 1 warning (1 duplicate)
error[E0425]: cannot find value `x`
    --> crates/rustbase/src/lib.rs:10:5
error: could not compile `rustbase`
warning: build failed, waiting for other jobs
warning: 2 warnings emitted";

    #[test]
    fn extracts_warnings_errors_and_locations() {
        let lines = extract_warning_lines(SAMPLE_STDERR);
        // Two diagnostics, each with its `-->` location; the
        // `) generated `, `could not compile`, `build
        // failed`, and `warnings emitted` summary lines are
        // filtered.
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("sort"));
        assert!(lines[1].contains("--> crates/rustbase/src/lib.rs:10:9"));
        assert!(lines[2].contains("E0425"));
        assert!(lines[3].contains("--> crates/rustbase/src/lib.rs:10:5"));
    }

    #[test]
    fn keeps_denied_lint_bare_error_with_location() {
        // A denied lint under `-D warnings` is reported as a
        // bare `error:` with no `[Exxx]` code. Both the
        // message and its location line must survive; the
        // `aborting due to` summary must not.
        let stderr = "\
error: this function has too many lines (111/100)
    --> crates/rustbase/src/big.rs:5:1
error: aborting due to 1 previous error";
        let lines = extract_warning_lines(stderr);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("too many lines"));
        assert!(lines[1].contains("--> crates/rustbase/src/big.rs:5:1"));
    }

    #[test]
    fn keeps_lint_message_containing_word_generated() {
        // Anchored `) generated ` filter must not drop a real
        // lint whose message merely contains "generated".
        let stderr = "warning: value assigned here is never \
            generated again";
        let lines = extract_warning_lines(stderr);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn empty_input() {
        let lines = extract_warning_lines("");
        assert!(lines.is_empty());
    }

    #[test]
    fn clean_output_gives_empty() {
        let stderr = "    Checking rustbase v0.2.1\n\
            Finished `dev` profile";
        let lines = extract_warning_lines(stderr);
        assert!(lines.is_empty());
    }
}
