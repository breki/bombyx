//! Text on its way to the operator's terminal.
//!
//! Five pure functions, and they are here rather than in the
//! binary because `src/bin/` is outside the coverage gate: the
//! line-ending substitution below had no test while the far less
//! interesting question of where `-t` sits in an argv had four.
//!
//! The table covers the whole module. Only `line_endings` is
//! public; the other four are crate-private and so do not appear
//! on this page, which is also why comments elsewhere name
//! `sanitize` in backticks rather than linking to it.
//!
//! | Function | Answers |
//! |----------|---------|
//! | `line_endings` | how a line ends on a Windows console |
//! | `sanitize` | what the VM host is allowed to put on the screen |
//! | `clip` | how much of a detail fits one column |
//! | `fail_reason` | which line of a failed command explains it |
//! | `first_line` | which line of a successful one describes it |

use std::borrow::Cow;

/// Ends every line with `\r\n` when `crlf`, and changes nothing
/// otherwise.
///
/// **Why a bare `\n` is not always enough on Windows.** A console
/// normally supplies the carriage return itself; with the bit that
/// suppresses that behaviour set (`DISABLE_NEWLINE_AUTO_RETURN`), a
/// line feed becomes a *pure* line feed -- down one row, same
/// column -- and everything written afterwards staircases, each line
/// starting where the previous one ended. Measured from a real run:
/// line lengths 23, 66 and 130 against leading indents of 0, 23 and
/// 66.
///
/// What leaves the console in that state is **unverified**. The
/// observation is that it happens after a command that runs `ssh`
/// and not in `self-update`, which spawns children but never `ssh`.
///
/// The caller decides `crlf` rather than this function sampling a
/// global, and that is the point of the parameter. The decision
/// differs per stream -- stdout can be redirected while stderr is
/// still a console, and vice versa -- and reading `stdout` to choose
/// endings for `stderr` gets both cases wrong: the failure line
/// staircases when stdout alone is redirected, and a captured
/// `2> log` gains carriage returns when it is not.
///
/// Borrows when there is nothing to do, so the common non-Windows
/// path allocates nothing.
///
/// **Idempotent.** A plain `replace('\n', "\r\n")` turns text that
/// already ends its lines with `\r\n` into `\r\r\n`, which prints a
/// blank row between every line -- so any CR already present is
/// dropped before the translation. Nothing feeds this CRLF today,
/// but a caller that streamed output from a PTY would, and the
/// failure would look like a different bug.
#[must_use]
pub fn line_endings(text: &str, crlf: bool) -> Cow<'_, str> {
    if !crlf {
        return Cow::Borrowed(text);
    }
    if !text.contains('\n') {
        return Cow::Borrowed(text);
    }
    Cow::Owned(text.replace('\r', "").replace('\n', "\r\n"))
}

/// Whether `c` is safe to print in the report verbatim.
///
/// An **allowlist**, deliberately: printable ASCII and the space,
/// nothing else. The blocklist version was tried first and could
/// not be completed. It covered control characters and an
/// enumerated slice of the bidirectional and formatting
/// characters, and still missed `U+2028`/`U+2029` (line and
/// paragraph separators, which are not `is_control`), the
/// variation selectors, and the whole tag block
/// `U+E0000`-`U+E007F` -- which renders as nothing at all in
/// every terminal and is the standard way to hide text inside
/// text. Enumerating what to reject means tracking Unicode; the
/// report is ASCII everywhere else by design, so enumerating what
/// to keep is both shorter and finishable.
///
/// The cost is that a genuinely non-ASCII path on the VM host
/// renders with `?` in place of each such character. That is the
/// right trade for a report the operator reads to decide whether
/// to push: an unreadable character is obvious, and a character
/// that alters how the rest of the line appears is not.
fn is_safe_to_print(c: char) -> bool {
    c.is_ascii_graphic() || c == ' '
}

