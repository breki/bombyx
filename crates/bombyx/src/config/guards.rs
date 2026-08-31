//! Checks shared by more than one config field.
//!
//! A rule that applies to several fields lives here once, so
//! there is one implementation and one message. The alternative
//! is what it replaces: the same `starts_with('-')` test written
//! out twice, with two different explanations, where widening
//! one of them leaves the other behind.
//!
//! Everything here returns [`FieldError`], not `ConfigError`.
//! These are checks on a value; whether that value came from a
//! file is the caller's business. See `config::error`.

use std::path::{Component, Path};

use super::error::FieldError;

/// Characters allowed in a path on the VM host.
pub(super) fn is_remote_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | '~')
}

/// Requires a value that is not blank.
pub(super) fn check_not_empty(
    field: &'static str,
    value: &str,
) -> Result<(), FieldError> {
    if value.trim().is_empty() {
        return Err(FieldError::Empty { field });
    }
    Ok(())
}

/// Refuses a value the named tool would treat as an option.
///
/// Command-line tools tell options from ordinary values by the
/// leading `-`. So a config value starting with `-` that we hand
/// to a program is read as an instruction to that program
/// instead of as data.
///
/// `tool` names the program in the message, because the answer
/// to "which program?" is what tells the operator where to
/// look: `host` reaches `ssh`, `ref` reaches `git`.
///
/// Worth knowing, because many tools do not work this way:
/// these accept options *after* positional arguments. So it is
/// not enough for the value to be in the last position.
pub(super) fn check_not_an_option(
    field: &'static str,
    value: &str,
    tool: &str,
) -> Result<(), FieldError> {
    if value.starts_with('-') {
        return Err(FieldError::invalid(
            field,
            &format!(
                "must not start with `-`, which {tool} reads as an option"
            ),
        ));
    }
    Ok(())
}

/// Requires every character of `value` to be one `allowed`
/// accepts, naming `expected` in the message when one is not.
pub(super) fn check_charset(
    field: &'static str,
    value: &str,
    allowed: fn(char) -> bool,
    expected: &str,
) -> Result<(), FieldError> {
    if let Some(bad) = value.chars().find(|c| !allowed(*c)) {
        return Err(FieldError::invalid(
            field,
            &format!("character {bad:?} is not allowed; use only {expected}"),
        ));
    }
    Ok(())
}

/// Requires a path that stays inside the project directory.
///
/// The value gets joined onto the working directory, and
/// `Path::join` **discards the left side** when the right one is
/// absolute. So an absolute value does not extend the project
/// path, it replaces it -- and since this config travels inside
/// a repository, that turns a clone into a tool that archives
/// whatever directory the repo names.
///
/// The rooted spellings are tested by hand rather than through
/// `Path::is_absolute`, because that answers differently per
/// platform: a Windows drive prefix is not absolute on Unix, and
/// the same config file gets read on both.
pub(super) fn check_project_relative(
    field: &'static str,
    value: &str,
) -> Result<(), FieldError> {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(FieldError::invalid(field, "must not name a drive"));
    }
    if value.starts_with('/') || value.starts_with('\\') {
        return Err(FieldError::invalid(
            field,
            "must be relative to the project directory",
        ));
    }
    if value.starts_with('~') {
        return Err(FieldError::invalid(field, "must not start with `~`"));
    }

    // Everything left must be an ordinary segment. This is what
    // rejects `..` and `.`, in any position rather than only at
    // the front.
    for component in Path::new(value).components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(FieldError::invalid(
                field,
                "must be a plain relative path, with no `.`, `..` or root",
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_option_message_names_the_tool_that_would_be_fooled() {
        // One rule, two callers, and the message has to send
        // the operator to the right place: `host` reaches `ssh`,
        // `ref` reaches `git`.
        let err = check_not_an_option("host", "-oProxyCommand=x", "ssh")
            .expect_err("must be refused");
        assert!(err.to_string().contains("ssh"), "{err}");

        let err = check_not_an_option("ref", "--upload-pack=x", "git")
            .expect_err("must be refused");
        assert!(err.to_string().contains("git"), "{err}");

        assert!(check_not_an_option("ref", "main", "git").is_ok());
    }
}
