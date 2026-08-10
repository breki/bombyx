use crate::helpers::{pair_with_locations, run_cargo_capture_env};

/// Maximum number of diagnostic lines to display.
const MAX_DIAGNOSTIC_LINES: usize = 10;

/// One rustdoc pass, and what it is able to see.
///
/// Two passes, because a broken doc link fails in one of two
/// ways and **neither pass catches both**. This was measured
/// rather than assumed:
///
/// - A link inside a *private* module that names something not in
///   scope. Breaking one in `bombyx`'s `doctor::text` produced
///   **0** errors in the public pass and **2** in the private one,
///   because the public pass never renders a private module's
///   docs at all.
/// - A *public* page linking to a private item. That is an error
///   in the public pass and perfectly legal in the private one --
///   rustdoc even suggests passing `--document-private-items` to
///   make it resolve.
///
/// So the two are not degrees of strictness; each sees a class the
/// other cannot. Running one while calling it a doc-link gate
/// would be worse than running neither, because it would look
/// covered.
struct Pass {
    /// What this pass is looking after, for the failure message.
    what: &'static str,
    /// Whether to document private items.
    private: bool,
}

/// The passes, in the order they run.
const PASSES: &[Pass] = &[
    Pass {
        what: "the published API",
        private: false,
    },
    Pass {
        what: "internal cross-references",
        private: true,
    },
];

/// Check that every doc comment builds and every doc link
/// resolves.
///
/// Prints `Doc OK` on success, or `FAILED` with the offending
/// lines.
pub fn doc() -> Result<(), String> {
    match doc_check()? {
        None => {
            println!("Doc OK");
            Ok(())
        }
        Some(failure) => {
            eprintln!("FAILED: {}\n", failure.summary);
            for line in failure.items.iter().take(MAX_DIAGNOSTIC_LINES) {
                eprintln!("  {line}");
            }
            if failure.items.len() > MAX_DIAGNOSTIC_LINES {
                eprintln!(
                    "  ... and {} more",
                    failure.items.len() - MAX_DIAGNOSTIC_LINES
                );
            }
            Err(failure.summary)
        }
    }
}

/// A failing rustdoc pass.
pub struct DocFailure {
    /// One-line description naming which pass failed.
    pub summary: String,
    /// The diagnostic lines, with their source locations.
    pub items: Vec<String>,
}

/// Runs both passes, returning the first failure.
///
/// `RUSTDOCFLAGS=-D warnings` is what makes this a gate rather
/// than a report: rustdoc's link lints are warnings by default,
/// so without it a broken link builds successfully and the
/// documentation quietly stops navigating.
///
/// # Errors
///
/// Returns `Err` only when rustdoc could not be run at all. A
/// pass that ran and found problems is `Ok(Some(..))`.
pub fn doc_check() -> Result<Option<DocFailure>, String> {
    for pass in PASSES {
        let mut args = vec!["doc", "--no-deps", "--workspace"];
        if pass.private {
            args.push("--document-private-items");
        }
        let output =
            run_cargo_capture_env(&args, &[("RUSTDOCFLAGS", "-D warnings")])?;
        if output.status.success() {
            continue;
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(Some(DocFailure {
            summary: format!("broken documentation in {}", pass.what),
            items: pair_with_locations(&stderr, is_diagnostic_line)
                .into_iter()
                .map(String::from)
                .collect(),
        }));
    }
    Ok(None)
}

/// True for a real rustdoc diagnostic, false for the cargo
/// summary lines that share the same prefix.
fn is_diagnostic_line(line: &str) -> bool {
    // `could not document` is cargo reporting that the crate
    // failed, which is the consequence rather than the cause, and
    // it appears once per crate. Reporting it alongside the real
    // diagnostic doubles the output and buries the useful half.
    let is_summary = line.contains("could not document")
        || line.contains("aborting due to")
        || line.contains("build failed")
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

    #[test]
    fn both_passes_run_and_differ_only_in_privacy() {
        // The whole value of this gate is that it runs twice.
        // A single pass would leave one class of broken link
        // unchecked -- see `Pass`'s doc for the measurement.
        assert_eq!(PASSES.len(), 2);
        assert!(!PASSES[0].private);
        assert!(PASSES[1].private);
    }

    #[test]
    fn extracts_the_diagnostic_and_its_location() {
        let stderr = "\
error: public documentation for `config` links to private item
  --> crates/bombyx/src/config.rs:12:7
error: could not document `bombyx`
warning: build failed, waiting for other jobs to finish...";
        let lines = pair_with_locations(stderr, is_diagnostic_line);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].contains("links to private item"));
        assert!(lines[1].contains("config.rs:12:7"));
    }

    #[test]
    fn drops_the_cargo_summary_but_keeps_a_real_warning() {
        // `could not document` is the consequence; without the
        // filter it appears once per crate and buries the cause.
        assert!(!is_diagnostic_line("error: could not document `bombyx`"));
        assert!(!is_diagnostic_line(
            "warning: build failed, waiting for other jobs"
        ));
        assert!(is_diagnostic_line("warning: unresolved link to `Foo`"));
        assert!(is_diagnostic_line("error[E0433]: failed to resolve"));
    }
}
