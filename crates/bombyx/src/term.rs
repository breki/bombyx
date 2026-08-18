//! Writing text out the way a terminal needs to read it.
//!
//! One pure function, here rather than in the binary because
//! `src/bin/` is outside the coverage gate: the substitution below
//! had no test while the far less interesting question of where `-t`
//! sits in an argv had four.

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
}
