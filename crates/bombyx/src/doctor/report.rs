//! Collecting findings and rendering the aligned report.

use std::fmt::Write as _;

use super::text::{clip, sanitize};
use super::{Finding, Outcome, Scope};

/// Width the rendered report aims to fit inside.
const LINE_WIDTH: usize = 80;

/// Indent every report line carries.
const INDENT: usize = 2;

/// Gap between report columns.
const GAP: usize = 2;

/// Width of the `ok`/`FAIL`/`skip` column.
const TAG_WIDTH: usize = 4;

/// Least detail a line will show, whatever the column widths.
///
/// The budget is `LINE_WIDTH` minus the prefix, and the prefix
/// grows with the host name, which `Config` does not bound. A
/// 49-character host left three characters of detail and a
/// 52-character one left none, so `FAIL` printed with no reason
/// at all -- a report that still looked complete and aligned
/// while having discarded the only actionable content in it.
/// Past this floor the line is allowed to run over 80 columns
/// instead: a wrapped reason can be read, a deleted one cannot.
const MIN_DETAIL: usize = 24;

/// Scope label for a check that runs on this workstation.
///
/// A constant because it is both printed and measured, and the
/// two have to be the same string: written twice, renaming it to
/// anything longer would silently misalign every line while the
/// column width still described the old label.
const LOCAL_LABEL: &str = "local";

/// The collected findings of a doctor run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    findings: Vec<Finding>,
}

impl Report {
    /// Appends a finding, preserving probe order.
    ///
    /// Named `add` rather than `push`/`extend`: those are the
    /// `Vec` and `Extend` spellings, and borrowing them for a
    /// type that is not a collection invites a reader to assume
    /// the rest of that interface exists.
    pub fn add(&mut self, finding: Finding) {
        self.findings.push(finding);
    }

    /// Appends several findings.
    pub fn add_all(&mut self, findings: impl IntoIterator<Item = Finding>) {
        self.findings.extend(findings);
    }

