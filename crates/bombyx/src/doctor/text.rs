//! Two operations on untrusted detail on its way to the report:
//! replacing what could misrepresent it, and fitting it in a
//! column. Why each matters is at the function that does it.

/// The most useful line explaining a failure.
///
/// Prefers the **last** non-blank stderr line. OpenSSH writes
/// the server's `Banner`, host-key notices and other chatter
/// before the real error, so taking the first line reports a
/// legal notice instead of `Permission denied (publickey)`.
/// Falls back to stdout, then to a fixed string, because a
/// failing `command -v` prints nothing at all.
pub(super) fn fail_reason(stdout: &str, stderr: &str) -> String {
    for text in [stderr, stdout] {
        if let Some(line) = text.lines().map(str::trim).rfind(|l| !l.is_empty())
        {
            return sanitize(line);
        }
    }
    "not found".to_owned()
}

/// The first non-blank line of `text`, sanitized.
pub(super) fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(sanitize)
        .unwrap_or_default()
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
/// [`super::report::Report::render`] is the enforcement point,
/// not this
/// function's callers. Details reach a `Finding` from several
/// places -- including the binary, which builds them from
/// spawn errors and tool banners -- and requiring each one to
/// remember the call is how one of them eventually does not.
/// The earlier calls stay because an `Outcome` is also read
/// programmatically.
pub(super) fn sanitize(text: &str) -> String {
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
pub(super) fn clip(detail: &str, budget: usize) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