/// Replaces anything that could misrepresent the report with
/// `?`.
///
/// Probe details are text from the VM host, printed straight to
/// the operator's terminal. Without this, a host can emit
/// cursor-movement escapes and repaint the report -- turning a
/// `FAIL` line into `ok` on the screen while the exit code says
/// otherwise. The report is the artifact the operator trusts to
/// decide whether to push, so the host must not be able to write
/// it.
///
/// The renderer is the enforcement point, not the code that
/// builds the text. A value reaches a report from several places
/// -- including the binary, which builds one from spawn errors
/// and tool banners -- and requiring each of them to remember
/// the call is how one of them eventually does not.
///
/// What needs it is every value that came from a VM host. A
/// value the operator wrote in their own `config.toml` is
/// printed as written: bombyx trusts that file the way it trusts
/// a command-line argument.
pub(crate) fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| if is_safe_to_print(c) { c } else { '?' })
        .collect()
}

/// Shortens `detail` to at most `budget` characters.
///
/// ASCII `...`, not an ellipsis character: every other byte
/// bombyx prints is ASCII, and a legacy Windows console code
/// page renders U+2026 as mojibake.
///
/// A budget too small for the marker degrades to as much of the
/// marker as fits. Returning the untruncated detail would be
/// worse than useless -- the caller asked for a width because it
/// is building an aligned line, and one over-long detail there
/// pushes every column out of place.
pub(crate) fn clip(detail: &str, budget: usize) -> String {
    if detail.chars().count() <= budget {
        return detail.to_owned();
    }
    let marker = "...";
    if budget < marker.len() + 1 {
        return marker.chars().take(budget).collect();
    }
    let kept: String = detail.chars().take(budget - marker.len()).collect();
    format!("{kept}{marker}")
}

/// The most useful line explaining a failure.
///
/// Prefers the **last** non-blank stderr line. OpenSSH writes
/// the server's `Banner`, host-key notices and other chatter
/// before the real error, so taking the first line reports a
/// legal notice instead of `Permission denied (publickey)`.
/// Falls back to stdout, then to a fixed string, because a
/// failing `command -v` prints nothing at all.
pub(crate) fn fail_reason(stdout: &str, stderr: &str) -> String {
    for text in [stderr, stdout] {
        if let Some(line) = text.lines().map(str::trim).rfind(|l| !l.is_empty())
        {
            return sanitize(line);
        }
    }
    "not found".to_owned()
}

/// The first non-blank line of `text`, sanitized.
pub(crate) fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(sanitize)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_text_alone_when_not_asked() {
        let text = "one\ntwo\n";
        let out = line_endings(text, false);
        assert_eq!(out, "one\ntwo\n");
        // Borrowed, not copied: the untranslated path is the common
        // one and should cost nothing.
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    #[test]
    fn pairs_every_line_feed_with_a_carriage_return() {
        assert_eq!(line_endings("one\ntwo\n", true), "one\r\ntwo\r\n");
    }

    #[test]
    fn text_with_no_line_feed_is_unchanged_either_way() {
        assert_eq!(line_endings("bare", true), "bare");
        assert_eq!(line_endings("bare", false), "bare");
    }

    #[test]
    fn an_existing_carriage_return_is_not_doubled() {
        // The remote can already send CRLF -- under a PTY it does --
        // and translating that again would produce `\r\r\n`, which
        // prints a blank line between every row.
        assert_eq!(line_endings("one\r\ntwo\r\n", true), "one\r\ntwo\r\n");
    }

    #[test]
    fn an_empty_string_stays_empty() {
        assert_eq!(line_endings("", true), "");
    }

    #[test]
    fn clip_never_exceeds_the_budget_it_was_given() {
        assert_eq!(clip("abcdefgh", 5), "ab...");
        assert_eq!(clip("abc", 5), "abc");
        // A budget with no room for the marker must still shrink
        // the detail: the caller is building an aligned line, and
        // returning the full text would push every column out.
        for budget in 0..4 {
            let out = clip("abcdefgh", budget);
            assert_eq!(out.chars().count(), budget, "{budget}: {out:?}");
        }
    }
}