    /// The findings, in probe order.
    #[must_use]
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// Whether every probe that ran found its precondition met.
    ///
    /// A skip does not count against the report: it is not
    /// evidence of a problem, and the failure that caused it is
    /// already on its own line.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.failures() == 0
    }

    /// How many probes failed.
    #[must_use]
    pub fn failures(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| matches!(f.outcome, Outcome::Fail(_)))
            .count()
    }

    /// Renders the report, aligned, with a closing summary.
    ///
    /// This is where host-supplied text is made safe to print;
    /// see `text::sanitize`.
    #[must_use]
    pub fn render(&self, host: &str) -> String {
        let name_width = self
            .findings
            .iter()
            .map(|f| f.name.chars().count())
            .max()
            .unwrap_or(0);
        // Measured in characters, like the host name beside it:
        // a byte count would misalign a non-ASCII label.
        let scope_width = host.chars().count().max(LOCAL_LABEL.chars().count());
        let prefix =
            INDENT + scope_width + GAP + name_width + GAP + TAG_WIDTH + GAP;
        let budget = LINE_WIDTH.saturating_sub(prefix).max(MIN_DETAIL);
        let mut out = String::new();
        for f in &self.findings {
            let scope = match f.scope {
                Scope::Local => LOCAL_LABEL,
                Scope::Host => host,
            };
            let (tag, detail) = match &f.outcome {
                Outcome::Pass(d) => ("ok", d.as_str()),
                Outcome::Fail(d) => ("FAIL", d.as_str()),
                Outcome::Skip(d) => ("skip", d.as_str()),
            };
            // One format, then trim: a line ending in
            // whitespace is noise in a diff and in a paste.
            let line = format!(
                "{blank:INDENT$}{scope:<scope_width$}{blank:GAP$}\
                 {name:<name_width$}{blank:GAP$}{tag:<TAG_WIDTH$}\
                 {blank:GAP$}{detail}",
                blank = "",
                name = f.name,
                detail = sanitize(&clip(detail, budget)),
            );
            let _ = writeln!(out, "{}", line.trim_end());
        }
        match self.failures() {
            0 => out.push_str("all checks passed\n"),
            1 => out.push_str("1 check failed\n"),
            n => {
                let _ = writeln!(out, "{n} checks failed");
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host_finding(name: &str, outcome: Outcome) -> Finding {
        Finding::new(Scope::Host, name, outcome)
    }

    #[test]
    fn details_cannot_repaint_the_report() {
        // A host emitting cursor escapes could otherwise
        // overwrite a FAIL line with `ok`. That `classify` also
        // sanitizes what it stores is `probes`' business and is
        // asserted there; welding both halves into one test would
        // leave a failure unable to say which module broke.
        let evil = "\x1b[2A\x1b[Kspoofed";
        let mut r = Report::default();
        r.add(host_finding("x", Outcome::Fail(evil.to_owned())));
        assert!(!r.render("h").contains('\x1b'));
    }

    #[test]
    fn render_is_the_enforcement_point_for_untrusted_detail() {
        // A `Finding` can be built anywhere, including in the
        // binary, so the guarantee cannot depend on each
        // producer remembering to sanitize.
        let mut r = Report::default();
        r.add(Finding::new(
            Scope::Local,
            "tar",
            // A right-to-left override reverses the run after
            // it, so the tag can be made to read backwards.
            Outcome::Pass("bsdtar\u{202e}3.8.4\u{200b}".to_owned()),
        ));
        let out = r.render("h");
        assert!(out.contains("bsdtar?3.8.4?"), "{out:?}");
        assert!(!out.contains('\u{202e}'), "{out:?}");
    }

    #[test]
    fn a_skip_is_not_a_failure_but_does_not_inflate_the_count() {
        let mut r = Report::default();
        r.add(host_finding("ssh", Outcome::Fail("down".into())));
        r.add(host_finding("tar", Outcome::Skip("no ssh".into())));
        assert!(!r.ok());
        assert_eq!(r.failures(), 1);

        let mut only_skips = Report::default();
        only_skips.add(host_finding("tar", Outcome::Skip("no ssh".into())));
        assert!(only_skips.ok(), "skips alone must not fail the report");
    }

    #[test]
    fn render_aligns_names_the_host_and_leaves_no_trailing_space() {
        let mut r = Report::default();
        r.add(Finding::new(
            Scope::Local,
            "tar",
            Outcome::Pass("bsdtar 3.8.4".into()),
        ));
        r.add(host_finding("ssh", Outcome::Pass(String::new())));
        // Compared line by line: one concatenated string with
        // escaped runs of spaces is unreadable, so a wrong
        // expectation is as easy to write as a right one.
        assert_eq!(
            r.render("frosti").lines().collect::<Vec<_>>(),
            vec![
                "  local   tar  ok    bsdtar 3.8.4",
                "  frosti  ssh  ok",
                "all checks passed",
            ]
        );
        for line in r.render("frosti").lines() {
            assert_eq!(line, line.trim_end());
        }
    }

    #[test]
    fn render_keeps_lines_inside_the_width_for_a_long_host_name() {
        // The budget is derived from the prefix, so a long host
        // name still yields a line that fits.
        let mut r = Report::default();
        r.add(host_finding(
            "libvirt provider",
            Outcome::Pass("x".repeat(200)),
        ));
        let out = r.render("vmhost.internal.example.com");
        let line = out.lines().next().unwrap();
        assert!(line.chars().count() <= LINE_WIDTH, "{line:?}");
        assert!(line.ends_with("..."), "{line:?}");
    }

    #[test]
    fn a_long_host_name_cannot_delete_the_failure_reason() {
        // The budget is 80 minus the prefix, and the prefix grows
        // with the host name, which `Config` does not bound. At 49
        // characters the detail was three dots; at 52 it was
        // empty, so `FAIL` printed with no reason at all while the
        // report still looked complete. Past the floor the line is
        // allowed to run long instead.
        let host = "deploy-user@vmhost.internal.example.company.com.au";
        let mut r = Report::default();
        r.add(host_finding(
            "libvirt provider",
            Outcome::Fail("Permission denied (publickey).".to_owned()),
        ));
        let line = r.render(host).lines().next().unwrap().to_owned();
        assert!(line.contains("Permission denied"), "{line:?}");
        assert!(
            line.len() > LINE_WIDTH,
            "expected an over-long line: {line}"
        );
    }

    #[test]
    fn only_printable_ascii_survives_into_the_report() {
        // An allowlist, so the characters a blocklist kept
        // missing are covered by construction: the line and
        // paragraph separators (which are not `is_control`), the
        // variation selectors, and the tag block that renders as
        // nothing at all and is the standard way to hide text
        // inside text.
        for hidden in [
            '\u{1b}',    // escape, the cursor-movement lead-in
            '\u{202e}',  // right-to-left override
            '\u{200b}',  // zero-width space
            '\u{2028}',  // line separator
            '\u{2029}',  // paragraph separator
            '\u{fe0f}',  // variation selector
            '\u{e0041}', // tag block: invisible everywhere
            '\u{feff}',  // byte-order mark
        ] {
            let detail = format!("vagrant-libvirt{hidden}-fork");
            let mut r = Report::default();
            r.add(host_finding("x", Outcome::Pass(detail)));
            let out = r.render("h");
            assert!(!out.contains(hidden), "{hidden:?} survived: {out:?}");
            assert!(out.contains("vagrant-libvirt?-fork"), "{out:?}");
        }
    }

    #[test]
    fn render_counts_failures_in_the_summary() {
        let mut r = Report::default();
        r.add(host_finding("a", Outcome::Fail("x".into())));
        assert!(r.render("h").ends_with("1 check failed\n"));
        r.add(host_finding("b", Outcome::Fail("y".into())));
        assert!(r.render("h").ends_with("2 checks failed\n"));
    }
}
